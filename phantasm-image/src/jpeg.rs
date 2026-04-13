//! Safe wrapper around mozjpeg-sys DCT coefficient I/O.
//!
//! The write path re-opens the original source file to copy critical parameters
//! (quant tables, restart intervals, progressive/baseline mode). Huffman tables
//! are re-optimized for the (possibly modified) coefficient distribution via
//! `cinfo.optimize_coding = TRUE`. Markers (EXIF, ICC, etc.) are preserved via
//! `jcopy_markers_setup` / `jcopy_markers_execute`.
//!
//! Fatal libjpeg errors are trapped by a custom `error_exit` callback that
//! panics across the `C-unwind` ABI boundary; each public entry point wraps
//! its unsafe body in `catch_unwind` and converts panics into
//! `ImageError::LibjpegError(..)`. All libjpeg/libc resources use drop guards
//! so unwinding still releases them.

use crate::error::ImageError;
use libc::FILE;
use mozjpeg_sys::*;
use std::any::Any;
use std::cell::UnsafeCell;
use std::ffi::CString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

pub struct JpegCoefficients {
    pub components: Vec<JpegComponent>,
    pub width: u32,
    pub height: u32,
    /// Estimated quality factor from the luminance quantization table.
    /// Rough approximation only; may be off by ±5–10 QF units.
    pub quality_estimate: Option<u8>,
    pub markers: Vec<JpegMarker>,
}

pub struct JpegComponent {
    pub id: u8,
    pub blocks_wide: usize,
    pub blocks_high: usize,
    /// Row-major over blocks; 64 coefficients per block (zigzag order).
    pub coefficients: Vec<i16>,
    /// Zigzag-indexed quantization table for this component.
    pub quant_table: [u16; 64],
    pub h_samp_factor: u8,
    pub v_samp_factor: u8,
}

pub struct JpegMarker {
    pub marker: u8,
    pub data: Vec<u8>,
}

impl JpegComponent {
    pub fn get(&self, block_row: usize, block_col: usize, dct_pos: usize) -> i16 {
        self.coefficients[block_row * self.blocks_wide * 64 + block_col * 64 + dct_pos]
    }

    pub fn set(&mut self, block_row: usize, block_col: usize, dct_pos: usize, value: i16) {
        self.coefficients[block_row * self.blocks_wide * 64 + block_col * 64 + dct_pos] = value;
    }

    pub fn coeff_count(&self) -> usize {
        self.coefficients.len()
    }
}

// ---------------------------------------------------------------------------
// Panic-based error manager
// ---------------------------------------------------------------------------

/// Typed panic payload raised by our `error_exit` callback. We match on this
/// in `catch_unwind` sites and convert it into `ImageError::LibjpegError`.
struct LibjpegPanic(String);

/// Custom `error_exit`: format the libjpeg message and panic across the
/// `extern "C-unwind"` ABI. Every FFI entry point is wrapped in
/// `catch_unwind`, so the panic is caught at the Rust boundary and turned
/// into an `Err` — libjpeg never gets to call `exit()`.
unsafe extern "C-unwind" fn rust_error_exit(cinfo: &mut jpeg_common_struct) {
    // libjpeg's `format_message` writes up to 80 bytes (JMSG_LENGTH_MAX) through
    // the buffer pointer. The upstream bindgen signature is marked "incorrect"
    // (`&[u8; 80]` instead of `&mut`), so we back the buffer with `UnsafeCell`
    // and hand out a shared ref — same underlying memory, no UB from writing
    // through an immutable reference.
    let cell: UnsafeCell<[u8; 80]> = UnsafeCell::new([0u8; 80]);
    if let Some(fmt) = (*cinfo.err).format_message {
        let buf_ref: &[u8; 80] = &*cell.get();
        fmt(cinfo, buf_ref);
    }
    let buffer = *cell.get();
    let nul = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    let msg = String::from_utf8_lossy(&buffer[..nul]).into_owned();
    let msg = if msg.is_empty() {
        format!("libjpeg fatal error (code {})", (*cinfo.err).msg_code)
    } else {
        msg
    };
    std::panic::panic_any(LibjpegPanic(msg));
}

/// Silence non-fatal warning/trace output so noisy corpora don't pollute stderr.
unsafe extern "C-unwind" fn rust_output_message(_cinfo: &mut jpeg_common_struct) {}

fn install_error_mgr(err: &mut jpeg_error_mgr) {
    unsafe {
        jpeg_std_error(err);
    }
    err.error_exit = Some(rust_error_exit);
    err.output_message = Some(rust_output_message);
}

