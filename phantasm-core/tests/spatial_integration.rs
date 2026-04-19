//! Integration tests for the spatial (PNG) embed/extract pipeline.
//!
//! These produce a small PNG fixture at runtime, embed a payload, extract it,
//! and assert byte-identical recovery.

use image::{ImageBuffer, Luma};
use phantasm_core::pipeline_spatial::{embed_png, extract_png, SpatialCost};
use std::path::PathBuf;
use tempfile::tempdir;

/// A 128×128 grayscale texture mixing a smooth gradient and structured noise.
/// Both S-UNIWARD and Uniform embeds must work on this.
fn write_textured_png(path: &PathBuf) {
    let (w, h) = (128u32, 128u32);
    let img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
        let gradient = (x + y) & 0xFF;
        let tex = (x.wrapping_mul(13) ^ y.wrapping_mul(29)) & 0x3F;
        // Hold pixels safely away from saturation (16..240) so the STC has
        // room to flip LSBs in either direction without saturation wet-marks.
        let v = 16 + ((gradient + tex) % 224);
        Luma([v as u8])
    });
    img.save(path).expect("write PNG fixture");
}

#[test]
fn png_s_uniward_roundtrip() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.png");
    let stego = tmp.path().join("stego.png");
    write_textured_png(&cover);

    let payload: Vec<u8> = (0u8..64).collect();
    let passphrase = "test-passphrase-s-uniward";

    let result =
        embed_png(&cover, &payload, passphrase, SpatialCost::SUniward, &stego).expect("embed");
    assert_eq!(result.bytes_embedded, payload.len());

    let recovered = extract_png(&stego, passphrase).expect("extract");
    assert_eq!(recovered, payload);
}

#[test]
fn png_uniform_roundtrip() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.png");
    let stego = tmp.path().join("stego.png");
    write_textured_png(&cover);

    let payload: Vec<u8> = b"phantasm png uniform cost roundtrip".to_vec();
    let passphrase = "test-passphrase-uniform";

    embed_png(&cover, &payload, passphrase, SpatialCost::Uniform, &stego).expect("embed");
    let recovered = extract_png(&stego, passphrase).expect("extract");
    assert_eq!(recovered, payload);
}

#[test]
fn png_wrong_passphrase_fails_cleanly() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.png");
    let stego = tmp.path().join("stego.png");
    write_textured_png(&cover);

    let payload: Vec<u8> = (0u8..32).collect();
    embed_png(&cover, &payload, "right-pw", SpatialCost::SUniward, &stego).expect("embed");
    let err = extract_png(&stego, "wrong-pw").unwrap_err();
    // A wrong passphrase reads a different permutation of bits; we want a
    // clean AuthFailed rather than a panic or opaque length error.
    let msg = err.to_string();
    assert!(
        msg.contains("authentication failed") || msg.contains("auth"),
        "expected auth failure, got: {msg}"
    );
}
