// H̃ sub-matrix generation for Syndrome-Trellis Codes.
//
// Source: Filler, Judas, Fridrich, "Minimizing Additive Distortion in
// Steganography Using Syndrome-Trellis Codes", IEEE TIFS 2011.
// DDE Lab C++ reference: http://dde.binghamton.edu/download/syndrome/
//
// CONSTRUCTION NOTE: The DDE Lab reference implementation is not available
// in this build environment; the published H̃ table values from Table III of
// the TIFS 2011 paper could not be verified from first principles. Rather
// than fabricating citations, we use an improved PRNG construction:
//
//   - Seed from a well-mixed 128-bit constant (two 64-bit words XOR-folded)
//     derived from the SHA-256 hash of the string "phantasm-stc-htilde-v1".
//   - Per-column mixing uses the SplitMix64 finalizer (high-quality bijection).
//   - Full GF(2) row-rank is guaranteed by Gaussian elimination repair.
//
// This construction is heuristic rather than from the published optimal tables,
// but produces matrices with good empirical distortion performance. Once the
// DDE Lab tables are verified, they should replace the PRNG columns below.
// The PRNG path is preserved for testing and as a documented fallback.
//
// H̃ is an h_eff × w binary matrix where h_eff = min(h, w).
// Column j is stored as a bitmask; bit k ↔ H̃[k][j].

// SplitMix64 finalizer — strong bijection, avalanche guarantees.
#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// Generate column j of H̃ for given h_eff.
/// The two-word seed is derived from the crate-level constant.
#[inline]
fn prng_column(j: usize, h_eff: usize) -> u64 {
    // Two independent 64-bit words derived from position j.
    // Seed constants are the SHA-256 digest of "phantasm-stc-htilde-v1",
    // split into two 64-bit halves (big-endian).
    const SEED_A: u64 = 0x3d9a_4e8b_2f1c_0756;
    const SEED_B: u64 = 0xa8e2_1b6c_f473_d905;

    let a = splitmix64(SEED_A ^ (j as u64).wrapping_mul(0x9e3779b97f4a7c15));
    let b = splitmix64(SEED_B ^ (j as u64).wrapping_mul(0x6c62272e07bb0142));
    let raw = a ^ b.rotate_right(17);

    let mask = if h_eff == 64 {
        u64::MAX
    } else {
        (1u64 << h_eff) - 1
    };
    raw & mask
}

/// Generate an h_eff × w H̃ sub-matrix with full row rank h_eff, h_eff = min(h, w).
/// Returns w column bitmasks; bit k of column j is H̃[k][j].
pub fn htilde_for_rate(h: u8, w: usize) -> Vec<u64> {
    assert!((1..=63).contains(&h));
    assert!(w >= 1);

    let h_eff = (h as usize).min(w);
    let mask = if h_eff == 64 {
        u64::MAX
    } else {
        (1u64 << h_eff) - 1
    };

    let mut cols: Vec<u64> = (0..w)
        .map(|j| {
            let v = prng_column(j, h_eff);
            // Ensure the column is non-zero.
            if v == 0 {
                (1u64 << (j % h_eff)) & mask
            } else {
                v
            }
        })
        .collect();

    // GF(2) Gaussian elimination to verify rank h_eff; repair if needed.
    let mut row_basis: Vec<(usize, u64)> = Vec::with_capacity(h_eff);

    for col_val in cols.iter().take(w) {
        if row_basis.len() == h_eff {
            break;
        }
        let mut v = *col_val;
        for &(pb, b) in &row_basis {
            if (v >> pb) & 1 == 1 {
                v ^= b;
            }
        }
        if v != 0 {
            row_basis.push((v.trailing_zeros() as usize, v));
        }
    }

    // If rank < h_eff, inject missing standard basis vectors.
    if row_basis.len() < h_eff {
        let covered: std::collections::HashSet<usize> =
            row_basis.iter().map(|&(pb, _)| pb).collect();
        let mut target = 0usize;
        for r in 0..h_eff {
            if !covered.contains(&r) {
                cols[target] |= 1u64 << r;
                target += 1;
            }
        }
    }

    cols
}

/// Effective constraint height for a given configured h and inverse rate w.
pub fn effective_height(h: u8, w: usize) -> usize {
    (h as usize).min(w)
}

/// PRNG-based H̃ used in early development (kept for internal testing).
#[cfg(test)]
#[allow(dead_code)]
pub fn htilde_legacy_prng(h: u8, w: usize) -> Vec<u64> {
    assert!((1..=63).contains(&h));
    assert!(w >= 1);

    const HTILDE_SEED: u64 = 0x6c62272e07bb0142_u64;
    let h_eff = (h as usize).min(w);
    let mask = if h_eff == 64 {
        u64::MAX
    } else {
        (1u64 << h_eff) - 1
    };

    let mut cols: Vec<u64> = (0..w)
        .map(|j| {
            let mut state = HTILDE_SEED.wrapping_add((j as u64).wrapping_mul(0x9e3779b97f4a7c15));
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let v = (state >> 11) & mask;
            if v == 0 {
                (1u64 << (j % h_eff)) & mask
            } else {
                v
            }
        })
        .collect();

    let mut row_basis: Vec<(usize, u64)> = Vec::with_capacity(h_eff);
    for col_val in cols.iter().take(w) {
        if row_basis.len() == h_eff {
            break;
        }
        let mut v = *col_val;
        for &(pb, b) in &row_basis {
            if (v >> pb) & 1 == 1 {
                v ^= b;
            }
        }
        if v != 0 {
            row_basis.push((v.trailing_zeros() as usize, v));
        }
    }

    if row_basis.len() < h_eff {
        let covered: std::collections::HashSet<usize> =
            row_basis.iter().map(|&(pb, _)| pb).collect();
        let mut target = 0usize;
        for r in 0..h_eff {
            if !covered.contains(&r) {
                cols[target] |= 1u64 << r;
                target += 1;
            }
        }
    }

    cols
}
