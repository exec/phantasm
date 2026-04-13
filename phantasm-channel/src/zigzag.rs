//! JPEG zigzag ↔ natural-order conversion tables and helpers.
//!
//! `phantasm-image` stores DCT coefficients and quant tables in **zigzag**
//! order (the order they appear in libjpeg's `JBLOCK`). The DCT/IDCT
//! functions in `phantasm-image::dct` operate on **natural** (row-major)
//! order. This module provides the bridge.

/// `ZIGZAG[zz] = natural` — the natural-order index of the `zz`-th zigzag slot.
#[rustfmt::skip]
pub const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Inverse table: `INV_ZIGZAG[natural] = zz`. Kept available for future
/// callers that need to look up zigzag indices from natural-order.
#[allow(dead_code)]
pub const INV_ZIGZAG: [usize; 64] = {
    let mut inv = [0usize; 64];
    let mut zz = 0;
    while zz < 64 {
        inv[ZIGZAG[zz]] = zz;
        zz += 1;
    }
    inv
};

/// Convert a 64-element zigzag-indexed block to natural row-major order.
pub fn zigzag_to_natural<T: Copy + Default>(zz: &[T; 64]) -> [T; 64] {
    let mut nat = [T::default(); 64];
    for i in 0..64 {
        nat[ZIGZAG[i]] = zz[i];
    }
    nat
}

/// Convert a 64-element natural-order block back to zigzag order.
pub fn natural_to_zigzag<T: Copy + Default>(nat: &[T; 64]) -> [T; 64] {
    let mut zz = [T::default(); 64];
    for i in 0..64 {
        zz[i] = nat[ZIGZAG[i]];
    }
    zz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inv_zigzag_is_actual_inverse() {
        for zz in 0..64 {
            assert_eq!(INV_ZIGZAG[ZIGZAG[zz]], zz);
        }
        for nat in 0..64 {
            assert_eq!(ZIGZAG[INV_ZIGZAG[nat]], nat);
        }
    }

    #[test]
    fn round_trip_block() {
        let zz: [i16; 64] = std::array::from_fn(|i| (i as i16) * 3 - 50);
        let nat = zigzag_to_natural(&zz);
        let back = natural_to_zigzag(&nat);
        assert_eq!(zz, back);
    }

    #[test]
    fn dc_position_is_zero_in_both_orders() {
        assert_eq!(ZIGZAG[0], 0);
        assert_eq!(INV_ZIGZAG[0], 0);
    }
}
