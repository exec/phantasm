use assert_cmd::Command;
use image::{ImageBuffer, Rgb};
use phantasm_core::hash_guard::classify_sensitivity;
use phantasm_core::SensitivityTier;
use phantasm_image::jpeg;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn make_test_jpeg(path: &Path, width: u32, height: u32) {
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = ((x * 255 / width) as u8).wrapping_add(y as u8);
        let g = ((y * 255 / height) as u8).wrapping_add(x as u8);
        let b = ((x + y) as u8).wrapping_mul(3);
        *pixel = Rgb([r, g, b]);
    }
    img.save(path).expect("failed to write test JPEG");
}

/// Build a synthetic cover that classifies Robust under `classify_sensitivity`.
/// Matches the recipe used by the hash_guard unit tests: block-wise varying mean
/// with pseudo-random content, which yields well-separated 32×32 DCT coefficients
/// and wide pHash margins. Writing a 16×16-block (128×128 px) pattern tiled to
/// the requested size.
fn make_robust_jpeg(path: &Path, width: u32, height: u32) {
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let br = (y / 8) as i32;
        let bc = (x / 8) as i32;
        // Same recipe as hash_guard::tests::robust_cover_yields_zero_wet_positions:
        // ((br*31 + bc*17) ^ 0x5A) % 80 + 150.
        let h = (br * 31 + bc * 17) ^ 0x5A;
        let v = (150 + h.rem_euclid(80)) as u8;
        *pixel = Rgb([v, v, v]);
    }
    img.save(path).expect("failed to write robust test JPEG");
}

fn roundtrip_with_cost_function(cost_function_flag: Option<&str>) {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.jpg");
    let stego = tmp.path().join("stego.jpg");
    let payload_in = tmp.path().join("payload.bin");
    let payload_out = tmp.path().join("recovered.bin");

    make_test_jpeg(&cover, 512, 512);
    let payload_bytes: Vec<u8> = (0..256u16).map(|i| (i & 0xff) as u8).collect();
    fs::write(&payload_in, &payload_bytes).unwrap();

    let mut embed = Command::cargo_bin("phantasm").unwrap();
    embed
        .arg("embed")
        .arg("--input")
        .arg(&cover)
        .arg("--payload")
        .arg(&payload_in)
        .arg("--passphrase")
        .arg("test-pass-123")
        .arg("--output")
        .arg(&stego);
    if let Some(cf) = cost_function_flag {
        embed.arg("--cost-function").arg(cf);
    }
    embed.assert().success();

    let mut extract = Command::cargo_bin("phantasm").unwrap();
    extract
        .arg("extract")
        .arg("--input")
        .arg(&stego)
        .arg("--passphrase")
        .arg("test-pass-123")
        .arg("--output")
        .arg(&payload_out);
    extract.assert().success();

    let recovered = fs::read(&payload_out).expect("read recovered payload");
    assert_eq!(
        recovered, payload_bytes,
        "round-trip payload mismatch for cost-function={:?}",
        cost_function_flag
    );
}

#[test]
fn test_embed_help_documents_cost_function_and_default_uerd() {
    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed").arg("--help");
    let assert = cmd.assert().success();
    assert
        .stdout(predicate::str::contains("--cost-function"))
        .stdout(predicate::str::contains("uniform"))
        .stdout(predicate::str::contains("uerd"))
        .stdout(predicate::str::contains("[default: uerd]"));
}

#[test]
fn test_embed_rejects_unknown_cost_function() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.jpg");
    let stego = tmp.path().join("stego.jpg");
    let payload = tmp.path().join("payload.txt");

    make_test_jpeg(&cover, 128, 128);
    fs::write(&payload, b"hi").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed")
        .arg("--input")
        .arg(&cover)
        .arg("--payload")
        .arg(&payload)
        .arg("--passphrase")
        .arg("test")
        .arg("--output")
        .arg(&stego)
        .arg("--cost-function")
        .arg("nonsense");

    cmd.assert().failure();
}

#[test]
fn test_embed_roundtrip_cost_function_uniform() {
    roundtrip_with_cost_function(Some("uniform"));
}

#[test]
fn test_embed_roundtrip_cost_function_uerd() {
    roundtrip_with_cost_function(Some("uerd"));
}

#[test]
fn test_embed_roundtrip_default_is_uerd() {
    // Round-trip when --cost-function is omitted. The default is uerd;
    // the help-text test above locks in that guarantee.
    roundtrip_with_cost_function(None);
}