fn panic_to_error(payload: Box<dyn Any + Send>) -> ImageError {
    if let Some(msg) = payload.downcast_ref::<LibjpegPanic>() {
        ImageError::LibjpegError(msg.0.clone())
    } else if let Some(s) = payload.downcast_ref::<String>() {
        ImageError::FfiFailure(s.clone())
    } else if let Some(s) = payload.downcast_ref::<&'static str>() {
        ImageError::FfiFailure((*s).to_string())
    } else {
        ImageError::FfiFailure("unknown panic in libjpeg FFI".to_string())
    }
}

// ---------------------------------------------------------------------------
// RAII drop guards for libjpeg + libc resources
// ---------------------------------------------------------------------------

struct FileGuard(*mut FILE);

impl FileGuard {
    fn as_ptr(&self) -> *mut FILE {
        self.0
    }
}

impl Drop for FileGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                libc::fclose(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

struct DecompressGuard {
    cinfo: Box<jpeg_decompress_struct>,
}

impl DecompressGuard {
    fn new(err: *mut jpeg_error_mgr) -> Self {
        let mut cinfo: Box<jpeg_decompress_struct> = Box::new(unsafe { std::mem::zeroed() });
        cinfo.common.err = err;
        unsafe { jpeg_create_decompress(&mut *cinfo) };
        Self { cinfo }
    }
}

impl Drop for DecompressGuard {
    fn drop(&mut self) {
        unsafe {
            // Safe to call in any state after `jpeg_create_decompress`; frees
            // any partial allocations including when we unwind mid-decode.
            jpeg_destroy_decompress(&mut self.cinfo);
        }
    }
}

struct CompressGuard {
    cinfo: Box<jpeg_compress_struct>,
}

impl CompressGuard {
    fn new(err: *mut jpeg_error_mgr) -> Self {
        let mut cinfo: Box<jpeg_compress_struct> = Box::new(unsafe { std::mem::zeroed() });
        cinfo.common.err = err;
        unsafe { jpeg_create_compress(&mut *cinfo) };
        Self { cinfo }
    }
}

impl Drop for CompressGuard {
    fn drop(&mut self) {
        unsafe {
            jpeg_destroy_compress(&mut self.cinfo);
        }
    }
}

// ---------------------------------------------------------------------------
// libc file helpers
// ---------------------------------------------------------------------------

fn open_read(path: &Path) -> Result<FileGuard, ImageError> {
    let s = CString::new(
        path.to_str()
            .ok_or_else(|| ImageError::InvalidFormat("non-UTF-8 path".to_string()))?,
    )
    .map_err(|e| ImageError::InvalidFormat(e.to_string()))?;
    let mode = CString::new("rb").unwrap();
    let fp = unsafe { libc::fopen(s.as_ptr(), mode.as_ptr()) };
    if fp.is_null() {
        Err(ImageError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(FileGuard(fp))
    }
}

fn open_write(path: &Path) -> Result<FileGuard, ImageError> {
    let s = CString::new(
        path.to_str()
            .ok_or_else(|| ImageError::InvalidFormat("non-UTF-8 path".to_string()))?,
    )
    .map_err(|e| ImageError::InvalidFormat(e.to_string()))?;
    let mode = CString::new("wb").unwrap();
    let fp = unsafe { libc::fopen(s.as_ptr(), mode.as_ptr()) };
    if fp.is_null() {
        Err(ImageError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(FileGuard(fp))
    }
}

// ---------------------------------------------------------------------------
// Quality estimation from luminance quant table
// ---------------------------------------------------------------------------

/// Estimate JPEG quality factor from the luminance quantization table.
///
/// Uses the libjpeg inverse heuristic: the standard QF=50 luma table has an
/// average entry of ~32.  We compare against that to approximate QF.
/// Accuracy is ±5–10 QF units; do not rely on this for exact reconstruction.
fn estimate_quality(quant: &[u16; 64]) -> u8 {
    // Standard JPEG luminance quantization table (QF=50 baseline)
    #[rustfmt::skip]
    const STD_LUMA_Q50: [u16; 64] = [
        16, 11, 10, 16,  24,  40,  51,  61,
        12, 12, 14, 19,  26,  58,  60,  55,
        14, 13, 16, 24,  40,  57,  69,  56,
        14, 17, 22, 29,  51,  87,  80,  62,
        18, 22, 37, 56,  68, 109, 103,  77,
        24, 35, 55, 64,  81, 104, 113,  92,
        49, 64, 78, 87, 103, 121, 120, 101,
        72, 92, 95, 98, 112, 100, 103,  99,
    ];

    // libjpeg: if scale < 100 → QF = 50 + (50*(100-scale))/100
    //          if scale >= 100 → QF = 5000/scale
    let scale_sum: f64 = quant
        .iter()
        .zip(STD_LUMA_Q50.iter())
        .map(|(&q, &s)| q as f64 / s as f64)
        .sum::<f64>()
        / 64.0;

    let scale = (scale_sum * 100.0).round() as i32;
    let qf = if scale <= 0 {
        100i32
    } else if scale < 100 {
        50 + (50 * (100 - scale)) / 100
    } else {
        5000 / scale
    };
    qf.clamp(1, 100) as u8
}

// ---------------------------------------------------------------------------
// Public API — every entry point is wrapped in `catch_unwind`
// ---------------------------------------------------------------------------

pub fn read(path: &Path) -> Result<JpegCoefficients, ImageError> {
    match catch_unwind(AssertUnwindSafe(|| unsafe { read_inner(path) })) {
        Ok(res) => res,
        Err(payload) => Err(panic_to_error(payload)),
    }
}

/// Write coefficients using the original source file to copy critical parameters.
///
/// `source_path` must be the original JPEG file that was read to produce
/// `coefficients`. The file must still exist. `dest_path` is the output file.
///
/// Huffman tables are re-optimized from the modified coefficient histogram
/// (`cinfo.optimize_coding = TRUE`), so file-size inflation from embedding is
/// kept near zero.
pub fn write_with_source(
    coefficients: &JpegCoefficients,
    source_path: &Path,
    dest_path: &Path,
) -> Result<(), ImageError> {
    write_with_source_opts(coefficients, source_path, dest_path, true)
}

/// Lower-level variant that lets the caller disable Huffman re-optimization.
///
/// Production callers should use [`write_with_source`]; this hook exists so
/// benchmarks and research harnesses can measure the effect of re-optimization
/// directly.
pub fn write_with_source_opts(
    coefficients: &JpegCoefficients,
    source_path: &Path,
    dest_path: &Path,
    optimize_huffman: bool,
) -> Result<(), ImageError> {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        write_with_source_inner(coefficients, source_path, dest_path, optimize_huffman)
    })) {
        Ok(res) => res,
        Err(payload) => Err(panic_to_error(payload)),
    }
}

