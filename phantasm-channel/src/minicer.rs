//! MINICER coefficient stabilization + ROAST overflow alleviation.
//!
//! The entry point is [`stabilize_component`]. It walks every AC position
//! of one JPEG component, simulates the channel's re-encode on each
//! containing block, and either (a) confirms the position is naturally
//! robust, (b) perturbs the source coefficient until parity survives, or
//! (c) marks the position wet so the wet-paper STC coder routes around
//! it.
//!
//! Per-position state machine:
//!
//! ```text
//!     ┌───────────────────────────────────────┐
//!     │ check parity of reencode(block)[p]    │
//!     └───────────┬──────────────┬────────────┘
//!                 │ same         │ differs
//!                 ▼              ▼
//!         ┌───────────┐   ┌──────────────────┐
//!         │ stable ✓  │   │ try ±k for k=1.. │
//!         └───────────┘   └────┬─────────────┘
//!                              │ found  │ overflow │ k>MAX
//!                              ▼        ▼          ▼
//!                       stabilized   wet (ROAST)  wet (MINICER)
//! ```
//!
//! "Same parity" means `c.rem_euclid(2) == reencode_c.rem_euclid(2)`
//! (always non-negative even/odd, regardless of sign of `c`).
//!
//! ## Why parity?
//!
//! LSB-replacement is the simplest possible embedding map: bit b is
//! encoded by setting `c.rem_euclid(2) == b`. STC and most modern
//! schemes still use LSB at the bit level — the cleverness lies in
//! *which* coefficients the encoder chooses to flip, not the embedding
//! map itself. Stabilizing parity is therefore the right target for
//! a Phase-2 MVP regardless of the cost function used downstream.

use crate::error::ChannelError;
use crate::simulate::reencode_block;
use crate::{
    StabilizationReport, MINICER_MAX_ITERATIONS, ROAST_BLOCK_WET_THRESHOLD,
    STABILIZED_COST_DISCOUNT,
};
use phantasm_cost::CostMap;
use phantasm_image::jpeg::JpegCoefficients;

/// Valid AC coefficient range for baseline 8-bit JPEG. Values outside
/// this window are clamped during quantization and trigger ROAST.
const COEF_MIN: i16 = -1023;
const COEF_MAX: i16 = 1023;

