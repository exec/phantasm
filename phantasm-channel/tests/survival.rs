//! End-to-end survival-rate test for the Twitter channel adapter.
//!
//! Workflow:
//!   1. Generate a synthetic 256×256 RGB cover and save as JPEG (QF 90).
//!   2. Read the JPEG into phantasm-image coefficients + a Uniform cost map.
//!   3. Run the Twitter profile's `stabilize` to perturb non-DC AC
//!      coefficients into parity-stable values.
//!   4. Write the stabilized cover back out.
//!   5. Re-encode through the `image` crate at QF 85 (the Twitter target).
//!   6. Read the re-encoded JPEG and compare each non-wet, stabilized
//!      position's parity against its parity in the stabilized cover.
//!   7. Assert ≥80 % survive.

use image::{ImageBuffer, Rgb};
use phantasm_channel::{ChannelAdapter, TwitterProfile};
use phantasm_cost::{DistortionFunction, Uniform};
use phantasm_image::jpeg::read as read_jpeg;
use phantasm_image::jpeg::write_with_source;
use std::path::PathBuf;

fn make_cover(path: &PathBuf) {
    // Plasma-ish pattern: lots of mid-frequency texture, plus a few
    // smooth gradients. Realistic mix for steg.
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(256, 256, |x, y| {
        let r = (((x as f32 * 0.13).sin() * 40.0 + 128.0) + ((y as f32 * 0.07).cos() * 30.0)) as u8;
        let g = (((x ^ y) % 64) as f32 * 2.0 + 90.0) as u8;
        let b = ((x as i32 - y as i32).unsigned_abs() % 200) as u8 + 30;
        Rgb([r, g, b])
    });
    img.save(path).expect("save cover");
}

fn reencode_through_image_crate(input: &PathBuf, output: &PathBuf, qf: u8) {
    // Decode → re-encode at the requested QF using the `image` crate's
    // baseline JPEG encoder. This is the "channel" surrogate.
    let img = image::open(input).expect("decode cover").to_rgb8();
    let mut out = std::fs::File::create(output).expect("create reenc");
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, qf);
    img.write_with_encoder(encoder).expect("encode reenc");
}

#[test]
fn twitter_profile_survival_above_80_percent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");
    let reenc_path = tmp.path().join("reenc.jpg");

    make_cover(&cover_path);

    let mut jpeg = read_jpeg(&cover_path).expect("read cover");
    let mut cost_map = Uniform.compute(&jpeg, 0);

    let profile = TwitterProfile::default();
    let report = profile
        .stabilize(&mut jpeg, 0, &mut cost_map)
        .expect("stabilize ok");

    // Sanity: we should have stabilized *some* positions and not flagged
    // the entire cost map as wet.
    let total = cost_map.positions.len();
    assert!(total > 0);
    let finite = cost_map.costs_plus.iter().filter(|c| c.is_finite()).count();
    assert!(
        finite > 0,
        "no finite-cost positions left after stabilization \
         (sacrificed {} blocks)",
        report.sacrificed_blocks
    );
    assert!(
        report.stabilized_count > 0,
        "no positions were stabilized at all"
    );

    // Snapshot the parity of every stabilized (finite-cost) position
    // BEFORE writing & re-encoding.
    let comp = &jpeg.components[0];
    let bw = comp.blocks_wide;
    let mut snapshot: Vec<((usize, usize, usize), i16)> = Vec::new();
    for (i, &(br, bc, dp)) in cost_map.positions.iter().enumerate() {
        if cost_map.costs_plus[i].is_finite() {
            let v = comp.coefficients[(br * bw + bc) * 64 + dp];
            snapshot.push(((br, bc, dp), v));
        }
    }
    let n = snapshot.len();
    assert!(n > 100, "need a non-trivial test, got n={n}");

    // Write the stabilized cover so the channel surrogate can read it.
    write_with_source(&jpeg, &cover_path, &stego_path).expect("write stego");

    // Channel re-encode.
    reencode_through_image_crate(&stego_path, &reenc_path, profile.target_qf);

    // Read back and compare parity at each snapshot position. Note: the
    // re-encoded JPEG's blocks_wide may differ if `image` crate produces
    // a slightly different layout, but for an aligned size it shouldn't.
    let re_jpeg = read_jpeg(&reenc_path).expect("read reenc");
    let re_comp = &re_jpeg.components[0];
    assert_eq!(
        re_comp.blocks_wide, comp.blocks_wide,
        "reencoded blocks_wide changed"
    );
    assert_eq!(
        re_comp.blocks_high, comp.blocks_high,
        "reencoded blocks_high changed"
    );

    let mut survived = 0usize;
    for ((br, bc, dp), original_v) in &snapshot {
        let new_v = re_comp.coefficients[(br * re_comp.blocks_wide + bc) * 64 + dp];
        if (new_v.rem_euclid(2)) == (original_v.rem_euclid(2)) {
            survived += 1;
        }
    }
    let rate = survived as f64 / n as f64;
    eprintln!(
        "twitter survival: {survived}/{n} = {:.1}%; \
         stabilized={}, wet={}, sacrificed_blocks={}",
        rate * 100.0,
        report.stabilized_count,
        report.wet_positions.len(),
        report.sacrificed_blocks,
    );
    assert!(
        rate >= 0.80,
        "survival rate {:.1}% below 80% target",
        rate * 100.0
    );
}
