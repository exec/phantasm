use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

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
