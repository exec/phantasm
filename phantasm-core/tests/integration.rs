use image::{GenericImageView, ImageBuffer, Rgb};
use phantasm_core::{
    ChannelProfile, CoreError, EmbedPlan, HashSensitivity, MinimalOrchestrator, Orchestrator,
    StealthTier,
};
use std::path::PathBuf;
use tempfile::tempdir;

fn make_test_jpeg(path: &PathBuf, width: u32, height: u32) {
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = ((x * 255 / width) as u8).wrapping_add(y as u8);
        let g = ((y * 255 / height) as u8).wrapping_add(x as u8);
        let b = ((x + y) as u8).wrapping_mul(3);
        *pixel = Rgb([r, g, b]);
    }
    img.save(path).expect("failed to write test JPEG");
}

fn make_plan() -> EmbedPlan {
    EmbedPlan {
        channel: ChannelProfile::builtin("lossless").unwrap(),
        stealth_tier: StealthTier::High,
        capacity_bits: 0,
        payload_bits: 0,
        ecc_bits: 0,
        estimated_detection_error: 0.5,
        hash_constrained_positions: 0,
        hash_sensitivity: HashSensitivity::Robust,
    }
}

#[test]
fn end_to_end_roundtrip_jpeg() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload_base = b"The quick brown fox jumps over the lazy dog";
    let mut payload = Vec::new();
    for _ in 0..4 {
        payload.extend_from_slice(payload_base);
    }

    let orchestrator = MinimalOrchestrator;
    let plan = make_plan();

    orchestrator
        .embed(
            &cover_path,
            &payload,
            "test-passphrase-12345",
            &plan,
            &stego_path,
        )
        .expect("embed failed");

    assert!(stego_path.exists(), "stego file not created");

    let cover_img = image::open(&cover_path).expect("open cover");
    let stego_img = image::open(&stego_path).expect("open stego");
    assert_eq!(
        cover_img.dimensions(),
        stego_img.dimensions(),
        "dimensions changed"
    );

    let cover_bytes = cover_img.into_bytes();
    let stego_bytes = stego_img.into_bytes();
    assert_ne!(cover_bytes, stego_bytes, "no modification detected");

    let recovered = orchestrator
        .extract(&stego_path, "test-passphrase-12345")
        .expect("extract failed");

    assert_eq!(recovered, payload, "payload mismatch");
}

#[test]
fn wrong_passphrase_fails() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload = b"secret payload for wrong passphrase test";
    let orchestrator = MinimalOrchestrator;
    let plan = make_plan();

    orchestrator
        .embed(&cover_path, payload, "correct", &plan, &stego_path)
        .expect("embed failed");

    let result = orchestrator.extract(&stego_path, "wrong");
    assert!(result.is_err(), "expected error with wrong passphrase");
    // With wrong passphrase, the permutation differs so extracted bits are garbage.
    // This manifests as InvalidData (bad frame length) or Crypto (auth failure).
    match result.unwrap_err() {
        CoreError::Crypto(_) | CoreError::InvalidData(_) => {}
        other => panic!("expected Crypto or InvalidData error, got: {:?}", other),
    }
}

#[test]
fn payload_too_large_errors_cleanly() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 128, 128);

    let large_payload = vec![0u8; 100 * 1024];
    let orchestrator = MinimalOrchestrator;
    let plan = make_plan();

    let result = orchestrator.embed(
        &cover_path,
        &large_payload,
        "test-passphrase",
        &plan,
        &stego_path,
    );

    assert!(result.is_err(), "expected PayloadTooLarge error");
    match result.unwrap_err() {
        CoreError::PayloadTooLarge { .. } => {}
        other => panic!("expected PayloadTooLarge, got: {:?}", other),
    }
}
