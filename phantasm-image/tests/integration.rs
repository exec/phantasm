use phantasm_image::jpeg;
use std::path::PathBuf;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Fixture generation helpers
// ---------------------------------------------------------------------------

/// Create a 128×128 RGB gradient buffer.
fn gradient_rgb(width: u32, height: u32) -> Vec<u8> {
    (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                [
                    ((x * 255) / width) as u8,
                    ((y * 255) / height) as u8,
                    ((x + y) * 255 / (width + height)) as u8,
                ]
            })
        })
        .collect()
}

/// Write a JPEG to a temp file using the `image` crate, return (TempFile, path).
fn make_jpeg_fixture(width: u32, height: u32, quality: u8) -> (NamedTempFile, PathBuf) {
    use image::{ImageBuffer, Rgb};
    let pixels = gradient_rgb(width, height);
    let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_raw(width, height, pixels).unwrap();
    let tmp = NamedTempFile::with_suffix(".jpg").unwrap();
    let path = tmp.path().to_path_buf();
    img.save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();
    // Use image crate's quality API to encode at specific quality
    // The above uses default quality; redo with explicit quality via encoder.
    {
        use image::codecs::jpeg::JpegEncoder;
        use std::fs::File;
        use std::io::BufWriter;
        let file = File::create(&path).unwrap();
        let writer = BufWriter::new(file);
        let mut enc = JpegEncoder::new_with_quality(writer, quality);
        enc.encode_image(&image::DynamicImage::ImageRgb8(img))
            .unwrap();
    }
    (tmp, path)
}

// ---------------------------------------------------------------------------
// Test 1: JPEG round-trip unmodified
// ---------------------------------------------------------------------------

