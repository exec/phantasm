use assert_cmd::Command;
use image::RgbImage;
use tempfile::TempDir;

fn make_rgb_image(w: u32, h: u32, val: u8) -> Vec<u8> {
    vec![val; (w * h * 3) as usize]
}

#[test]
fn test_cli_compare_identical_images() {
    let tmp = TempDir::new().unwrap();
    let cover_dir = tmp.path().join("cover");
    let stego_dir = tmp.path().join("stego");
    std::fs::create_dir_all(&cover_dir).unwrap();
    std::fs::create_dir_all(&stego_dir).unwrap();

    let pixels = make_rgb_image(128, 128, 100);
    let img = RgbImage::from_raw(128, 128, pixels).unwrap();
    img.save(cover_dir.join("test.png")).unwrap();
    img.save(stego_dir.join("test.png")).unwrap();

    let output = Command::cargo_bin("phantasm-bench")
        .unwrap()
        .args([
            "compare",
            cover_dir.to_str().unwrap(),
            stego_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "exit code non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"mean_mse\": 0.0"),
        "expected mean_mse=0.0 in output:\n{stdout}"
    );
}
