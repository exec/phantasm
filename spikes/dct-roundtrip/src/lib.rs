use libc::FILE;
use mozjpeg_sys::*;
use std::ffi::CString;
use std::path::Path;

/// A snapshot of all DCT coefficients from a JPEG file, organized by component.
/// coeffs[comp][row][block_col][coef_idx]
pub struct CoeffSnapshot {
    pub num_components: usize,
    /// coeffs[comp_idx] = flat Vec of JCOEF (row-major over blocks, then 64 coeffs per block)
    pub coeffs: Vec<Vec<i16>>,
    pub width_in_blocks: Vec<u32>,
    pub height_in_blocks: Vec<u32>,
}

impl CoeffSnapshot {
    pub fn coeff(&self, comp: usize, block_row: u32, block_col: u32, coef_idx: usize) -> i16 {
        let w = self.width_in_blocks[comp] as usize;
        let idx = (block_row as usize) * w * 64 + (block_col as usize) * 64 + coef_idx;
        self.coeffs[comp][idx]
    }

    pub fn set_coeff(&mut self, comp: usize, block_row: u32, block_col: u32, coef_idx: usize, val: i16) {
        let w = self.width_in_blocks[comp] as usize;
        let idx = (block_row as usize) * w * 64 + (block_col as usize) * 64 + coef_idx;
        self.coeffs[comp][idx] = val;
    }

    pub fn total_coeffs(&self) -> usize {
        self.coeffs.iter().map(|c| c.len()).sum()
    }
}

/// Opens a JPEG file for reading, returning a FILE pointer.
/// Caller must close it.
unsafe fn fopen_read(path: &Path) -> *mut FILE {
    let s = CString::new(path.to_str().expect("non-utf8 path")).unwrap();
    let mode = CString::new("rb").unwrap();
    libc::fopen(s.as_ptr(), mode.as_ptr())
}

unsafe fn fopen_write(path: &Path) -> *mut FILE {
    let s = CString::new(path.to_str().expect("non-utf8 path")).unwrap();
    let mode = CString::new("wb").unwrap();
    libc::fopen(s.as_ptr(), mode.as_ptr())
}

/// Reads all DCT coefficients from a JPEG file into a CoeffSnapshot.
pub fn read_coefficients(path: &Path) -> Result<CoeffSnapshot, String> {
    unsafe {
        let fp = fopen_read(path);
        if fp.is_null() {
            return Err(format!("cannot open {}", path.display()));
        }

        let mut err: jpeg_error_mgr = std::mem::zeroed();
        let mut cinfo: jpeg_decompress_struct = std::mem::zeroed();
        cinfo.common.err = jpeg_std_error(&mut err);

        jpeg_create_decompress(&mut cinfo);
        jpeg_stdio_src(&mut cinfo, fp);
        jpeg_read_header(&mut cinfo, true as _);

        // Read coefficients without decompressing to pixels
        let coef_arrays = jpeg_read_coefficients(&mut cinfo);

        let num_components = cinfo.num_components as usize;
        let comp_info_slice = std::slice::from_raw_parts(cinfo.comp_info, num_components);

        let mut snapshot = CoeffSnapshot {
            num_components,
            coeffs: Vec::with_capacity(num_components),
            width_in_blocks: Vec::with_capacity(num_components),
            height_in_blocks: Vec::with_capacity(num_components),
        };

        for (ci, comp) in comp_info_slice.iter().enumerate() {
            let w = comp.width_in_blocks;
            let h = comp.height_in_blocks;
            snapshot.width_in_blocks.push(w);
            snapshot.height_in_blocks.push(h);

            let total = (w as usize) * (h as usize) * 64;
            let mut comp_coeffs = vec![0i16; total];

            // Access each row of blocks via the virtual array manager
            for row in 0..h {
                let block_row_ptr = ((*cinfo.common.mem).access_virt_barray.unwrap())(
                    &mut cinfo.common,
                    *coef_arrays.add(ci),
                    row,
                    1,
                    false as _,
                );
                // block_row_ptr is JBLOCKARRAY = *mut JBLOCKROW = *mut *mut [i16; 64]
                // It points to one row of blocks
                let row_ptr: *mut JBLOCK = *block_row_ptr;
                for col in 0..w as usize {
                    let block: &JBLOCK = &*row_ptr.add(col);
                    let base = row as usize * w as usize * 64 + col * 64;
                    comp_coeffs[base..base + 64].copy_from_slice(block);
                }
            }

            snapshot.coeffs.push(comp_coeffs);
        }

        jpeg_finish_decompress(&mut cinfo);
        jpeg_destroy_decompress(&mut cinfo);
        libc::fclose(fp);

        Ok(snapshot)
    }
}

