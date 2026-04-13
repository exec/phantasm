use crate::double_layer::{DoubleLayerDecoder, DoubleLayerEncoder};
use crate::{StcConfig, StcDecoder, StcEncoder, StcError};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

fn make_encoder(h: u8) -> StcEncoder {
    StcEncoder::new(StcConfig {
        constraint_height: h,
    })
}

fn make_decoder(h: u8) -> StcDecoder {
    StcDecoder::new(StcConfig {
        constraint_height: h,
    })
}

fn random_bits(rng: &mut StdRng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.gen_range(0u8..=1)).collect()
}

fn random_costs(rng: &mut StdRng, len: usize) -> Vec<f64> {
    (0..len).map(|_| rng.gen_range(0.01f64..1.0)).collect()
}

// Test 1: tiny roundtrip — cover=20, msg=4, rate=1/5, h=7
#[test]
fn test_tiny_roundtrip() {
    let mut rng = StdRng::seed_from_u64(42);
    let cover = random_bits(&mut rng, 20);
    let message = random_bits(&mut rng, 4);
    let costs = random_costs(&mut rng, 20);

    let enc = make_encoder(7);
    let dec = make_decoder(7);

    let stego = enc.embed(&cover, &costs, &message).unwrap();
    assert_eq!(stego.len(), 20);
    let extracted = dec.extract(&stego, 4);
    assert_eq!(extracted, message, "tiny roundtrip failed");
}

// Test 2: random roundtrip, 100 seeds — cover=1024, msg=128, rate=1/8, h=7
#[test]
fn test_random_roundtrip_many_seeds() {
    let enc = make_encoder(7);
    let dec = make_decoder(7);

    for seed in 0u64..100 {
        let mut rng = StdRng::seed_from_u64(seed);
        let cover = random_bits(&mut rng, 1024);
        let message = random_bits(&mut rng, 128);
        let costs = random_costs(&mut rng, 1024);

        let stego = enc.embed(&cover, &costs, &message).unwrap();
        let extracted = dec.extract(&stego, 128);
        assert_eq!(extracted, message, "roundtrip failed at seed {seed}");
    }
}

// Test 3: wet paper roundtrip — 10% infinite costs
#[test]
fn test_wet_paper_roundtrip() {
    let enc = make_encoder(7);
    let dec = make_decoder(7);

    for seed in 0u64..100 {
        let mut rng = StdRng::seed_from_u64(seed + 1000);
        let cover = random_bits(&mut rng, 1024);
        let message = random_bits(&mut rng, 128);
        let mut costs = random_costs(&mut rng, 1024);

        // Mark ~10% as wet
        let wet: Vec<usize> = (0..1024).filter(|_| rng.gen_bool(0.1)).collect();
        for &i in &wet {
            costs[i] = f64::INFINITY;
        }

        let stego = enc.embed(&cover, &costs, &message).unwrap();

        // (a) wet positions must not be flipped
        for &i in &wet {
            assert_eq!(
                stego[i], cover[i],
                "wet position {i} was flipped (seed {seed})"
            );
        }

        // (b) extraction recovers the message
        let extracted = dec.extract(&stego, 128);
        assert_eq!(extracted, message, "wet roundtrip failed at seed {seed}");
    }
}

// Test 4: distortion bound — cover=2048, msg=256, rate=1/8, uniform cost=1, h=10
#[test]
fn test_distortion_bound() {
    let enc = make_encoder(10);

    let total_distortion: f64 = (0u64..20)
        .map(|seed| {
            let mut rng = StdRng::seed_from_u64(seed + 2000);
            let cover = random_bits(&mut rng, 2048);
            let message = random_bits(&mut rng, 256);
            let costs = vec![1.0f64; 2048];

            let stego = enc.embed(&cover, &costs, &message).unwrap();
            let dist: f64 = cover
                .iter()
                .zip(stego.iter())
                .map(|(&c, &s)| if c != s { 1.0 } else { 0.0 })
                .sum();
            dist
        })
        .sum();

    let avg_distortion = total_distortion / 20.0;
    let threshold = 0.25 * 256.0; // 64 flips on average
    assert!(
        avg_distortion < threshold,
        "average distortion {avg_distortion:.2} >= threshold {threshold:.2}; STC may not be working"
    );
}

