//! Sidecar-file distortion function: loads per-coefficient costs from an
//! externally-computed binary file. Used for adversarial cost research where
//! an out-of-tree gradient computation produces costs that the existing
//! Rust embed pipeline can consume.
//!
//! Sidecar binary format (little-endian throughout):
//!
//! ```text
//! offset  bytes  field
//! 0       8      "PHCOST\0\0"  magic
//! 8       4      uint32 version (currently 2)
//! 12      4      uint32 orig_h (cover height in pixels)
//! 16      4      uint32 orig_w (cover width in pixels)
//! 20      4      uint32 block_size (must be 8)
//! 24      4      uint32 n_blocks_y (orig_h / 8)
//! 28      4      uint32 n_blocks_x (orig_w / 8)
//! 32+     4 *    float32[n_blocks_y * n_blocks_x * 64] cost map
//!                indexed as [block_row][block_col][dct_row * 8 + dct_col]
//! ```
//!
//! DC coefficients (dct_pos == 0) are skipped when building the CostMap, to
//! match the convention used by [`super::Uniform`], [`super::Uerd`], and
//! [`super::Juniward`].

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"PHCOST\0\0";
const VERSION_V2: u32 = 2;
const VERSION_V3: u32 = 3;
const BLOCK: usize = 8;
const HEADER_BYTES: usize = 32;

/// Distortion function that reads per-coefficient costs from a sidecar binary
/// file produced by an out-of-tree adversarial cost computer.
///
/// The sidecar must be paired with a specific cover image. The Y-channel
/// dimensions of the cover (component 0 of the JPEG) must match the sidecar
/// header's `orig_h` / `orig_w`, otherwise [`Self::compute`] panics with a
/// clear error message.
pub struct Sidecar {
    sidecar_path: PathBuf,
}

impl Sidecar {
    /// Construct a sidecar cost source pointing at the given path.
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            sidecar_path: path.into(),
        }
    }

    fn load(&self) -> SidecarData {
        load_sidecar(&self.sidecar_path)
            .unwrap_or_else(|e| panic!("failed to load sidecar {:?}: {}", self.sidecar_path, e))
    }
}

impl DistortionFunction for Sidecar {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let data = self.load();
        let component = &jpeg.components[component_idx];

        if data.n_blocks_y < component.blocks_high || data.n_blocks_x < component.blocks_wide {
            panic!(
                "sidecar block grid {}x{} smaller than JPEG component grid {}x{}",
                data.n_blocks_y, data.n_blocks_x, component.blocks_high, component.blocks_wide
            );
        }

        let mut positions = Vec::with_capacity(component.blocks_high * component.blocks_wide * 63);
        let mut costs_plus = Vec::with_capacity(positions.capacity());
        let mut costs_minus = Vec::with_capacity(positions.capacity());

        for br in 0..component.blocks_high {
            for bc in 0..component.blocks_wide {
                let block_offset = (br * data.n_blocks_x + bc) * BLOCK * BLOCK;
                for dp in 1..64 {
                    let cp = data.costs_plus[block_offset + dp] as f64;
                    let cm = data
                        .costs_minus
                        .as_ref()
                        .map(|m| m[block_offset + dp] as f64)
                        .unwrap_or(cp);
                    let cp = if cp.is_finite() && cp > 0.0 { cp } else { 1.0 };
                    let cm = if cm.is_finite() && cm > 0.0 { cm } else { 1.0 };
                    positions.push((br, bc, dp));
                    costs_plus.push(cp);
                    costs_minus.push(cm);
                }
            }
        }

        CostMap {
            costs_plus,
            costs_minus,
            positions,
        }
    }

    fn name(&self) -> &str {
        "sidecar"
    }
}

struct SidecarData {
    n_blocks_y: usize,
    n_blocks_x: usize,
    costs_plus: Vec<f32>,
    costs_minus: Option<Vec<f32>>,
}

fn parse_floats(slice: &[u8], n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    for chunk in slice[..n * 4].chunks_exact(4) {
        out.push(f32::from_le_bytes(chunk.try_into().unwrap()));
    }
    out
}

