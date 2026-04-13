//! Safe wrapper around mozjpeg-sys DCT coefficient I/O.
//!
//! The write path re-opens the original source file to copy critical parameters
//! (quant tables, Huffman tables, restart intervals, progressive/baseline mode).
//! Markers (EXIF, ICC, etc.) are preserved via `jpeg_save_markers`.

use crate::error::ImageError;
use libc::FILE;
use mozjpeg_sys::*;
use std::ffi::CString;
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
// Internal C FILE helpers
// ---------------------------------------------------------------------------

unsafe fn fopen_read(path: &Path) -> Result<*mut FILE, ImageError> {
    let s = CString::new(
        path.to_str()
            .ok_or_else(|| ImageError::InvalidFormat("non-UTF-8 path".to_string()))?,
    )
    .map_err(|e| ImageError::InvalidFormat(e.to_string()))?;
    let mode = CString::new("rb").unwrap();
    let fp = libc::fopen(s.as_ptr(), mode.as_ptr());
    if fp.is_null() {
        Err(ImageError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(fp)
    }
}

unsafe fn fopen_write(path: &Path) -> Result<*mut FILE, ImageError> {
    let s = CString::new(
        path.to_str()
            .ok_or_else(|| ImageError::InvalidFormat("non-UTF-8 path".to_string()))?,
    )
    .map_err(|e| ImageError::InvalidFormat(e.to_string()))?;
    let mode = CString::new("wb").unwrap();
    let fp = libc::fopen(s.as_ptr(), mode.as_ptr());
    if fp.is_null() {
        Err(ImageError::Io(std::io::Error::last_os_error()))
    } else {
        Ok(fp)
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

    // Compute ratio of provided table to QF=50 standard table.
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
// Public API
// ---------------------------------------------------------------------------

pub fn read(path: &Path) -> Result<JpegCoefficients, ImageError> {
    unsafe { read_inner(path) }
}

/// Write coefficients using the original source file to copy critical parameters.
///
/// `source_path` must be the original JPEG file that was read to produce `coefficients`.
/// The file must still exist. `dest_path` is the output file.
pub fn write_with_source(
    coefficients: &JpegCoefficients,
    source_path: &Path,
    dest_path: &Path,
) -> Result<(), ImageError> {
    unsafe { write_with_source_inner(coefficients, source_path, dest_path) }
}

unsafe fn read_inner(path: &Path) -> Result<JpegCoefficients, ImageError> {
    let fp = fopen_read(path)?;

    let mut err: jpeg_error_mgr = std::mem::zeroed();
    let mut cinfo: jpeg_decompress_struct = std::mem::zeroed();
    cinfo.common.err = jpeg_std_error(&mut err);
    jpeg_create_decompress(&mut cinfo);
    jpeg_stdio_src(&mut cinfo, fp);

    // Save all APP and COM markers so we can copy them out.
    for marker in 0u32..15 {
        jpeg_save_markers(&mut cinfo, (0xe0 + marker) as i32, 0xffff);
    }
    jpeg_save_markers(&mut cinfo, 0xfe, 0xffff); // COM marker

    jpeg_read_header(&mut cinfo, true as _);

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

    let coef_arrays = jpeg_read_coefficients(&mut cinfo);
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

        // Extract quantization table for this component.
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

    jpeg_finish_decompress(&mut cinfo);
    jpeg_destroy_decompress(&mut cinfo);
    libc::fclose(fp);

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
) -> Result<(), ImageError> {
    let src_fp = fopen_read(source_path)?;
    let dst_fp = match fopen_write(dest_path) {
        Ok(fp) => fp,
        Err(e) => {
            libc::fclose(src_fp);
            return Err(e);
        }
    };

    let mut src_err: jpeg_error_mgr = std::mem::zeroed();
    let mut src_cinfo: jpeg_decompress_struct = std::mem::zeroed();
    src_cinfo.common.err = jpeg_std_error(&mut src_err);
    jpeg_create_decompress(&mut src_cinfo);
    jpeg_stdio_src(&mut src_cinfo, src_fp);

    // Set up marker saving so jcopy_markers_execute can replay them into the output.
    jcopy_markers_setup(&mut src_cinfo, JCOPY_OPTION_JCOPYOPT_ALL);

    jpeg_read_header(&mut src_cinfo, true as _);
    let orig_coef_arrays = jpeg_read_coefficients(&mut src_cinfo);

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
    let mut dst_cinfo: jpeg_compress_struct = std::mem::zeroed();
    dst_cinfo.common.err = jpeg_std_error(&mut dst_err);
    jpeg_create_compress(&mut dst_cinfo);
    jpeg_stdio_dest(&mut dst_cinfo, dst_fp);

    jpeg_copy_critical_parameters(&src_cinfo, &mut dst_cinfo);
    jpeg_write_coefficients(&mut dst_cinfo, orig_coef_arrays);
    // Copy APP/COM markers after the compressor has started.
    jcopy_markers_execute(&mut src_cinfo, &mut dst_cinfo, JCOPY_OPTION_JCOPYOPT_ALL);
    jpeg_finish_compress(&mut dst_cinfo);
    jpeg_destroy_compress(&mut dst_cinfo);

    jpeg_finish_decompress(&mut src_cinfo);
    jpeg_destroy_decompress(&mut src_cinfo);

    libc::fclose(src_fp);
    libc::fclose(dst_fp);

    Ok(())
}