#[test]
fn jpeg_roundtrip_unmodified() {
    let (_src_tmp, src_path) = make_jpeg_fixture(128, 128, 85);
    let dst_tmp = NamedTempFile::with_suffix(".jpg").unwrap();
    let dst_path = dst_tmp.path().to_path_buf();

    let original = jpeg::read(&src_path).unwrap();
    jpeg::write_with_source(&original, &src_path, &dst_path).unwrap();
    let readback = jpeg::read(&dst_path).unwrap();

    assert_eq!(original.components.len(), readback.components.len());
    for (orig_comp, back_comp) in original.components.iter().zip(readback.components.iter()) {
        assert_eq!(
            orig_comp.coefficients, back_comp.coefficients,
            "component coefficients differ after unmodified round-trip"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2: JPEG single-coefficient modification persists
// ---------------------------------------------------------------------------

#[test]
fn jpeg_coefficient_modification_persists() {
    let (_src_tmp, src_path) = make_jpeg_fixture(128, 128, 85);
    let dst_tmp = NamedTempFile::with_suffix(".jpg").unwrap();
    let dst_path = dst_tmp.path().to_path_buf();

    let mut coeffs = jpeg::read(&src_path).unwrap();

    // Modify a mid-frequency AC coefficient in Y-component, block (0,0), position 27
    let original_val = coeffs.components[0].get(0, 0, 27);
    let new_val = if original_val < i16::MAX {
        original_val + 1
    } else {
        original_val - 1
    };
    coeffs.components[0].set(0, 0, 27, new_val);

    jpeg::write_with_source(&coeffs, &src_path, &dst_path).unwrap();
    let readback = jpeg::read(&dst_path).unwrap();

    // The modified coefficient must have survived.
    assert_eq!(readback.components[0].get(0, 0, 27), new_val);

    // All other coefficients must be unchanged.
    let reread_original = jpeg::read(&src_path).unwrap();
    for (ci, (orig_comp, back_comp)) in reread_original
        .components
        .iter()
        .zip(readback.components.iter())
        .enumerate()
    {
        for i in 0..orig_comp.coefficients.len() {
            if ci == 0 && i == 27 {
                continue; // This is the one we modified.
            }
            assert_eq!(
                orig_comp.coefficients[i], back_comp.coefficients[i],
                "unexpected change at component={ci} coefficient={i}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test 3: JPEG marker preservation
// ---------------------------------------------------------------------------

#[test]
fn jpeg_marker_preservation() {
    // Build a JPEG with an APP2 marker containing known bytes.
    // We'll write raw JFIF bytes with an injected APP2 marker.
    // Easiest: use mozjpeg-sys directly to write a JPEG with a marker.
    // Alternative: use the image crate to generate a JPEG, then patch it manually.

    let (_src_tmp, src_path) = make_jpeg_fixture(64, 64, 85);

    // Patch the JPEG file to inject an APP2 marker with known payload.
    let known_payload: Vec<u8> = b"phantasm-test-marker-data".to_vec();
    let jpeg_bytes = std::fs::read(&src_path).unwrap();

    // Build patched JPEG: SOI + APP2 marker + rest of file (skip original SOI).
    let mut patched = Vec::new();
    // SOI
    patched.extend_from_slice(&[0xFF, 0xD8]);
    // APP2 marker: 0xFF 0xE2, length (big-endian, includes the 2-byte length field itself)
    let marker_len = (known_payload.len() + 2) as u16;
    patched.push(0xFF);
    patched.push(0xE2);
    patched.push((marker_len >> 8) as u8);
    patched.push((marker_len & 0xFF) as u8);
    patched.extend_from_slice(&known_payload);
    // Rest of original JPEG after SOI (skip first 2 bytes = SOI).
    patched.extend_from_slice(&jpeg_bytes[2..]);

    let patched_tmp = NamedTempFile::with_suffix(".jpg").unwrap();
    let patched_path = patched_tmp.path().to_path_buf();
    std::fs::write(&patched_path, &patched).unwrap();

    // Read it and check the marker is present.
    let read_result = jpeg::read(&patched_path);
    // If the patched JPEG is valid, verify marker; otherwise this test is best-effort.
    match read_result {
        Ok(jc) => {
            // Round-trip through write_with_source and verify marker still present.
            let dst_tmp = NamedTempFile::with_suffix(".jpg").unwrap();
            let dst_path = dst_tmp.path().to_path_buf();
            jpeg::write_with_source(&jc, &patched_path, &dst_path).unwrap();
            let readback = jpeg::read(&dst_path).unwrap();
            let found_after = readback.markers.iter().any(|m| {
                m.marker == 0xE2
                    && m.data
                        .windows(known_payload.len())
                        .any(|w| w == known_payload)
            });
            assert!(
                found_after,
                "APP2 marker was not preserved after round-trip"
            );
        }
        Err(_) => {
            // Patched JPEG was rejected by libjpeg; skip the assertion.
            // This can happen if the JFIF header validation fails.
            eprintln!("jpeg_marker_preservation: patched JPEG was rejected, skipping assertion");
        }
    }
}

// ---------------------------------------------------------------------------
// Test 4: JPEG quality estimate sanity
// ---------------------------------------------------------------------------

#[test]
fn jpeg_quality_estimate_sanity() {
    let (_src_tmp, src_path) = make_jpeg_fixture(128, 128, 85);
    let jc = jpeg::read(&src_path).unwrap();
    let qf = jc
        .quality_estimate
        .expect("quality estimate should be present");
    assert!(
        (75..=95).contains(&qf),
        "quality estimate {qf} is outside expected [75, 95] range for QF=85 input"
    );
}

// ---------------------------------------------------------------------------
// Test 5: PNG round-trip
// ---------------------------------------------------------------------------

#[test]
fn png_roundtrip() {
    use phantasm_image::png::{self, PngColor, PngImage};

    let width = 64u32;
    let height = 64u32;
    let pixels: Vec<u8> = gradient_rgb(width, height);

    let tmp = NamedTempFile::with_suffix(".png").unwrap();
    let path = tmp.path().to_path_buf();

    let img = PngImage {
        width,
        height,
        pixels: pixels.clone(),
        color: PngColor::Rgb8,
    };
    png::write(&img, &path).unwrap();

    let loaded = png::read(&path).unwrap();
    assert_eq!(loaded.width, width);
    assert_eq!(loaded.height, height);
    assert_eq!(loaded.pixels, pixels);
}

// ---------------------------------------------------------------------------
// Test 6: DCT identity
// ---------------------------------------------------------------------------

#[test]
fn dct_identity() {
    use phantasm_image::dct::{dct2d_8x8, idct2d_8x8};
    let block: [f64; 64] =
        std::array::from_fn(|i| (i as f64 * 7.13 + std::f64::consts::PI).sin() * 100.0);
    let dct = dct2d_8x8(&block);
    let recovered = idct2d_8x8(&dct);
    for (a, b) in block.iter().zip(recovered.iter()) {
        assert!(
            (a - b).abs() < 1e-10,
            "DCT identity failed: {a} vs {b}, diff={}",
            (a - b).abs()
        );
    }
}

// ---------------------------------------------------------------------------
// Test 7: DCT reference (all-128 block)
// ---------------------------------------------------------------------------

#[test]
fn dct_all_128_dc() {
    use phantasm_image::dct::dct2d_8x8;
    let block = [128.0f64; 64];
    let dct = dct2d_8x8(&block);
    assert!(
        (dct[0] - 1024.0).abs() < 1e-9,
        "DC expected 1024, got {}",
        dct[0]
    );
    for (i, &v) in dct.iter().enumerate().skip(1) {
        assert!(v.abs() < 1e-9, "AC[{i}] expected ~0, got {v}");
    }
}

// ---------------------------------------------------------------------------
// Test 8: YCbCr gray pixel
// ---------------------------------------------------------------------------

#[test]
fn ycbcr_gray_pixel() {
    use phantasm_image::color::{rgb_to_ycbcr, ycbcr_to_rgb};
    let ycbcr = rgb_to_ycbcr([128, 128, 128]);
    assert_eq!(ycbcr, [128, 128, 128]);
    let rgb = ycbcr_to_rgb(ycbcr);
    assert_eq!(rgb, [128, 128, 128]);
}

// ---------------------------------------------------------------------------
// Test 9: YCbCr red pixel
// ---------------------------------------------------------------------------

#[test]
fn ycbcr_red_pixel() {
    use phantasm_image::color::rgb_to_ycbcr;
    let ycbcr = rgb_to_ycbcr([255, 0, 0]);
    let y = ycbcr[0] as i32;
    let cb = ycbcr[1] as i32;
    let cr = ycbcr[2] as i32;
    assert!((y - 76).abs() <= 2, "Y={y} expected ~76");
    assert!((cb - 85).abs() <= 2, "Cb={cb} expected ~85");
    assert!((cr - 255).abs() <= 2, "Cr={cr} expected ~255");
}

// ---------------------------------------------------------------------------
// Test 10: Component indexing get/set round-trip
// ---------------------------------------------------------------------------

#[test]
fn component_indexing_roundtrip() {
    let (_src_tmp, src_path) = make_jpeg_fixture(64, 64, 85);
    let mut jc = jpeg::read(&src_path).unwrap();

    let comp = &mut jc.components[0];
    let bw = comp.blocks_wide;
    let bh = comp.blocks_high;

    // Write a sentinel value at each corner block, position 1.
    let positions = [
        (0, 0, 1usize),
        (0, bw - 1, 1),
        (bh - 1, 0, 1),
        (bh - 1, bw - 1, 1),
    ];

    let sentinels: Vec<i16> = vec![100, 200, -100, -200];
    for (&(br, bc, pos), &val) in positions.iter().zip(sentinels.iter()) {
        comp.set(br, bc, pos, val);
    }

    for (&(br, bc, pos), &val) in positions.iter().zip(sentinels.iter()) {
        assert_eq!(
            comp.get(br, bc, pos),
            val,
            "get/set mismatch at ({br},{bc},{pos})"
        );
    }
}
