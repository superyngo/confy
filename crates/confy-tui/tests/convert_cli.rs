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

    // `--lang en` pinned: these assert on the ENGLISH message text, and the
    // language falls back to the developer's own `~/.config/confy/config.toml`
    // when no flag is given (see `convert_cli_respects_config_file_lang_when_no_flag`),
    // so an unpinned assertion fails on a zh-TW machine.
    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--lang",
            "en",
        ])
        .assert()
        .failure()
        .stderr(contains("--yes"));

    assert!(!output.exists(), "no file written without confirmation");
}

#[test]
fn existing_output_is_not_overwritten_without_yes() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.yaml");
    fs::write(&input, "{ \"a\": 1 }\n").unwrap();
    fs::write(&output, "precious: true\n").unwrap();

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--lang",
            "en",
        ])
        .assert()
        .failure()
        .stderr(contains("refusing to overwrite an existing file without --yes"));

    assert_eq!(fs::read_to_string(&output).unwrap(), "precious: true\n");
}

#[test]
fn existing_output_is_overwritten_with_yes() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.yaml");
    fs::write(&input, "{ \"a\": 1 }\n").unwrap();
    fs::write(&output, "precious: true\n").unwrap();

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&output).unwrap(), "a: 1\n");
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
            "--lang",
            "en",
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
            "--lang",
            "en",
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

#[test]
fn convert_cli_respects_lang_flag_zh_tw() {
    use confy_core::session::{tr_args, Lang};
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.yaml");
    fs::write(&input, "{ \"a\": 1 }\n").unwrap();

    let expected_wrote = tr_args(Lang::ZhTw, "cli.convert.wrote", &[output.to_str().unwrap()]);

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--lang",
            "zh-TW",
        ])
        .assert()
        .success()
        .stderr(contains(&expected_wrote));
}

#[test]
fn convert_cli_lossy_refusal_renders_in_zh_tw() {
    use confy_core::session::{tr, Lang};
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.toml");
    let output = dir.path().join("out.json");
    fs::write(&input, "n = 0xFF\n").unwrap();

    let expected_refusal = tr(Lang::ZhTw, "cli.convert.refuse-non-interactive");

    confy()
        .args([
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--lang",
            "zh-TW",
        ])
        .assert()
        .failure()
        .stderr(contains(expected_refusal));
}

#[test]
fn convert_cli_respects_config_file_lang_when_no_flag() {
    use confy_core::session::{tr_args, Lang};
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("in.json");
    let output = dir.path().join("out.yaml");
    fs::write(&input, "{ \"a\": 1 }\n").unwrap();

    // Write a config file with lang = "zh-TW"
    let config_dir = dir.path().join("confy");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.toml"), "lang = \"zh-TW\"\n").unwrap();

    let expected_wrote = tr_args(Lang::ZhTw, "cli.convert.wrote", &[output.to_str().unwrap()]);

    confy()
        .env("XDG_CONFIG_HOME", dir.path())
        .args(["convert", input.to_str().unwrap(), output.to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains(&expected_wrote));
}