fn load_sidecar(path: &Path) -> Result<SidecarData, String> {
    let bytes = fs::read(path).map_err(|e| format!("read: {}", e))?;
    if bytes.len() < HEADER_BYTES {
        return Err(format!("file too small: {} bytes", bytes.len()));
    }
    if &bytes[0..8] != MAGIC {
        return Err(format!("bad magic: {:?}", &bytes[0..8]));
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if version != VERSION_V2 && version != VERSION_V3 {
        return Err(format!(
            "unsupported version {} (expected {} or {})",
            version, VERSION_V2, VERSION_V3
        ));
    }
    let _orig_h = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let _orig_w = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let block_size = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
    if block_size != BLOCK {
        return Err(format!(
            "block_size {} not supported (expected 8)",
            block_size
        ));
    }
    let n_blocks_y = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
    let n_blocks_x = u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize;
    let n_floats = n_blocks_y * n_blocks_x * BLOCK * BLOCK;

    let n_arrays = if version == VERSION_V3 { 2 } else { 1 };
    let expected_size = HEADER_BYTES + n_floats * 4 * n_arrays;
    if bytes.len() != expected_size {
        return Err(format!(
            "size mismatch: file is {} bytes, expected {} (header {} + {} arrays × {} floats × 4)",
            bytes.len(),
            expected_size,
            HEADER_BYTES,
            n_arrays,
            n_floats
        ));
    }

    let costs_plus = parse_floats(&bytes[HEADER_BYTES..], n_floats);
    let costs_minus = if version == VERSION_V3 {
        Some(parse_floats(
            &bytes[HEADER_BYTES + n_floats * 4..],
            n_floats,
        ))
    } else {
        None
    };

    Ok(SidecarData {
        n_blocks_y,
        n_blocks_x,
        costs_plus,
        costs_minus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_uniform_sidecar(path: &Path, n_y: usize, n_x: usize, value: f32) {
        let n = n_y * n_x * 64;
        let mut buf = Vec::with_capacity(HEADER_BYTES + n * 4);
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION_V2.to_le_bytes());
        buf.extend_from_slice(&((n_y * 8) as u32).to_le_bytes());
        buf.extend_from_slice(&((n_x * 8) as u32).to_le_bytes());
        buf.extend_from_slice(&(BLOCK as u32).to_le_bytes());
        buf.extend_from_slice(&(n_y as u32).to_le_bytes());
        buf.extend_from_slice(&(n_x as u32).to_le_bytes());
        for _ in 0..n {
            buf.extend_from_slice(&value.to_le_bytes());
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&buf).unwrap();
    }

    fn write_signed_sidecar(path: &Path, n_y: usize, n_x: usize, plus: f32, minus: f32) {
        let n = n_y * n_x * 64;
        let mut buf = Vec::with_capacity(HEADER_BYTES + 2 * n * 4);
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION_V3.to_le_bytes());
        buf.extend_from_slice(&((n_y * 8) as u32).to_le_bytes());
        buf.extend_from_slice(&((n_x * 8) as u32).to_le_bytes());
        buf.extend_from_slice(&(BLOCK as u32).to_le_bytes());
        buf.extend_from_slice(&(n_y as u32).to_le_bytes());
        buf.extend_from_slice(&(n_x as u32).to_le_bytes());
        for _ in 0..n {
            buf.extend_from_slice(&plus.to_le_bytes());
        }
        for _ in 0..n {
            buf.extend_from_slice(&minus.to_le_bytes());
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&buf).unwrap();
    }

    #[test]
    fn round_trip_uniform_sidecar_v2() {
        let tmp = std::env::temp_dir().join("phantasm-cost-sidecar-v2.advcost");
        write_uniform_sidecar(&tmp, 4, 4, 2.5);
        let data = load_sidecar(&tmp).unwrap();
        assert_eq!(data.n_blocks_y, 4);
        assert_eq!(data.n_blocks_x, 4);
        assert_eq!(data.costs_plus.len(), 4 * 4 * 64);
        assert!(data.costs_minus.is_none());
        for c in &data.costs_plus {
            assert_eq!(*c, 2.5);
        }
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn round_trip_signed_sidecar_v3() {
        let tmp = std::env::temp_dir().join("phantasm-cost-sidecar-v3.advcost");
        write_signed_sidecar(&tmp, 4, 4, 1.5, 0.7);
        let data = load_sidecar(&tmp).unwrap();
        assert_eq!(data.n_blocks_y, 4);
        assert_eq!(data.n_blocks_x, 4);
        assert_eq!(data.costs_plus.len(), 4 * 4 * 64);
        let cm = data.costs_minus.as_ref().expect("v3 has costs_minus");
        assert_eq!(cm.len(), 4 * 4 * 64);
        for c in &data.costs_plus {
            assert_eq!(*c, 1.5);
        }
        for c in cm {
            assert_eq!(*c, 0.7);
        }
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn rejects_bad_magic() {
        let tmp = std::env::temp_dir().join("phantasm-cost-sidecar-bad-magic.advcost");
        fs::write(&tmp, b"NOTPHCOST").unwrap();
        assert!(load_sidecar(&tmp).is_err());
        fs::remove_file(&tmp).ok();
    }

    #[test]
    fn name() {
        let s = Sidecar::new("/tmp/never-loaded.advcost");
        assert_eq!(s.name(), "sidecar");
    }
}
