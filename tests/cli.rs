//! Integration tests that exercise the compiled `qrgen` binary.

use std::io::Write;
use std::process::{Command, Stdio};

fn qrgen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_qrgen"))
}

/// Run `qrgen` with the given args, feeding `input` to its stdin.
fn run_with_stdin(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = qrgen()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn qrgen");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(input)
        .expect("failed to write to stdin");
    child.wait_with_output().expect("failed to wait for qrgen")
}

#[test]
fn rejects_scale_out_of_range() {
    for scale in ["0", "257"] {
        let out = qrgen()
            .args(["text", "hi", "--scale", scale])
            .output()
            .expect("failed to run qrgen");
        assert!(!out.status.success(), "--scale {scale} should be rejected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("--scale"), "stderr: {stderr}");
    }
}

#[test]
fn rejects_margin_out_of_range() {
    let out = qrgen()
        .args(["text", "hi", "--margin", "65"])
        .output()
        .expect("failed to run qrgen");
    assert!(!out.status.success(), "--margin 65 should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--margin"), "stderr: {stderr}");
}

#[test]
fn accepts_scale_and_margin_at_bounds() {
    let out = qrgen()
        .args(["text", "hi", "--scale", "1", "--margin", "0"])
        .output()
        .expect("failed to run qrgen");
    assert!(out.status.success());
    assert!(!out.stdout.is_empty(), "terminal art should be printed");

    let out = qrgen()
        .args(["text", "hi", "--scale", "256", "--margin", "64"])
        .arg("--help")
        .output()
        .expect("failed to run qrgen");
    // --help short-circuits rendering, so this only checks argument parsing.
    assert!(out.status.success());
}

#[test]
fn rejects_output_dimension_over_the_cap() {
    // Even a version-1 QR (21 modules) at the CLI maxima is
    // (21 + 2*64) * 256 = 38144 px, past the 32768 px cap.
    let dir = std::env::temp_dir().join("qrgen-cli-test-cap");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let path = dir.join("too-big.png");
    let out = qrgen()
        .args(["text", "hi", "--scale", "256", "--margin", "64"])
        .arg("--output")
        .arg(&path)
        .output()
        .expect("failed to run qrgen");
    assert!(!out.status.success(), "oversized output should be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("reduce --scale or --margin"),
        "stderr: {stderr}"
    );
    assert!(!path.exists(), "no file should be written");
}

#[test]
fn compact_halves_terminal_output_lines() {
    let normal = qrgen()
        .args(["text", "https://printemps.tokyo"])
        .output()
        .expect("failed to run qrgen");
    assert!(normal.status.success());
    let compact = qrgen()
        .args(["text", "https://printemps.tokyo", "--compact"])
        .output()
        .expect("failed to run qrgen");
    assert!(compact.status.success());

    let normal_lines = normal.stdout.iter().filter(|&&b| b == b'\n').count();
    let compact_lines = compact.stdout.iter().filter(|&&b| b == b'\n').count();
    assert!(normal_lines > 0);
    assert_eq!(compact_lines, normal_lines.div_ceil(2));
    let art = String::from_utf8_lossy(&compact.stdout);
    assert!(art.contains('\u{2580}'), "half-block characters expected");
}

#[test]
fn text_dash_reads_data_from_stdin() {
    let piped = run_with_stdin(&["text", "-"], b"https://printemps.tokyo\n");
    assert!(piped.status.success());

    // The trailing newline is trimmed, so the output must match the same
    // data passed directly as an argument.
    let direct = qrgen()
        .args(["text", "https://printemps.tokyo"])
        .output()
        .expect("failed to run qrgen");
    assert!(direct.status.success());
    assert_eq!(piped.stdout, direct.stdout);
}

#[test]
fn text_dash_rejects_empty_stdin() {
    let out = run_with_stdin(&["text", "-"], b"");
    assert!(!out.status.success(), "empty stdin should be an error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("stdin"), "stderr: {stderr}");
}

#[test]
fn text_dash_literal_requires_stdin_only_for_dash() {
    // A plain argument that merely contains a dash is not stdin.
    let out = qrgen()
        .args(["text", "not-a-dash"])
        .output()
        .expect("failed to run qrgen");
    assert!(out.status.success());
}
