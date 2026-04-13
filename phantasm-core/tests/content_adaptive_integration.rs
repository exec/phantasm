use image::{ImageBuffer, Rgb};
use phantasm_core::{
    ChannelProfile, ContentAdaptiveOrchestrator, CoreError, EmbedPlan, HashSensitivity,
    MinimalOrchestrator, Orchestrator, StealthTier,
};
use phantasm_cost::Uniform;
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

/// Test 1: Roundtrip with Uniform cost function.
#[test]
fn content_adaptive_uniform_roundtrip() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload: Vec<u8> = (0u8..=255u8).cycle().take(500).collect();
    let plan = make_plan();
    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));

    orchestrator
        .embed(
            &cover_path,
            &payload,
            "test-passphrase-ca",
            &plan,
            &stego_path,
        )
        .expect("embed failed");

    assert!(stego_path.exists(), "stego file not created");

    let recovered = orchestrator
        .extract(&stego_path, "test-passphrase-ca")
        .expect("extract failed");

    assert_eq!(recovered, payload, "payload mismatch");
}

/// Test 2: Embed with MinimalOrchestrator, extract with ContentAdaptive(Uniform).
#[test]
fn cross_orchestrator_minimal_embed_ca_extract() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload: Vec<u8> = b"cross-orchestrator compatibility test payload".to_vec();
    let plan = make_plan();

    MinimalOrchestrator
        .embed(
            &cover_path,
            &payload,
            "shared-passphrase",
            &plan,
            &stego_path,
        )
        .expect("embed failed");

    let recovered = ContentAdaptiveOrchestrator::new(Box::new(Uniform))
        .extract(&stego_path, "shared-passphrase")
        .expect("extract failed");

    assert_eq!(recovered, payload, "cross-orchestrator payload mismatch");
}

/// Test 3: Embed with ContentAdaptive(Uniform), extract with Minimal.
#[test]
fn cross_orchestrator_ca_embed_minimal_extract() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload: Vec<u8> = b"reverse cross-orchestrator compatibility test".to_vec();
    let plan = make_plan();

    ContentAdaptiveOrchestrator::new(Box::new(Uniform))
        .embed(
            &cover_path,
            &payload,
            "shared-passphrase-2",
            &plan,
            &stego_path,
        )
        .expect("embed failed");

    let recovered = MinimalOrchestrator
        .extract(&stego_path, "shared-passphrase-2")
        .expect("extract failed");

    assert_eq!(
        recovered, payload,
        "reverse cross-orchestrator payload mismatch"
    );
}

/// Test 4: Wrong passphrase fails.
#[test]
fn content_adaptive_wrong_passphrase_fails() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 512, 512);

    let payload = b"secret payload for wrong passphrase test";
    let plan = make_plan();
    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));

    orchestrator
        .embed(&cover_path, payload, "correct", &plan, &stego_path)
        .expect("embed failed");

    let result = orchestrator.extract(&stego_path, "wrong");
    assert!(result.is_err(), "expected error with wrong passphrase");
    match result.unwrap_err() {
        CoreError::Crypto(_) | CoreError::InvalidData(_) => {}
        other => panic!("expected Crypto or InvalidData error, got: {:?}", other),
    }
}

/// Test 5: Payload too large errors cleanly.
#[test]
fn content_adaptive_payload_too_large() {
    let tmp = tempdir().unwrap();
    let cover_path = tmp.path().join("cover.jpg");
    let stego_path = tmp.path().join("stego.jpg");

    make_test_jpeg(&cover_path, 128, 128);

    let large_payload = vec![0u8; 100 * 1024];
    let plan = make_plan();
    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));

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

/// Test 6: Distortion name is propagated.
#[test]
fn content_adaptive_distortion_name_propagated() {
    let orchestrator = ContentAdaptiveOrchestrator::new(Box::new(Uniform));
    assert_eq!(orchestrator.distortion_name(), "uniform");
}
