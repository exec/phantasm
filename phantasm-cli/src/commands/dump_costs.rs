//! Hidden research command: dump a per-coefficient cost map for a JPEG cover
//! to a sidecar binary file (PHCOST v3 format). Used by the out-of-tree
//! adversarial-cost workflow to load J-UNIWARD baseline costs into Python so
//! they can be combined with detector-gradient adversarial signals.
//!
//! Sidecar format matches the version 3 layout consumed by [`phantasm_cost::Sidecar`]:
//! both `costs_plus` and `costs_minus` arrays are written (here equal, since
//! the existing distortion functions are symmetric in the modification direction).

use anyhow::{Context, Result};
use phantasm_cost::{DistortionFunction, Juniward, Uerd, Uniform};
use phantasm_image::jpeg;
use std::path::Path;

use crate::CostFunctionChoice;

const MAGIC: &[u8; 8] = b"PHCOST\0\0";
const VERSION: u32 = 3;
const BLOCK: usize = 8;

pub fn run(input: &Path, output: &Path, cost_function: CostFunctionChoice) -> Result<()> {
    let jpeg = jpeg::read(input).with_context(|| format!("failed to read {}", input.display()))?;

    let distortion: Box<dyn DistortionFunction> = match cost_function {
        CostFunctionChoice::Uniform => Box::new(Uniform),
        CostFunctionChoice::Uerd => Box::new(Uerd),
        CostFunctionChoice::Juniward => Box::new(Juniward),
        CostFunctionChoice::FromSidecar => {
            anyhow::bail!("dump-costs cannot dump from-sidecar (no underlying cost function)")
        }
    };

    let cost_map = distortion.compute(&jpeg, 0);
    let component = &jpeg.components[0];
    let n_blocks_y = component.blocks_high;
    let n_blocks_x = component.blocks_wide;
    let n_floats = n_blocks_y * n_blocks_x * BLOCK * BLOCK;

    // Build a dense (n_blocks_y, n_blocks_x, 8, 8) array. Default cost = 1.0
    // (uniform) at DC and any unmapped position; per-coefficient costs come
    // from the cost_map's positions list.
    let mut plus = vec![1.0f32; n_floats];
    let mut minus = vec![1.0f32; n_floats];
    for (i, &(br, bc, dp)) in cost_map.positions.iter().enumerate() {
        let idx = (br * n_blocks_x + bc) * BLOCK * BLOCK + dp;
        plus[idx] = cost_map.costs_plus[i] as f32;
        minus[idx] = cost_map.costs_minus[i] as f32;
    }

    let mut buf = Vec::with_capacity(32 + 2 * n_floats * 4);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    let h = (n_blocks_y * BLOCK) as u32;
    let w = (n_blocks_x * BLOCK) as u32;
    buf.extend_from_slice(&h.to_le_bytes());
    buf.extend_from_slice(&w.to_le_bytes());
    buf.extend_from_slice(&(BLOCK as u32).to_le_bytes());
    buf.extend_from_slice(&(n_blocks_y as u32).to_le_bytes());
    buf.extend_from_slice(&(n_blocks_x as u32).to_le_bytes());
    for v in &plus {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    for v in &minus {
        buf.extend_from_slice(&v.to_le_bytes());
    }

    std::fs::write(output, &buf)
        .with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "wrote {} ({} bytes, {} blocks, cost_function={})",
        output.display(),
        buf.len(),
        n_blocks_y * n_blocks_x,
        cost_function
    );
    Ok(())
}