// Test 5: constraint height scan
//
// Uses n=1024, m=128 (w=8) with 20 seeds. The theoretical monotone improvement
// of STC with h is asymptotic (n >> 2^h required); at w=8 and moderate n,
// small variance is expected between adjacent h values. We assert that:
// 1. All (h, seed) pairs roundtrip correctly.
// 2. The highest tested h (11) is not significantly worse than the lowest (5).
#[test]
fn test_constraint_height_scan() {
    let heights = [5u8, 7, 9, 11];
    let seeds = 20u64;

    let mut avg_distortions: Vec<f64> = Vec::new();

    for &h in &heights {
        let enc = make_encoder(h);
        let dec = make_decoder(h);

        let total: f64 = (0..seeds)
            .map(|seed| {
                let mut rng = StdRng::seed_from_u64(seed + 3000);
                let cover = random_bits(&mut rng, 1024);
                let message = random_bits(&mut rng, 128);
                let costs = random_costs(&mut rng, 1024);

                let stego = enc.embed(&cover, &costs, &message).unwrap();
                let extracted = dec.extract(&stego, 128);
                assert_eq!(extracted, message, "roundtrip failed at h={h} seed={seed}");

                cover
                    .iter()
                    .zip(stego.iter())
                    .zip(costs.iter())
                    .map(|((&c, &s), &cost)| if c != s { cost } else { 0.0 })
                    .sum::<f64>()
            })
            .sum();

        avg_distortions.push(total / seeds as f64);
    }

    // The overall trend must be non-increasing. We check that h=11 is not
    // significantly worse than h=5 (within 15% — the asymptotic guarantee
    // requires n >> 2^h which we approach but don't fully satisfy at n=1024, h=11).
    let best_h_dist = avg_distortions[0]; // h=5 baseline
    let last_h_dist = *avg_distortions.last().unwrap(); // h=11
    assert!(
        last_h_dist <= best_h_dist * 1.15,
        "h=11 avg_dist={last_h_dist:.4} significantly worse than h=5 avg_dist={best_h_dist:.4}"
    );

    // Also assert each adjacent pair doesn't degrade by more than 15%.
    for i in 1..avg_distortions.len() {
        let prev = avg_distortions[i - 1];
        let curr = avg_distortions[i];
        assert!(
            curr <= prev * 1.15,
            "h={} avg_dist={curr:.4} significantly worse than h={} avg_dist={prev:.4}",
            heights[i],
            heights[i - 1]
        );
    }
}

// Test 6: edge cases
#[test]
fn test_empty_message() {
    let enc = make_encoder(7);
    let cover = vec![0u8, 1, 0, 1];
    let costs = vec![0.5f64; 4];
    let result = enc.embed(&cover, &costs, &[]);
    // Empty message: return cover unchanged
    assert_eq!(result.unwrap(), cover);
}

#[test]
fn test_length_mismatch() {
    let enc = make_encoder(7);
    let cover = vec![0u8; 10];
    let costs = vec![0.5f64; 10];
    let message = vec![0u8; 3]; // 10 % 3 != 0
    let err = enc.embed(&cover, &costs, &message).unwrap_err();
    assert_eq!(
        err,
        StcError::LengthMismatch {
            cover: 10,
            message: 3
        }
    );
}

#[test]
fn test_zero_cover_nonzero_message() {
    let enc = make_encoder(7);
    let err = enc.embed(&[], &[], &[0u8]).unwrap_err();
    assert_eq!(
        err,
        StcError::LengthMismatch {
            cover: 0,
            message: 1
        }
    );
}