// ---------------------------------------------------------------------------
// Unsafe implementations (always reached through catch_unwind)
// ---------------------------------------------------------------------------

unsafe fn read_inner(path: &Path) -> Result<JpegCoefficients, ImageError> {
    let fp = open_read(path)?;

    let mut err: jpeg_error_mgr = std::mem::zeroed();
    install_error_mgr(&mut err);

    let mut dec = DecompressGuard::new(&mut err);
    let cinfo: &mut jpeg_decompress_struct = &mut dec.cinfo;
    jpeg_stdio_src(cinfo, fp.as_ptr());

    // Save all APP and COM markers so we can copy them out.
    for marker in 0u32..15 {
        jpeg_save_markers(cinfo, (0xe0 + marker) as i32, 0xffff);
    }
    jpeg_save_markers(cinfo, 0xfe, 0xffff); // COM marker

    jpeg_read_header(cinfo, true as _);

    let width = cinfo.image_width;
    let height = cinfo.image_height;
    let num_components = cinfo.num_components as usize;

    // Collect preserved markers.
    let mut markers = Vec::new();
    let mut marker_ptr = cinfo.marker_list;
    while !marker_ptr.is_null() {
        let m = &*marker_ptr;
        let data_len = m.data_length as usize;
        let data = std::slice::from_raw_parts(m.data, data_len).to_vec();
        markers.push(JpegMarker {
            marker: m.marker,
            data,
        });
        marker_ptr = m.next;
    }

    let coef_arrays = jpeg_read_coefficients(cinfo);
    let comp_info_slice = std::slice::from_raw_parts(cinfo.comp_info, num_components);

    let mut components = Vec::with_capacity(num_components);

    for (ci, comp) in comp_info_slice.iter().enumerate() {
        let bw = comp.width_in_blocks as usize;
        let bh = comp.height_in_blocks as usize;
        let total = bw * bh * 64;
        let mut coeffs = vec![0i16; total];

        for row in 0..bh {
            let block_row_ptr = ((*cinfo.common.mem).access_virt_barray.unwrap())(
                &mut cinfo.common,
                *coef_arrays.add(ci),
                row as u32,
                1,
                false as _,
            );
            let row_ptr: *mut JBLOCK = *block_row_ptr;
            for col in 0..bw {
                let block: &JBLOCK = &*row_ptr.add(col);
                let base = row * bw * 64 + col * 64;
                coeffs[base..base + 64].copy_from_slice(block);
            }
        }

        let mut quant_table = [1u16; 64];
        if !comp.quant_table.is_null() {
            let qt = &*comp.quant_table;
            quant_table.copy_from_slice(&qt.quantval);
        }

        components.push(JpegComponent {
            id: comp.component_id as u8,
            blocks_wide: bw,
            blocks_high: bh,
            coefficients: coeffs,
            quant_table,
            h_samp_factor: comp.h_samp_factor as u8,
            v_samp_factor: comp.v_samp_factor as u8,
        });
    }

    let quality_estimate = if !components.is_empty() {
        Some(estimate_quality(&components[0].quant_table))
    } else {
        None
    };

    jpeg_finish_decompress(cinfo);
    // `dec` + `fp` drop at end of scope → destroy_decompress + fclose.
    drop(dec);
    drop(fp);

    Ok(JpegCoefficients {
        components,
        width,
        height,
        quality_estimate,
        markers,
    })
}