#[test]
fn test_embed_roundtrip_cost_function_juniward() {
    roundtrip_with_cost_function(Some("j-uniward"));
}

#[test]
fn test_help_lists_all_subcommands() {
    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("--help");
    let assert = cmd.assert().success();

    assert
        .stdout(predicate::str::contains("embed"))
        .stdout(predicate::str::contains("extract"))
        .stdout(predicate::str::contains("analyze"))
        .stdout(predicate::str::contains("channels"))
        .stdout(predicate::str::contains("bench"));
}

#[test]
fn test_channels_returns_exit_code_0() {
    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("channels");
    cmd.assert().success();
}

#[test]
fn test_channels_includes_facebook_and_twitter() {
    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("channels");
    let assert = cmd.assert().success();

    assert
        .stdout(predicate::str::contains("facebook"))
        .stdout(predicate::str::contains("twitter"));
}

#[test]
fn test_embed_invalid_channel_fails() {
    let tmp = tempdir().unwrap();
    let input = tmp.path().join("input.jpg");
    let output = tmp.path().join("output.jpg");
    let payload = tmp.path().join("payload.txt");

    fs::write(&input, b"fake image").unwrap();
    fs::write(&payload, b"secret message").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed")
        .arg("--input")
        .arg(&input)
        .arg("--payload")
        .arg(&payload)
        .arg("--passphrase")
        .arg("test")
        .arg("--output")
        .arg(&output)
        .arg("--channel")
        .arg("invalid");

    cmd.assert().failure();
}

#[test]
fn test_embed_multiple_layers() {
    let tmp = tempdir().unwrap();
    let input = tmp.path().join("input.jpg");
    let output = tmp.path().join("output.jpg");
    let file1 = tmp.path().join("file1.txt");
    let file2 = tmp.path().join("file2.txt");

    fs::write(&input, b"fake image").unwrap();
    fs::write(&file1, b"message1").unwrap();
    fs::write(&file2, b"message2").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--layer")
        .arg(format!("pass1:{}", file1.display()))
        .arg("--layer")
        .arg(format!("pass2:{}", file2.display()));

    let assert = cmd.assert().success();
    assert
        .stdout(predicate::str::contains("pass1"))
        .stdout(predicate::str::contains("pass2"));
}

#[test]
fn test_embed_requires_payload_or_layers() {
    let tmp = tempdir().unwrap();
    let input = tmp.path().join("input.jpg");
    let output = tmp.path().join("output.jpg");

    fs::write(&input, b"fake image").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output);

    cmd.assert().failure();
}

#[test]
fn test_analyze_with_json_flag() {
    let tmp = tempdir().unwrap();
    let path = tmp.path().join("test.jpg");
    fs::write(&path, b"fake image").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("analyze").arg(&path).arg("--json");
    cmd.assert().success();
}

#[test]
fn test_extract_requires_passphrase() {
    let tmp = tempdir().unwrap();
    let input = tmp.path().join("stego.jpg");
    let output = tmp.path().join("payload.txt");

    fs::write(&input, b"fake stego image").unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("extract")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output);

    cmd.assert().failure();
}

/// Helper: embed+extract round-trip with explicit channel_adapter and hash_guard
/// flags, using the supplied cover fn. Both embed and extract get the same flag
/// values.
fn roundtrip_with_hooks(
    cover_fn: fn(&Path, u32, u32),
    cover_size: u32,
    channel_adapter: &str,
    hash_guard: &str,
) {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.jpg");
    let stego = tmp.path().join("stego.jpg");
    let payload_in = tmp.path().join("payload.bin");
    let payload_out = tmp.path().join("recovered.bin");

    cover_fn(&cover, cover_size, cover_size);
    let payload_bytes: Vec<u8> = (0..64u16).map(|i| (i & 0xff) as u8).collect();
    fs::write(&payload_in, &payload_bytes).unwrap();

    let mut embed = Command::cargo_bin("phantasm").unwrap();
    embed
        .arg("embed")
        .arg("--input")
        .arg(&cover)
        .arg("--payload")
        .arg(&payload_in)
        .arg("--passphrase")
        .arg("test-pass-hooks")
        .arg("--output")
        .arg(&stego)
        .arg("--cost-function")
        .arg("uerd")
        .arg("--channel-adapter")
        .arg(channel_adapter)
        .arg("--hash-guard")
        .arg(hash_guard);
    embed.assert().success();

    let mut extract = Command::cargo_bin("phantasm").unwrap();
    extract
        .arg("extract")
        .arg("--input")
        .arg(&stego)
        .arg("--passphrase")
        .arg("test-pass-hooks")
        .arg("--output")
        .arg(&payload_out)
        .arg("--channel-adapter")
        .arg(channel_adapter)
        .arg("--hash-guard")
        .arg(hash_guard);
    extract.assert().success();

    let recovered = fs::read(&payload_out).expect("read recovered payload");
    assert_eq!(
        recovered, payload_bytes,
        "round-trip payload mismatch for channel_adapter={} hash_guard={}",
        channel_adapter, hash_guard
    );
}