// Test 7: all-wet infeasible
#[test]
fn test_infeasible_wet_paper() {
    let enc = make_encoder(7);
    let dec = make_decoder(7);

    // A cover that does not encode the desired message under all-wet costs.
    // We need syndrome(cover) != message. Generate until we find one.
    let mut rng = StdRng::seed_from_u64(99999);
    let cover = random_bits(&mut rng, 64);
    let message = random_bits(&mut rng, 8);
    let costs = vec![f64::INFINITY; 64];

    let current = dec.extract(&cover, 8);
    if current == message {
        // If by chance they match, the encoder should succeed (return cover).
        let stego = enc.embed(&cover, &costs, &message).unwrap();
        assert_eq!(stego, cover);
    } else {
        // The encoder must return InfeasibleWetPaper.
        let err = enc.embed(&cover, &costs, &message).unwrap_err();
        assert_eq!(err, StcError::InfeasibleWetPaper);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Double-layer STC tests
// ────────────────────────────────────────────────────────────────────────────

fn make_dl_encoder(h: u8) -> DoubleLayerEncoder {
    DoubleLayerEncoder::new(StcConfig {
        constraint_height: h,
    })
}

fn make_dl_decoder(h: u8) -> DoubleLayerDecoder {
    DoubleLayerDecoder::new(StcConfig {
        constraint_height: h,
    })
}

/// Generate random ternary costs (both directions, always finite).
fn random_ternary_costs(rng: &mut StdRng, n: usize) -> (Vec<f64>, Vec<f64>) {
    let cp: Vec<f64> = (0..n).map(|_| rng.gen_range(0.01f64..1.0)).collect();
    let cm: Vec<f64> = (0..n).map(|_| rng.gen_range(0.01f64..1.0)).collect();
    (cp, cm)
}

/// Generate random i16 DCT-like cover coefficients (bounded away from extremes).
fn random_dct_cover(rng: &mut StdRng, n: usize) -> Vec<i16> {
    (0..n).map(|_| rng.gen_range(-1000i16..=1000)).collect()
}

/// Compute the number of bits embeddable: n / m_bits must be integer for both halves.
/// Returns a valid total_bits count for given n.
fn valid_total_bits_for(n: usize, approx_bits: usize) -> usize {
    // We need (total_bits+1)/2 to divide n, and (total_bits/2) to divide n.
    // Simplest: use total_bits = 2 * (n / w) for some w.
    // We use approx_bits and round down to nearest even number where both halves divide n.
    let mut bits = approx_bits;
    loop {
        let m1 = bits.div_ceil(2);
        let m2 = bits - m1;
        if m1 > 0 && n.is_multiple_of(m1) && (m2 == 0 || n.is_multiple_of(m2)) {
            return bits;
        }
        if bits == 0 {
            panic!("cannot find valid total_bits for n={n}");
        }
        bits -= 1;
    }
}

// Test DL-1: tiny roundtrip — cover=32 i16, message=8 bits, h=7
#[test]
fn test_dl_tiny_roundtrip() {
    let mut rng = StdRng::seed_from_u64(10000);
    let cover = random_dct_cover(&mut rng, 32);
    let (cp, cm) = random_ternary_costs(&mut rng, 32);
    let total_bits = valid_total_bits_for(32, 8);
    let message: Vec<u8> = (0..total_bits).map(|_| rng.gen_range(0u8..=1)).collect();

    let enc = make_dl_encoder(7);
    let dec = make_dl_decoder(7);

    let stego = enc.embed(&cover, &cp, &cm, &message).unwrap();
    assert_eq!(stego.len(), 32);
    let extracted = dec.extract(&stego, total_bits);
    assert_eq!(extracted, message, "DL tiny roundtrip failed");
}

// Test DL-2: realistic roundtrip — cover=4096 i16, message=512 bits, h=10, 50 seeds
#[test]
fn test_dl_realistic_roundtrip() {
    let n = 4096;
    let approx_bits = 512;
    let total_bits = valid_total_bits_for(n, approx_bits);

    let enc = make_dl_encoder(10);
    let dec = make_dl_decoder(10);

    for seed in 0u64..50 {
        let mut rng = StdRng::seed_from_u64(seed + 20000);
        let cover = random_dct_cover(&mut rng, n);
        let (cp, cm) = random_ternary_costs(&mut rng, n);
        let message: Vec<u8> = (0..total_bits).map(|_| rng.gen_range(0u8..=1)).collect();

        let stego = enc.embed(&cover, &cp, &cm, &message).unwrap();
        let extracted = dec.extract(&stego, total_bits);
        assert_eq!(
            extracted, message,
            "DL realistic roundtrip failed at seed {seed}"
        );
    }
}

// Test DL-3: wet paper ternary — 20% of positions fully wet (both costs = infinity)
#[test]
fn test_dl_wet_paper_ternary() {
    let n = 4096;
    let total_bits = valid_total_bits_for(n, 512);
    let enc = make_dl_encoder(10);
    let dec = make_dl_decoder(10);

    for seed in 0u64..20 {
        let mut rng = StdRng::seed_from_u64(seed + 30000);
        let cover = random_dct_cover(&mut rng, n);
        let (mut cp, mut cm) = random_ternary_costs(&mut rng, n);
        let message: Vec<u8> = (0..total_bits).map(|_| rng.gen_range(0u8..=1)).collect();

        // Mark ~20% as fully wet.
        let wet: Vec<usize> = (0..n).filter(|_| rng.gen_bool(0.2)).collect();
        for &i in &wet {
            cp[i] = f64::INFINITY;
            cm[i] = f64::INFINITY;
        }

        let stego = enc.embed(&cover, &cp, &cm, &message).unwrap();

        // Fully wet positions must not be modified.
        for &i in &wet {
            assert_eq!(
                stego[i], cover[i],
                "wet position {i} was modified (seed {seed})"
            );
        }

        let extracted = dec.extract(&stego, total_bits);
        assert_eq!(
            extracted, message,
            "DL wet paper roundtrip failed at seed {seed}"
        );
    }
}

// Test DL-4: half-wet ternary — some positions only allow + or only allow -
#[test]
fn test_dl_half_wet_ternary() {
    let n = 4096;
    let total_bits = valid_total_bits_for(n, 512);
    let enc = make_dl_encoder(10);
    let dec = make_dl_decoder(10);

    for seed in 0u64..20 {
        let mut rng = StdRng::seed_from_u64(seed + 40000);
        // Cover: avoid extremes so ±1 is always valid in the allowed direction.
        let cover: Vec<i16> = (0..n).map(|_| rng.gen_range(-500i16..=500)).collect();
        let (mut cp, mut cm) = random_ternary_costs(&mut rng, n);
        let message: Vec<u8> = (0..total_bits).map(|_| rng.gen_range(0u8..=1)).collect();

        // ~15% only allow +1 (costs_minus = infinity)
        // ~15% only allow -1 (costs_plus = infinity)
        for i in 0..n {
            let r: f64 = rng.gen();
            if r < 0.15 {
                cm[i] = f64::INFINITY; // only + allowed
            } else if r < 0.30 {
                cp[i] = f64::INFINITY; // only - allowed
            }
        }

        let stego = enc.embed(&cover, &cp, &cm, &message).unwrap();

        // Positions where only + is allowed must not decrease.
        // Positions where only - is allowed must not increase.
        for i in 0..n {
            if cp[i].is_infinite() && cm[i].is_finite() {
                assert!(
                    stego[i] <= cover[i],
                    "position {i} only allows -, but stego[i]={} > cover[i]={} (seed {seed})",
                    stego[i],
                    cover[i]
                );
            }
            if cm[i].is_infinite() && cp[i].is_finite() {
                assert!(
                    stego[i] >= cover[i],
                    "position {i} only allows +, but stego[i]={} < cover[i]={} (seed {seed})",
                    stego[i],
                    cover[i]
                );
            }
        }

        let extracted = dec.extract(&stego, total_bits);
        assert_eq!(
            extracted, message,
            "DL half-wet roundtrip failed at seed {seed}"
        );
    }
}

// Test DL-5: capacity advantage — double-layer embeds ~2× bits per coefficient
// vs single-layer at the same cover length, averaged over 20 seeds.
//
// The construction embeds m1 bits via plane-0 (±1 moves) and m2 bits via
// plane-1 (±2 moves). Total bits ≈ 2 × single-layer at the same cover length n.
//
// We assert:
//   (a) m_double ≥ 1.5× m_single (roughly 2× raw capacity in bits),
//   (b) bits-per-coefficient (message bits / n) is ≥ 1.5× single-layer,
//   (c) the additional distortion (L1) from double-layer is ≤ 4× single-layer.
#[test]
fn test_dl_capacity_advantage() {
    let n = 4096;
    let seeds = 20u64;

    // Single-layer: m bits = n/8 (rate 1/8, w=8).
    let m_single = n / 8;
    let enc_single = StcEncoder::new(StcConfig {
        constraint_height: 10,
    });

    // Double-layer: ~2× the message bits.
    let m_double = valid_total_bits_for(n, n / 4);
    let enc_double = make_dl_encoder(10);

    let mut total_l1_single = 0.0f64;
    let mut total_l1_double = 0.0f64;

    for seed in 0u64..seeds {
        let mut rng = StdRng::seed_from_u64(seed + 50000);

        let cover_i16: Vec<i16> = (0..n).map(|_| rng.gen_range(-500i16..=500)).collect();
        let cover_bits: Vec<u8> = cover_i16
            .iter()
            .map(|&x| (x.unsigned_abs() as u8) & 1)
            .collect();

        let costs_uniform = vec![1.0f64; n];
        let msg_single: Vec<u8> = (0..m_single).map(|_| rng.gen_range(0u8..=1)).collect();

        let stego_single = enc_single
            .embed(&cover_bits, &costs_uniform, &msg_single)
            .unwrap();
        // Single-layer distortion: count of flipped bits (each costs 1).
        let l1_single: f64 = cover_bits
            .iter()
            .zip(stego_single.iter())
            .map(|(&c, &s)| if c != s { 1.0 } else { 0.0 })
            .sum();
        total_l1_single += l1_single;

        // Double-layer distortion: L1 norm of i16 changes (captures ±2 costing more).
        let cp = costs_uniform.clone();
        let cm = costs_uniform.clone();
        let msg_double: Vec<u8> = (0..m_double).map(|_| rng.gen_range(0u8..=1)).collect();

        let stego_double = enc_double.embed(&cover_i16, &cp, &cm, &msg_double).unwrap();
        let l1_double: f64 = cover_i16
            .iter()
            .zip(stego_double.iter())
            .map(|(&c, &s)| (c - s).unsigned_abs() as f64)
            .sum();
        total_l1_double += l1_double;
    }

    let avg_l1_single = total_l1_single / seeds as f64;
    let avg_l1_double = total_l1_double / seeds as f64;

    // (a) Double-layer embeds ≥ 1.5× more bits.
    let bits_ratio = m_double as f64 / m_single as f64;
    assert!(
        bits_ratio >= 1.5,
        "bits ratio {bits_ratio:.2} < 1.5 — double-layer isn't embedding significantly more bits"
    );

    // (b) Bits-per-coefficient ratio ≥ 1.5: double-layer embeds more bits per cover element.
    let bpc_single = m_single as f64 / n as f64;
    let bpc_double = m_double as f64 / n as f64;
    let bpc_ratio = bpc_double / bpc_single;
    assert!(
        bpc_ratio >= 1.5,
        "bits-per-coefficient ratio {bpc_ratio:.2} < 1.5 \
         (single bpc={bpc_single:.3}, double bpc={bpc_double:.3})"
    );

    // (c) L1 distortion stays bounded: double-layer uses ≤ 4× L1 distortion vs single.
    let l1_ratio = avg_l1_double / avg_l1_single;
    assert!(
        l1_ratio <= 4.0,
        "L1 distortion ratio {l1_ratio:.2} > 4.0 — double-layer distortion too high \
         (single={avg_l1_single:.1}, double={avg_l1_double:.1})"
    );
}

// Test DL-5b: bits-per-L1 efficiency vs single-layer.
//
// With conditional-probability layering the double-layer construction should
// achieve essentially the same bits-per-L1 ratio as single-layer at matched
// rate (asymptotic bound = 1.0). The legacy independent-layer construction
// achieved ~0.68. This test asserts ≥ 0.90, which is a comfortable lower
// bound that catches any regression in the cost decomposition while leaving
// headroom for finite-n / Viterbi quantization noise (h=10, n=4096 measured
// at 0.99).
#[test]
fn test_dl_bits_per_l1_efficiency() {
    let n = 4096;
    let seeds = 30u64;
    let h = 10u8;
    let m_single = n / 8; // 512 bits, w=8
    let m_double = 1024; // 2× single, valid for n=4096 (m1=512, m2=512)

    let enc_single = StcEncoder::new(StcConfig {
        constraint_height: h,
    });
    let enc_double = make_dl_encoder(h);

    let mut tot_l1_s = 0.0f64;
    let mut tot_l1_d = 0.0f64;
    for seed in 0..seeds {
        let mut rng = StdRng::seed_from_u64(seed + 60000);

        let cover_i16: Vec<i16> = (0..n).map(|_| rng.gen_range(-500i16..=500)).collect();
        let cover_bits: Vec<u8> = cover_i16.iter().map(|&x| x.rem_euclid(2) as u8).collect();
        let costs = vec![1.0f64; n];

        let msg_s: Vec<u8> = (0..m_single).map(|_| rng.gen_range(0u8..=1)).collect();
        let stego_s = enc_single.embed(&cover_bits, &costs, &msg_s).unwrap();
        let l1_s: f64 = cover_bits
            .iter()
            .zip(stego_s.iter())
            .map(|(&a, &b)| if a != b { 1.0 } else { 0.0 })
            .sum();
        tot_l1_s += l1_s;

        let cp = vec![1.0f64; n];
        let cm = vec![1.0f64; n];
        let msg_d: Vec<u8> = (0..m_double).map(|_| rng.gen_range(0u8..=1)).collect();
        let stego_d = enc_double.embed(&cover_i16, &cp, &cm, &msg_d).unwrap();
        let l1_d: f64 = cover_i16
            .iter()
            .zip(stego_d.iter())
            .map(|(&a, &b)| (a - b).unsigned_abs() as f64)
            .sum();
        tot_l1_d += l1_d;
    }

    let avg_l1_s = tot_l1_s / seeds as f64;
    let avg_l1_d = tot_l1_d / seeds as f64;
    let bpl1_s = m_single as f64 / avg_l1_s;
    let bpl1_d = m_double as f64 / avg_l1_d;
    let ratio = bpl1_d / bpl1_s;

    assert!(
        ratio >= 0.90,
        "double-layer bits/L1 efficiency {ratio:.3} below 0.90 \
         (single bits/L1={bpl1_s:.3}, double bits/L1={bpl1_d:.3}, \
         single avg L1={avg_l1_s:.1}, double avg L1={avg_l1_d:.1}) \
         — conditional layering may have regressed"
    );
}

// Test DL-6: extreme coefficient values — near i16 bounds, forbidden directions.
#[test]
fn test_dl_extreme_values() {
    let enc = make_dl_encoder(7);
    let dec = make_dl_decoder(7);

    let n = 64;
    let total_bits = valid_total_bits_for(n, 8);

    // Build a cover with coefficients near extremes.
    let cover: Vec<i16> = (0..n)
        .map(|i| match i % 4 {
            0 => i16::MAX - 1,
            1 => i16::MIN + 1,
            2 => 0,
            _ => 100,
        })
        .collect();

    // Set costs so extremes are protected:
    // i16::MAX - 1: can only go down (-1), costs_plus = infinity
    // i16::MIN + 1: can only go up (+1), costs_minus = infinity
    let mut cp = vec![0.5f64; n];
    let mut cm = vec![0.5f64; n];
    for i in 0..n {
        match i % 4 {
            0 => cp[i] = f64::INFINITY, // near MAX: only -1 allowed
            1 => cm[i] = f64::INFINITY, // near MIN: only +1 allowed
            _ => {}
        }
    }

    let mut rng = StdRng::seed_from_u64(99999);
    let message: Vec<u8> = (0..total_bits).map(|_| rng.gen_range(0u8..=1)).collect();

    let stego = enc.embed(&cover, &cp, &cm, &message).unwrap();

    // Verify bounds are not exceeded.
    for i in 0..n {
        // i16 values are always within [i16::MIN, i16::MAX] by the type's definition.
        // We just assert we didn't accidentally produce a non-i16 value (compile-time guarantee).
        let _ = stego[i]; // bounds guaranteed by type
                          // Near-MAX positions must not increase.
        if i % 4 == 0 {
            assert!(stego[i] <= cover[i], "near-MAX position {i} increased");
        }
        // Near-MIN positions must not decrease.
        if i % 4 == 1 {
            assert!(stego[i] >= cover[i], "near-MIN position {i} decreased");
        }
    }

    let extracted = dec.extract(&stego, total_bits);
    assert_eq!(extracted, message, "DL extreme values roundtrip failed");
}