/// Run MINICER + ROAST on one component of a JPEG.
///
/// `quant_tgt` is the channel's destination quantization table (zigzag).
/// The function mutates `cover.components[component_idx].coefficients`
/// (perturbing AC values to make them fixed points of the re-encode at
/// parity granularity) and mutates `cost_map` (setting wet positions to
/// `f64::INFINITY` and discounting stabilized ones).
pub fn stabilize_component(
    cover: &mut JpegCoefficients,
    component_idx: usize,
    quant_tgt: &[u16; 64],
    cost_map: &mut CostMap,
) -> Result<StabilizationReport, ChannelError> {
    if component_idx >= cover.components.len() {
        return Err(ChannelError::ComponentIndexOutOfRange(
            component_idx,
            cover.components.len(),
        ));
    }
    let comp = &cover.components[component_idx];
    let bw = comp.blocks_wide;
    let bh = comp.blocks_high;
    let total_blocks = bw * bh;

    // Index cost-map positions by (br, bc, dp) → entry index.
    let mut cost_index: std::collections::HashMap<(usize, usize, usize), usize> =
        std::collections::HashMap::with_capacity(cost_map.positions.len());
    for (i, &(br, bc, dp)) in cost_map.positions.iter().enumerate() {
        cost_index.insert((br, bc, dp), i);
    }

    // Validate cost map references.
    for &(br, bc, _) in &cost_map.positions {
        if br >= bh || bc >= bw {
            return Err(ChannelError::CostMapPositionOutOfBounds { br, bc, bw, bh });
        }
    }

    let quant_src = comp.quant_table;

    // Per-block accounting for ROAST block sacrifice.
    let mut block_wet_count: Vec<usize> = vec![0; total_blocks];
    let mut wet_positions: Vec<(usize, usize, usize)> = Vec::new();
    let mut stabilized_count: usize = 0;
    let mut overflow_alleviated_count: usize = 0;
    let mut natural_survivors: usize = 0;
    let mut total_examined: usize = 0;

    // Walk blocks in raster order so a block's mutated coefficients are
    // visible to subsequent positions in the same block.
    for br in 0..bh {
        for bc in 0..bw {
            let block_idx = br * bw + bc;

            for dp in 1..64usize {
                // Skip positions not in the cost map (e.g., orchestrator
                // already excluded them).
                let Some(&ci) = cost_index.get(&(br, bc, dp)) else {
                    continue;
                };
                total_examined += 1;

                // Take a snapshot of the block in zigzag order.
                let mut block = read_block(&cover.components[component_idx], br, bc);
                let original_value = block[dp];

                // First check: is the unmodified position already stable?
                let reenc = reencode_block(&block, &quant_src, quant_tgt);
                if same_parity(reenc[dp], original_value) {
                    natural_survivors += 1;
                    stabilized_count += 1;
                    discount_cost(cost_map, ci);
                    continue;
                }

                // Iterate ±k. Try +1, −1, +2, −2, ...
                let mut stabilized: Option<i16> = None;
                let mut overflowed = false;
                'iter: for k in 1..=(MINICER_MAX_ITERATIONS as i32) {
                    for &sign in &[1i32, -1] {
                        let candidate_i32 = original_value as i32 + sign * k;
                        if candidate_i32 < COEF_MIN as i32 || candidate_i32 > COEF_MAX as i32 {
                            overflowed = true;
                            continue;
                        }
                        let candidate = candidate_i32 as i16;
                        block[dp] = candidate;
                        let reenc = reencode_block(&block, &quant_src, quant_tgt);
                        if same_parity(reenc[dp], candidate) {
                            stabilized = Some(candidate);
                            break 'iter;
                        }
                    }
                }
                // Restore the snapshot to original_value before we either
                // commit a stabilized perturbation or mark wet.
                block[dp] = original_value;

                match stabilized {
                    Some(new_val) => {
                        write_coef(&mut cover.components[component_idx], br, bc, dp, new_val);
                        stabilized_count += 1;
                        discount_cost(cost_map, ci);
                    }
                    None => {
                        // Wet: ROAST if all candidates would overflow,
                        // else MINICER (iteration cap).
                        wet_positions.push((br, bc, dp));
                        block_wet_count[block_idx] += 1;
                        if overflowed && stabilized.is_none() {
                            // Heuristic: if every k overflowed at least
                            // once it's a ROAST overflow case; otherwise
                            // it's a plain MINICER iteration timeout.
                            // Counting only wholly-overflowed positions
                            // would understate the rate, so we count
                            // any overflow contribution.
                            overflow_alleviated_count += 1;
                        }
                        cost_map.costs_plus[ci] = f64::INFINITY;
                        cost_map.costs_minus[ci] = f64::INFINITY;
                    }
                }
            }
        }
    }

    // ROAST block sacrifice: any block over the threshold loses *all*
    // its remaining positions.
    let mut sacrificed_blocks: usize = 0;
    let mut blocks_to_kill: Vec<usize> = Vec::new();
    for (block_idx, &count) in block_wet_count.iter().enumerate() {
        if count > ROAST_BLOCK_WET_THRESHOLD {
            blocks_to_kill.push(block_idx);
        }
    }
    for block_idx in &blocks_to_kill {
        sacrificed_blocks += 1;
        let br = block_idx / bw;
        let bc = block_idx % bw;
        for dp in 1..64usize {
            if let Some(&ci) = cost_index.get(&(br, bc, dp)) {
                if cost_map.costs_plus[ci].is_finite() {
                    cost_map.costs_plus[ci] = f64::INFINITY;
                    cost_map.costs_minus[ci] = f64::INFINITY;
                    // Roll back the stabilized accounting if we previously
                    // counted this position as stabilized.
                    stabilized_count = stabilized_count.saturating_sub(1);
                    wet_positions.push((br, bc, dp));
                }
            }
        }
    }

    let survival_rate_estimate = if total_examined > 0 {
        natural_survivors as f64 / total_examined as f64
    } else {
        0.0
    };

    Ok(StabilizationReport {
        wet_positions,
        stabilized_count,
        overflow_alleviated_count,
        sacrificed_blocks,
        survival_rate_estimate,
    })
}

fn read_block(comp: &phantasm_image::jpeg::JpegComponent, br: usize, bc: usize) -> [i16; 64] {
    let base = (br * comp.blocks_wide + bc) * 64;
    let mut out = [0i16; 64];
    out.copy_from_slice(&comp.coefficients[base..base + 64]);
    out
}

fn write_coef(
    comp: &mut phantasm_image::jpeg::JpegComponent,
    br: usize,
    bc: usize,
    dp: usize,
    value: i16,
) {
    let base = (br * comp.blocks_wide + bc) * 64;
    comp.coefficients[base + dp] = value;
}

#[inline]
fn same_parity(a: i16, b: i16) -> bool {
    (a.rem_euclid(2)) == (b.rem_euclid(2))
}

#[inline]
fn discount_cost(cost_map: &mut CostMap, ci: usize) {
    if cost_map.costs_plus[ci].is_finite() {
        cost_map.costs_plus[ci] *= STABILIZED_COST_DISCOUNT;
    }
    if cost_map.costs_minus[ci].is_finite() {
        cost_map.costs_minus[ci] *= STABILIZED_COST_DISCOUNT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_helper_basic() {
        assert!(same_parity(2, 4));
        assert!(same_parity(-3, 5));
        assert!(!same_parity(2, 3));
        assert!(same_parity(0, 0));
        assert!(!same_parity(0, -1));
    }
}