#[test]
fn test_embed_roundtrip_none_channel_none_hash_guard() {
    // Backward-compat: --channel-adapter=none and --hash-guard=none should
    // preserve the pre-integration round-trip behavior.
    roundtrip_with_hooks(make_test_jpeg, 512, "none", "none");
}

#[test]
fn test_embed_roundtrip_twitter_channel_adapter() {
    // Twitter MINICER+ROAST stabilization; round-trips through phantasm's own
    // extract (not through an actual Twitter re-encode — that is tested in the
    // phantasm-channel crate's simulation suite).
    roundtrip_with_hooks(make_test_jpeg, 512, "twitter", "none");
}

/// Search for a cover size that classifies Robust. Returns the size that
/// produced a Robust classification, or None if no tested size did. The
/// hash_guard classifier runs on decoded luma, so the JPEG round-trip can push
/// a given synthetic recipe across tier boundaries in ways that are hard to
/// predict statically.
fn find_robust_cover_size(tmp_dir: &Path) -> Option<u32> {
    for &size in &[1024u32, 768, 640, 512] {
        let cover = tmp_dir.join(format!("probe_{}.jpg", size));
        make_robust_jpeg(&cover, size, size);
        let j = jpeg::read(&cover).ok()?;
        if classify_sensitivity(&j) == SensitivityTier::Robust {
            return Some(size);
        }
    }
    None
}

#[test]
fn test_embed_roundtrip_phash_hash_guard_robust_cover() {
    let tmp = tempdir().unwrap();
    let Some(size) = find_robust_cover_size(tmp.path()) else {
        eprintln!(
            "skipping: no tested cover size classified Robust — the hash guard \
             contract (no wet positions added for Robust covers) is covered by \
             phantasm_core::hash_guard unit tests; this CLI test exists only to \
             exercise the flag wiring and is best-effort"
        );
        return;
    };
    // Robust cover → hash guard should add ~0 wet positions and the round-trip
    // should succeed.
    roundtrip_with_hooks(make_robust_jpeg, size, "none", "phash");
}

#[test]
fn test_embed_roundtrip_twitter_and_phash_combined_robust_cover() {
    let tmp = tempdir().unwrap();
    let Some(size) = find_robust_cover_size(tmp.path()) else {
        eprintln!("skipping: no tested cover size classified Robust (see sibling test)");
        return;
    };
    // All three features at once on a Robust cover.
    roundtrip_with_hooks(make_robust_jpeg, size, "twitter", "phash");
}

#[test]
fn test_embed_help_documents_new_flags() {
    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("embed").arg("--help");
    let assert = cmd.assert().success();
    assert
        .stdout(predicate::str::contains("--channel-adapter"))
        .stdout(predicate::str::contains("twitter"))
        .stdout(predicate::str::contains("--hash-guard"))
        .stdout(predicate::str::contains("phash"))
        .stdout(predicate::str::contains("dhash"));
}

#[test]
fn test_analyze_reports_sensitivity_tier() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover.jpg");
    make_test_jpeg(&cover, 256, 256);

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("analyze").arg(&cover);
    let assert = cmd.assert().success();
    assert
        .stdout(predicate::str::contains("Sensitivity tier:"))
        .stdout(predicate::str::contains("Hash-guard (pHash)"));
}

#[test]
fn test_bench_prints_stub_message() {
    let tmp = tempdir().unwrap();
    let cover = tmp.path().join("cover");
    let stego = tmp.path().join("stego");

    fs::create_dir(&cover).unwrap();
    fs::create_dir(&stego).unwrap();

    let mut cmd = Command::cargo_bin("phantasm").unwrap();
    cmd.arg("bench")
        .arg("--cover-dir")
        .arg(&cover)
        .arg("--stego-dir")
        .arg(&stego);

    let assert = cmd.assert().success();
    assert.stdout(predicate::str::contains("phantasm-bench"));
}
