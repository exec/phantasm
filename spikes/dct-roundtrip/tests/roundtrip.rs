use dct_roundtrip::{compare_snapshots, read_coefficients, round_trip, write_coefficients};
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_512() -> PathBuf {
    fixtures_dir().join("fixture_512.jpg")
}

fn fixture_1024() -> PathBuf {
    fixtures_dir().join("fixture_1024.jpg")
}

/// Unmodified round-trip: read → write → re-read, all coefficients must match.
#[test]
fn test_unmodified_roundtrip_512() {
    let input = fixture_512();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let output = tmp.path().to_path_buf();

    let snap_original = read_coefficients(&input).expect("read input");
    write_coefficients(&input, &output, &snap_original).expect("write output");
    let snap_readback = read_coefficients(&output).expect("read output");

    let total = compare_snapshots(&snap_original, &snap_readback)
        .expect("snapshots should be bit-exact");

    assert!(total > 0, "expected at least one coefficient");
    println!("unmodified roundtrip: {} coefficients matched", total);
}

/// Single-coefficient modification: modified coef persists, no other coef changes.
#[test]
fn test_single_coeff_modification() {
    let input = fixture_512();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let output = tmp.path().to_path_buf();

    // Read original
    let original = read_coefficients(&input).expect("read input");

    // Modify one coefficient: luminance component, first block, DCT index 27 (position 3,3)
    let comp = 0;
    let block_row = 0u32;
    let block_col = 0u32;
    let coef_idx = 27usize;

    let orig_val = original.coeff(comp, block_row, block_col, coef_idx);
    let new_val = orig_val.wrapping_add(1);

    let mut modified = read_coefficients(&input).expect("read input again");
    modified.set_coeff(comp, block_row, block_col, coef_idx, new_val);

    write_coefficients(&input, &output, &modified).expect("write modified");

    let readback = read_coefficients(&output).expect("read modified output");

    // The modified coefficient must persist
    let readback_val = readback.coeff(comp, block_row, block_col, coef_idx);
    assert_eq!(
        readback_val, new_val,
        "modified coef should read back as {}, got {}",
        new_val, readback_val
    );

    // All other coefficients must be unchanged relative to the modified snapshot
    let total = compare_snapshots(&modified, &readback)
        .expect("all coefficients (including modification) should match exactly");

    assert!(total > 0);
    println!(
        "single coeff modification: orig={} new={} readback={}, {} total coefficients verified",
        orig_val, new_val, readback_val, total
    );
}

/// Large image round-trip: ≥1024×1024 to ensure multi-block correctness.
#[test]
fn test_roundtrip_1024() {
    let input = fixture_1024();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let output = tmp.path().to_path_buf();

    let result = round_trip(&input, &output, false).expect("round trip should succeed");

    // Expect many coefficients from a 1024x1024 image
    // 1024/8 = 128 blocks per row/col, 3 components (Y, Cb, Cr with chroma subsampling)
    // Y: 128*128*64 = 1,048,576; at minimum expect > 1M coefficients
    assert!(
        result.total_coeffs > 1_000_000,
        "expected >1M coefficients from 1024x1024 image, got {}",
        result.total_coeffs
    );
    println!(
        "1024x1024 round-trip: {} total coefficients verified bit-exact",
        result.total_coeffs
    );
}

/// Verify that modification of a coefficient in the 1024 fixture also persists.
#[test]
fn test_single_coeff_modification_1024() {
    let input = fixture_1024();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let output = tmp.path().to_path_buf();

    let result = round_trip(&input, &output, true).expect("modified round trip should succeed");

    let wrote = result.new_value.unwrap();
    let read = result.verified_value.unwrap();
    assert_eq!(
        wrote, read,
        "1024 fixture: wrote {} but read back {}",
        wrote, read
    );
    println!(
        "1024 fixture single coeff: orig={} -> written={} -> readback={}, {} coefficients verified",
        result.original_value.unwrap(),
        wrote,
        read,
        result.total_coeffs
    );
}
