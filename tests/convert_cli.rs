//! Integration tests for the `confy convert` CLI subcommand (Phase 4).

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use tempfile::TempDir;

fn confy() -> Command {
    Command::cargo_bin("confy").unwrap()
}

#[test]
fn lossless_conversion_writes_and_leaves_source_untouched() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.yaml");
    let src = "{ \"a\": 1, \"b\": [true, \"hi\"] }\n";
    fs::write(&input, src).unwrap();

    confy()
        .args(["convert", input.to_str().unwrap(), output.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        "a: 1\nb:\n  - true\n  - hi\n"
    );
    // Source is byte-identical.
    assert_eq!(fs::read_to_string(&input).unwrap(), src);
}

#[test]
fn lossy_conversion_refuses_without_yes() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.toml");
    let output = dir.path().join("out.json");
    fs::write(&input, "n = 0xFF\n").unwrap();

    confy()
        .args(["convert", input.to_str().unwrap(), output.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(contains("--yes"));

    assert!(!output.exists(), "no file written without confirmation");
}

#[test]
fn lossy_conversion_with_yes_writes_and_warns() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.toml");
    let output = dir.path().join("out.json");
    fs::write(&input, "n = 0xFF\n").unwrap();

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .success()
        .stderr(contains("non-decimal"));

    assert_eq!(fs::read_to_string(&output).unwrap(), "{\n  \"n\": 255\n}\n");
}

#[test]
fn null_to_toml_aborts_with_no_file() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.toml");
    fs::write(&input, "{ \"a\": { \"b\": null } }\n").unwrap();

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(contains("aborted"))
        .stderr(contains("a.b"));

    assert!(!output.exists(), "aborted conversion writes nothing");
}

#[test]
fn explicit_from_to_overrides_extension() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("data.txt");
    let output = dir.path().join("data.out");
    fs::write(&input, "a = 1\n").unwrap();

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--from",
            "toml",
            "--to",
            "json",
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&output).unwrap(), "{\n  \"a\": 1\n}\n");
}
