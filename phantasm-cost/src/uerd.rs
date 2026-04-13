//! UERD: Uniform Embedding Revisited Distortion.
//!
//! Guo, Ni, Shi. "Uniform Embedding for Efficient JPEG Steganography." IEEE TIFS, 2014.
//! Guo, Ni, Shi. "Using Statistical Image Model for JPEG Steganography: Uniform Embedding
//!     Revisited." IEEE TIFS, 2015.
//!
//! Per-coefficient cost: ρ(b, u, v) = q(u,v) / D_b
//! where D_b = Σ |B(u,v)| · q(u,v) over all AC positions in block b.
//!
//! Both `coefficients` and `quant_table` in `JpegComponent` are zigzag-indexed
//! (inherited from mozjpeg's JBLOCK layout), so `dct_pos` indexes both consistently
//! without any un-zigzag conversion.

use crate::{CostMap, DistortionFunction};
use phantasm_image::jpeg::JpegCoefficients;

const EPSILON: f64 = 0.01;

pub struct Uerd;

impl Uerd {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Uerd {
    fn default() -> Self {
        Self::new()
    }
}

impl DistortionFunction for Uerd {
    fn compute(&self, jpeg: &JpegCoefficients, component_idx: usize) -> CostMap {
        let component = &jpeg.components[component_idx];
        let num_blocks = component.blocks_high * component.blocks_wide;

        // Step 1: compute D_b for each block.
        let mut d_b = vec![0.0f64; num_blocks];
        for br in 0..component.blocks_high {
            for bc in 0..component.blocks_wide {
                let block_idx = br * component.blocks_wide + bc;
                let base = block_idx * 64;
                // Sum over AC positions (dct_pos 1..64); skip DC at pos 0.
                let mut energy = 0.0f64;
                for dp in 1..64usize {
                    let coeff = component.coefficients[base + dp] as f64;
                    let q = component.quant_table[dp] as f64;
                    energy += coeff.abs() * q;
                }
                d_b[block_idx] = energy;
            }
        }

        // Step 2: D_avg (mean D_b across all blocks).
        let d_avg = if num_blocks > 0 {
            d_b.iter().sum::<f64>() / num_blocks as f64
        } else {
            1.0
        };
        let _ = d_avg; // D_rel not used in the simplified formula below.

        // Step 3 & 4: build CostMap.
        // w(u,v) = q(u,v); ρ = q(u,v) / max(D_b, EPSILON).
        // DC (dct_pos == 0) is excluded entirely.
        let capacity = num_blocks * 63;
        let mut positions = Vec::with_capacity(capacity);
        let mut costs_plus = Vec::with_capacity(capacity);
        let mut costs_minus = Vec::with_capacity(capacity);

        for br in 0..component.blocks_high {
            for bc in 0..component.blocks_wide {
                let block_idx = br * component.blocks_wide + bc;
                let base = block_idx * 64;
                let db = d_b[block_idx];
                let db_safe = if db > 0.0 { db } else { EPSILON };

                for dp in 1..64usize {
                    let coeff = component.coefficients[base + dp];
                    let q = component.quant_table[dp] as f64;
                    let rho = q / db_safe;

                    // Saturation: saturating direction gets infinity (wet-paper).
                    let cp = if coeff == i16::MAX {
                        f64::INFINITY
                    } else {
                        rho
                    };
                    let cm = if coeff == i16::MIN {
                        f64::INFINITY
                    } else {
                        rho
                    };

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
        "uerd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phantasm_image::jpeg::{JpegCoefficients, JpegComponent};

    /// Build a synthetic JpegCoefficients with the given per-block coefficient data.
    /// `block_coeffs` is a slice of 64-element arrays, one per block.
    /// All blocks share the same quant_table (standard luminance QF=50, zigzag).
    fn make_jpeg(block_coeffs: &[Vec<i16>]) -> JpegCoefficients {
        let num_blocks = block_coeffs.len();
        // Use a simple flat quant table: q[dp] = dp as u16 + 1 for all positions.
        // This gives non-uniform weights and exercises the UERD formula properly.
        let mut quant_table = [1u16; 64];
        for (i, q) in quant_table.iter_mut().enumerate() {
            *q = (i as u16) + 1; // q[0]=1, q[1]=2, ..., q[63]=64
        }

        let mut coefficients = vec![0i16; num_blocks * 64];
        for (bi, block) in block_coeffs.iter().enumerate() {
            coefficients[bi * 64..(bi + 1) * 64].copy_from_slice(block);
        }

        JpegCoefficients {
            components: vec![JpegComponent {
                id: 1,
                blocks_wide: num_blocks,
                blocks_high: 1,
                coefficients,
                quant_table,
                h_samp_factor: 1,
                v_samp_factor: 1,
            }],
            width: (num_blocks * 8) as u32,
            height: 8,
            quality_estimate: None,
            markers: vec![],
        }
    }

    fn smooth_block() -> Vec<i16> {
        // DC=100, all AC=0 → D_b = 0 → cost ≈ q / EPSILON (very high)
        let mut b = vec![0i16; 64];
        b[0] = 100;
        b
    }

    fn textured_block() -> Vec<i16> {
        // DC=100, AC positions filled with large values → D_b large → cost low
        let mut b = vec![50i16; 64];
        b[0] = 100; // DC
        b
    }

    #[test]
    fn smooth_block_has_higher_costs_than_textured() {
        let jpeg = make_jpeg(&[smooth_block(), textured_block()]);
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);

        // Smooth block is block 0 (bc=0), textured is block 1 (bc=1).
        // Find cost at same dct_pos in each block and compare.
        let mut smooth_cost = None;
        let mut textured_cost = None;
        for (i, &(br, bc, dp)) in cost_map.positions.iter().enumerate() {
            if br == 0 && bc == 0 && dp == 1 {
                smooth_cost = Some(cost_map.costs_plus[i]);
            }
            if br == 0 && bc == 1 && dp == 1 {
                textured_cost = Some(cost_map.costs_plus[i]);
            }
        }
        let sc = smooth_cost.expect("smooth block cost not found");
        let tc = textured_cost.expect("textured block cost not found");
        assert!(
            sc > tc,
            "smooth cost ({sc}) should be > textured cost ({tc})"
        );
    }

    #[test]
    fn zero_coeff_in_smooth_block_has_very_high_cost() {
        let jpeg = make_jpeg(&[smooth_block(), textured_block()]);
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);

        let mut smooth_cost = None;
        let mut textured_cost = None;
        for (i, &(br, bc, dp)) in cost_map.positions.iter().enumerate() {
            if br == 0 && bc == 0 && dp == 5 {
                smooth_cost = Some(cost_map.costs_plus[i]);
            }
            if br == 0 && bc == 1 && dp == 5 {
                textured_cost = Some(cost_map.costs_plus[i]);
            }
        }
        let sc = smooth_cost.expect("smooth cost not found");
        let tc = textured_cost.expect("textured cost not found");
        assert!(
            sc >= 100.0 * tc,
            "smooth cost ({sc}) should be >= 100× textured cost ({tc})"
        );
    }

    #[test]
    fn dc_coefficients_excluded_from_positions() {
        let jpeg = make_jpeg(&[smooth_block(), textured_block()]);
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);
        for &(_, _, dp) in &cost_map.positions {
            assert_ne!(dp, 0, "DC coefficient (dct_pos=0) should be excluded");
        }
    }

    #[test]
    fn saturation_wet_paper() {
        let mut block = vec![0i16; 64];
        block[0] = 0; // DC
        block[10] = i16::MAX; // saturated AC coefficient

        // Give the block non-zero energy so D_b > 0 (use another AC pos).
        block[5] = 100;

        let jpeg = make_jpeg(&[block]);
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);

        let mut found_sat = false;
        for (i, &(_, _, dp)) in cost_map.positions.iter().enumerate() {
            if dp == 10 {
                assert_eq!(
                    cost_map.costs_plus[i],
                    f64::INFINITY,
                    "costs_plus at i16::MAX should be infinity"
                );
                assert!(
                    cost_map.costs_minus[i].is_finite(),
                    "costs_minus at i16::MAX should be finite"
                );
                found_sat = true;
            }
        }
        assert!(found_sat, "position with i16::MAX not found in cost map");
    }