/// Writes a CoeffSnapshot into output_path using input_path's header/tables as template.
/// The coefficient values in `snapshot` are written as-is.
pub fn write_coefficients(input_path: &Path, output_path: &Path, snapshot: &CoeffSnapshot) -> Result<(), String> {
    unsafe {
        // Open source for reading header/quant tables
        let src_fp = fopen_read(input_path);
        if src_fp.is_null() {
            return Err(format!("cannot open source {}", input_path.display()));
        }
        let dst_fp = fopen_write(output_path);
        if dst_fp.is_null() {
            libc::fclose(src_fp);
            return Err(format!("cannot open dest {}", output_path.display()));
        }

        let mut src_err: jpeg_error_mgr = std::mem::zeroed();
        let mut src_cinfo: jpeg_decompress_struct = std::mem::zeroed();
        src_cinfo.common.err = jpeg_std_error(&mut src_err);
        jpeg_create_decompress(&mut src_cinfo);
        jpeg_stdio_src(&mut src_cinfo, src_fp);
        jpeg_read_header(&mut src_cinfo, true as _);
        // Read original coefficients so we have a valid coef_arrays to pass
        let orig_coef_arrays = jpeg_read_coefficients(&mut src_cinfo);

        let num_components = src_cinfo.num_components as usize;
        let comp_info_slice = std::slice::from_raw_parts(src_cinfo.comp_info, num_components);

        // Overwrite the virtual arrays with our snapshot's coefficients
        for (ci, comp) in comp_info_slice.iter().enumerate() {
            let w = comp.width_in_blocks;
            let h = comp.height_in_blocks;

            for row in 0..h {
                let block_row_ptr = ((*src_cinfo.common.mem).access_virt_barray.unwrap())(
                    &mut src_cinfo.common,
                    *orig_coef_arrays.add(ci),
                    row,
                    1,
                    true as _, // writable
                );
                let row_ptr: *mut JBLOCK = *block_row_ptr;
                for col in 0..w as usize {
                    let block: &mut JBLOCK = &mut *row_ptr.add(col);
                    let base = row as usize * w as usize * 64 + col * 64;
                    block.copy_from_slice(&snapshot.coeffs[ci][base..base + 64]);
                }
            }
        }

        // Set up compressor and write
        let mut dst_err: jpeg_error_mgr = std::mem::zeroed();
        let mut dst_cinfo: jpeg_compress_struct = std::mem::zeroed();
        dst_cinfo.common.err = jpeg_std_error(&mut dst_err);
        jpeg_create_compress(&mut dst_cinfo);
        jpeg_stdio_dest(&mut dst_cinfo, dst_fp);

        // Copy all compression params from source
        jpeg_copy_critical_parameters(&src_cinfo, &mut dst_cinfo);

        jpeg_write_coefficients(&mut dst_cinfo, orig_coef_arrays);

        jpeg_finish_compress(&mut dst_cinfo);
        jpeg_destroy_compress(&mut dst_cinfo);

        jpeg_finish_decompress(&mut src_cinfo);
        jpeg_destroy_decompress(&mut src_cinfo);

        libc::fclose(src_fp);
        libc::fclose(dst_fp);

        Ok(())
    }
}

