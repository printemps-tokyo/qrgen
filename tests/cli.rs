//! Integration tests that exercise the compiled `qrgen` binary.

use std::process::Command;

fn qrgen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_qrgen"))
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