    #[test]
    fn cost_map_lengths_match() {
        let jpeg = make_jpeg(&[smooth_block(), textured_block()]);
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);
        assert_eq!(cost_map.costs_plus.len(), cost_map.positions.len());
        assert_eq!(cost_map.costs_minus.len(), cost_map.positions.len());
    }

    #[test]
    fn determinism() {
        let jpeg = make_jpeg(&[smooth_block(), textured_block()]);
        let uerd = Uerd::new();
        let cm1 = uerd.compute(&jpeg, 0);
        let cm2 = uerd.compute(&jpeg, 0);
        assert_eq!(cm1.positions, cm2.positions);
        assert_eq!(cm1.costs_plus, cm2.costs_plus);
        assert_eq!(cm1.costs_minus, cm2.costs_minus);
    }

    #[test]
    fn name_is_uerd() {
        assert_eq!(Uerd::new().name(), "uerd");
    }

    #[test]
    fn real_fixture_smoke_test() {
        use image::{ImageBuffer, Rgb};
        use std::path::PathBuf;

        let tmp_dir = tempfile::tempdir().expect("tempdir");
        let jpeg_path: PathBuf = tmp_dir.path().join("test.jpg");

        // Generate a 128×128 RGB plasma-like image (checkerboard pattern for texture).
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(128, 128, |x, y| {
            let r = ((x * 2 + y) % 256) as u8;
            let g = ((x + y * 3) % 256) as u8;
            let b = ((x * y + 13) % 256) as u8;
            Rgb([r, g, b])
        });
        img.save(&jpeg_path).expect("save JPEG");

        let jpeg = phantasm_image::jpeg::read(&jpeg_path).expect("read JPEG");
        let uerd = Uerd::new();
        let cost_map = uerd.compute(&jpeg, 0);

        assert!(!cost_map.is_empty(), "cost map should be non-empty");

        let finite_positive_count = cost_map
            .costs_plus
            .iter()
            .filter(|&&c| c.is_finite() && c > 0.0)
            .count();
        let ratio = finite_positive_count as f64 / cost_map.len() as f64;
        assert!(
            ratio >= 0.5,
            "at least 50% of costs should be finite positive, got {:.1}%",
            ratio * 100.0
        );

        let finite_costs: Vec<f64> = cost_map
            .costs_plus
            .iter()
            .copied()
            .filter(|c| c.is_finite())
            .collect();
        let mean = finite_costs.iter().sum::<f64>() / finite_costs.len() as f64;
        assert!(
            (0.01..=1000.0).contains(&mean),
            "mean cost {mean} out of expected range [0.01, 1000]"
        );
    }
}
