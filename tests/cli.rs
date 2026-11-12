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
fn rejects_compact_combined_with_output_file() {
    let dir = std::env::temp_dir().join("qrgen-cli-test-compact");
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let path = dir.join("qr.png");
    std::fs::remove_file(&path).ok();
    let out = qrgen()
        .args(["text", "hi", "--compact", "--output"])
        .arg(&path)
        .output()
        .expect("failed to run qrgen");
    assert!(
        !out.status.success(),
        "--compact with --output should be rejected"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--compact applies to terminal output only"),
        "stderr: {stderr}"
    );
    assert!(!path.exists(), "no file should be written");
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

/// Decode the single QR code in a PNG file with an independent reader.
fn decode_png(path: &std::path::Path) -> String {
    let img = image::open(path).expect("readable PNG").to_luma8();
    let (w, h) = img.dimensions();
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(w as usize, h as usize, |x, y| {
            img.get_pixel(x as u32, y as u32)[0]
        });
    let grids = prepared.detect_grids();
    assert_eq!(grids.len(), 1, "expected one code in {}", path.display());
    let (_, content) = grids[0].decode().expect("the code decodes");
    content
}

/// A 64x64 red circle on transparency, as a logo.
fn write_logo(path: &std::path::Path) {
    let img = image::RgbaImage::from_fn(64, 64, |x, y| {
        let (dx, dy) = (x as i32 - 32, y as i32 - 32);
        if dx * dx + dy * dy < 30 * 30 {
            image::Rgba([220, 30, 30, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    img.save(path).unwrap();
}

#[test]
fn a_code_with_a_logo_still_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("logo.png");
    write_logo(&logo);
    let url = "https://printemps-tokyo.github.io/";
    for size in ["20", "30"] {
        let out = dir.path().join(format!("logo-{size}.png"));
        let o = qrgen()
            .args(["text", url, "--logo"])
            .arg(&logo)
            .args(["--logo-size", size, "-o"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(decode_png(&out), url, "logo size {size}%");
        // The logo's red is in the middle of the image.
        let img = image::open(&out).unwrap().to_rgb8();
        let c = img.width() / 2;
        assert_eq!(img.get_pixel(c, c).0, [220, 30, 30]);
    }
    // SVG gets the logo as an embedded PNG.
    let svg = dir.path().join("logo.svg");
    assert!(qrgen()
        .args(["text", url, "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&svg)
        .status()
        .unwrap()
        .success());
    let text = std::fs::read_to_string(&svg).unwrap();
    assert!(text.contains("<image ") && text.contains("href=\"data:image/png;base64,iVBORw0KGgo"));
}

#[test]
fn a_logo_needs_enough_error_correction_and_an_image_output() {
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("logo.png");
    write_logo(&logo);
    let out = dir.path().join("x.png");
    let o = qrgen()
        .args(["text", "hi", "--ec", "m", "--logo"])
        .arg(&logo)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--ec q or h"));
    let o = qrgen()
        .args(["text", "hi", "--logo"])
        .arg(&logo)
        .output()
        .unwrap();
    assert!(!o.status.success());
    let o = qrgen()
        .args(["text", "hi", "--logo-size", "40", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(!o.status.success());
}

#[test]
fn vcard_and_event_codes_carry_their_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("card.png");
    let o = qrgen()
        .args([
            "vcard",
            "--name",
            "山田 太郎",
            "--family",
            "山田",
            "--given",
            "太郎",
            "--org",
            "Printemps, Inc.",
            "--tel",
            "+81-3-0000-0000",
            "--email",
            "taro@example.com",
            "-o",
        ])
        .arg(&card)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = decode_png(&card);
    assert!(
        text.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\nN:山田;太郎;;;\r\nFN:山田 太郎\r\n"),
        "{text:?}"
    );
    assert!(text.contains("ORG:Printemps\\, Inc.\r\n"));
    assert!(text.contains("EMAIL;TYPE=INTERNET:taro@example.com\r\n"));

    let ev = dir.path().join("event.png");
    let o = qrgen()
        .args([
            "event",
            "--summary",
            "Release party",
            "--start",
            "2026-12-24 19:00",
            "--location",
            "Tokyo",
            "-o",
        ])
        .arg(&ev)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        decode_png(&ev),
        "BEGIN:VEVENT\r\nSUMMARY:Release party\r\nDTSTART:20261224T190000\r\nDTEND:20261224T200000\r\nLOCATION:Tokyo\r\nEND:VEVENT\r\n"
    );
}

#[test]
fn batch_writes_one_image_per_row_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("codes.csv");
    std::fs::write(
        &csv,
        "file,data\r\nhome,https://example.com/\r\n,\"a, quoted \"\"value\"\"\"\r\n",
    )
    .unwrap();
    let out = dir.path().join("out");
    let o = qrgen()
        .arg("batch")
        .arg(&csv)
        .arg("--out-dir")
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(decode_png(&out.join("home.png")), "https://example.com/");
    assert_eq!(decode_png(&out.join("002.png")), "a, quoted \"value\"");

    let again = qrgen()
        .arg("batch")
        .arg(&csv)
        .arg("--out-dir")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("--force"));
    assert!(qrgen()
        .arg("batch")
        .arg(&csv)
        .arg("--out-dir")
        .arg(&out)
        .arg("--force")
        .status()
        .unwrap()
        .success());

    // An unsafe name stops the run before anything is written.
    std::fs::write(&csv, "file,data\nok,1\n../escape,2\n").unwrap();
    let fresh = dir.path().join("fresh");
    let o = qrgen()
        .arg("batch")
        .arg(&csv)
        .arg("--out-dir")
        .arg(&fresh)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(!fresh.exists());
}