unsafe fn write_with_source_inner(
    jc: &JpegCoefficients,
    source_path: &Path,
    dest_path: &Path,
    optimize_huffman: bool,
) -> Result<(), ImageError> {
    let src_fp = open_read(source_path)?;
    let dst_fp = open_write(dest_path)?;

    let mut src_err: jpeg_error_mgr = std::mem::zeroed();
    install_error_mgr(&mut src_err);
    let mut src_dec = DecompressGuard::new(&mut src_err);
    let src_cinfo: &mut jpeg_decompress_struct = &mut src_dec.cinfo;
    jpeg_stdio_src(src_cinfo, src_fp.as_ptr());

    jcopy_markers_setup(src_cinfo, JCOPY_OPTION_JCOPYOPT_ALL);

    jpeg_read_header(src_cinfo, true as _);
    let orig_coef_arrays = jpeg_read_coefficients(src_cinfo);

    let num_components = src_cinfo.num_components as usize;
    let comp_info_slice = std::slice::from_raw_parts(src_cinfo.comp_info, num_components);

    // Overwrite virtual arrays with our (possibly modified) coefficients.
    for (ci, comp) in comp_info_slice.iter().enumerate() {
        if ci >= jc.components.len() {
            break;
        }
        let our = &jc.components[ci];
        let bw = comp.width_in_blocks as usize;
        let bh = comp.height_in_blocks as usize;

        for row in 0..bh {
            let block_row_ptr = ((*src_cinfo.common.mem).access_virt_barray.unwrap())(
                &mut src_cinfo.common,
                *orig_coef_arrays.add(ci),
                row as u32,
                1,
                true as _,
            );
            let row_ptr: *mut JBLOCK = *block_row_ptr;
            for col in 0..bw {
                let block: &mut JBLOCK = &mut *row_ptr.add(col);
                let base = row * bw * 64 + col * 64;
                block.copy_from_slice(&our.coefficients[base..base + 64]);
            }
        }
    }

    let mut dst_err: jpeg_error_mgr = std::mem::zeroed();
    install_error_mgr(&mut dst_err);
    let mut dst_enc = CompressGuard::new(&mut dst_err);
    let dst_cinfo: &mut jpeg_compress_struct = &mut dst_enc.cinfo;
    jpeg_stdio_dest(dst_cinfo, dst_fp.as_ptr());

    jpeg_copy_critical_parameters(src_cinfo, dst_cinfo);

    // Re-optimize Huffman tables against the (possibly modified) coefficient
    // distribution. `jpeg_copy_critical_parameters` copies quant tables but
    // leaves Huffman tables untouched; with `optimize_coding = TRUE` libjpeg
    // builds fresh optimal tables during `jpeg_write_coefficients`.
    dst_cinfo.optimize_coding = if optimize_huffman { 1 } else { 0 } as boolean;

    jpeg_write_coefficients(dst_cinfo, orig_coef_arrays);
    jcopy_markers_execute(src_cinfo, dst_cinfo, JCOPY_OPTION_JCOPYOPT_ALL);
    jpeg_finish_compress(dst_cinfo);

    jpeg_finish_decompress(src_cinfo);

    drop(dst_enc);
    drop(src_dec);
    drop(dst_fp);
    drop(src_fp);

    Ok(())
}