/// Compares two CoeffSnapshots for bit-exact equality.
/// Returns Ok(total_coeffs) on match, Err with first mismatch info on failure.
pub fn compare_snapshots(a: &CoeffSnapshot, b: &CoeffSnapshot) -> Result<usize, String> {
    if a.num_components != b.num_components {
        return Err(format!(
            "component count mismatch: {} vs {}",
            a.num_components, b.num_components
        ));
    }
    let mut total = 0usize;
    for ci in 0..a.num_components {
        if a.coeffs[ci].len() != b.coeffs[ci].len() {
            return Err(format!(
                "component {} length mismatch: {} vs {}",
                ci,
                a.coeffs[ci].len(),
                b.coeffs[ci].len()
            ));
        }
        for (idx, (va, vb)) in a.coeffs[ci].iter().zip(b.coeffs[ci].iter()).enumerate() {
            if va != vb {
                let w = a.width_in_blocks[ci] as usize;
                let block_idx = idx / 64;
                let coef_idx = idx % 64;
                let block_row = block_idx / w;
                let block_col = block_idx % w;
                return Err(format!(
                    "mismatch at comp={} block_row={} block_col={} coef={}: {} vs {}",
                    ci, block_row, block_col, coef_idx, va, vb
                ));
            }
            total += 1;
        }
    }
    Ok(total)
}

/// Performs a full round-trip: read coefficients, write them to output (optionally with a
/// modification), re-read, and compare.
pub struct RoundTripResult {
    pub total_coeffs: usize,
    pub modified_comp: Option<usize>,
    pub modified_block_row: Option<u32>,
    pub modified_block_col: Option<u32>,
    pub modified_coef_idx: Option<usize>,
    pub original_value: Option<i16>,
    pub new_value: Option<i16>,
    pub verified_value: Option<i16>,
}

pub fn round_trip(
    input_path: &Path,
    output_path: &Path,
    modify: bool,
) -> Result<RoundTripResult, String> {
    let mut snapshot = read_coefficients(input_path)?;

    let (mod_comp, mod_row, mod_col, mod_coef, orig_val, new_val) = if modify {
        // Find a safe AC coefficient in position (3,3) of the luminance component (comp 0)
        // DCT position (row=3, col=3) maps to index 3*8+3 = 27
        let ci = 0;
        let br = 0u32;
        let bc = 0u32;
        let coef_idx = 27usize; // position (3,3) in 8x8 block, a mid-frequency AC coef
        let orig = snapshot.coeff(ci, br, bc, coef_idx);

        // Choose delta: add +1 unless orig is i16::MAX (won't happen in practice), else -1
        let delta: i16 = if orig < i16::MAX { 1 } else { -1 };
        let new = orig + delta;
        snapshot.set_coeff(ci, br, bc, coef_idx, new);

        (Some(ci), Some(br), Some(bc), Some(coef_idx), Some(orig), Some(new))
    } else {
        (None, None, None, None, None, None)
    };

    write_coefficients(input_path, output_path, &snapshot)?;

    // Re-read and verify
    let readback = read_coefficients(output_path)?;

    let total = compare_snapshots(&snapshot, &readback).map_err(|e| {
        format!("round-trip verification failed: {}", e)
    })?;

    let verified = mod_coef.map(|ci_coef| {
        readback.coeff(
            mod_comp.unwrap(),
            mod_row.unwrap(),
            mod_col.unwrap(),
            ci_coef,
        )
    });

    Ok(RoundTripResult {
        total_coeffs: total,
        modified_comp: mod_comp,
        modified_block_row: mod_row,
        modified_block_col: mod_col,
        modified_coef_idx: mod_coef,
        original_value: orig_val,
        new_value: new_val,
        verified_value: verified,
    })
}
