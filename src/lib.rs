//! qrgen — generate QR codes locally as PNG, SVG, or terminal art.
//!
//! The encoder produces a simple boolean [`Matrix`] of modules; the renderers
//! turn that matrix into a PNG image, an SVG document, or ANSI terminal output.
//! Keeping the matrix separate from the renderers makes every step pure and
//! easy to unit test, and keeps the QR specifics in one place (the `qrcode`
//! crate). There is no network access: everything is computed locally.

use anyhow::{anyhow, bail, Context, Result};
use image::{Rgb, RgbImage};
use qrcode::{Color, EcLevel, QrCode};

/// A square grid of QR modules. `dark[y * width + x]` is true for a dark module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub width: usize,
    pub dark: Vec<bool>,
}

impl Matrix {
    /// Whether the module at column `x`, row `y` is dark.
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.width + x]
    }
}

/// The output formats qrgen can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutFormat {
    Png,
    Svg,
    Ansi,
}

/// Pick an output format from a file extension (`.png` / `.svg`).
pub fn format_from_path(path: &std::path::Path) -> Result<OutFormat> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok(OutFormat::Png),
        Some("svg") => Ok(OutFormat::Svg),
        Some(other) => Err(anyhow!(
            "unsupported output extension: .{other} (use .png or .svg)"
        )),
        None => Err(anyhow!("output path has no extension: {}", path.display())),
    }
}

/// Parse an error-correction level: `l`, `m`, `q`, or `h` (case-insensitive).
pub fn parse_ec(input: &str) -> Result<EcLevel> {
    match input.to_ascii_lowercase().as_str() {
        "l" => Ok(EcLevel::L),
        "m" => Ok(EcLevel::M),
        "q" => Ok(EcLevel::Q),
        "h" => Ok(EcLevel::H),
        other => Err(anyhow!(
            "invalid error-correction level {other:?} (use l/m/q/h)"
        )),
    }
}

/// Parse a hex color ("#aabbcc", "aabbcc", "#abc", "abc") into RGB bytes.
pub fn parse_rgb(input: &str) -> Result<[u8; 3]> {
    let h = input.trim().strip_prefix('#').unwrap_or(input.trim());
    let expanded = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => bail!("invalid hex color: {input:?}"),
    };
    let val =
        u32::from_str_radix(&expanded, 16).map_err(|_| anyhow!("invalid hex color: {input:?}"))?;
    Ok([
        ((val >> 16) & 0xff) as u8,
        ((val >> 8) & 0xff) as u8,
        (val & 0xff) as u8,
    ])
}

/// Encode `data` into a QR module matrix at the given error-correction level.
pub fn encode(data: &[u8], ec: EcLevel) -> Result<Matrix> {
    let code = QrCode::with_error_correction_level(data, ec).context("failed to build QR code")?;
    let width = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == Color::Dark)
        .collect();
    Ok(Matrix { width, dark })
}

/// Build a Wi-Fi join payload (`WIFI:...;;`). `auth` is `wpa`, `wep`, or
/// `nopass`. Special characters in the SSID/password are backslash-escaped.
pub fn build_wifi_payload(
    ssid: &str,
    password: Option<&str>,
    auth: &str,
    hidden: bool,
) -> Result<String> {
    let t = match auth.to_ascii_lowercase().as_str() {
        "wpa" | "wpa2" => "WPA",
        "wep" => "WEP",
        "nopass" | "none" => "nopass",
        other => bail!("invalid --auth {other:?} (use wpa, wep, or nopass)"),
    };
    let mut out = format!("WIFI:T:{};S:{};", t, escape_wifi(ssid));
    if t != "nopass" {
        let p = password.ok_or_else(|| anyhow!("--password is required unless --auth nopass"))?;
        out.push_str(&format!("P:{};", escape_wifi(p)));
    }
    if hidden {
        out.push_str("H:true;");
    }
    out.push(';');
    Ok(out)
}

/// Backslash-escape the characters reserved by the WIFI: URI scheme.
fn escape_wifi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Maximum width/height of a rendered PNG or SVG in pixels. Anything larger
/// would allocate gigabytes of pixel data for no practical benefit.
pub const MAX_OUTPUT_DIM: usize = 32_768;

/// Compute the total output dimension `(modules + 2 * margin) * scale` in
/// pixels, rejecting anything above [`MAX_OUTPUT_DIM`].
fn checked_output_dim(matrix: &Matrix, scale: usize, margin: usize) -> Result<usize> {
    let total = matrix.width + 2 * margin;
    let px = total
        .checked_mul(scale)
        .filter(|&px| px <= MAX_OUTPUT_DIM)
        .ok_or_else(|| {
            let n = total.saturating_mul(scale);
            anyhow!("output would be {n}x{n} px; reduce --scale or --margin (max {MAX_OUTPUT_DIM})")
        })?;
    Ok(px.max(1))
}

/// Render the matrix to an RGB PNG image buffer. `scale` is the pixel size of
/// each module (clamped to at least 1) and `margin` the quiet-zone width in
/// modules. Fails if the total output dimension exceeds [`MAX_OUTPUT_DIM`].
pub fn render_png(
    matrix: &Matrix,
    scale: usize,
    margin: usize,
    fg: [u8; 3],
    bg: [u8; 3],
) -> Result<RgbImage> {
    // Defensive guard: a zero scale would produce an empty, unscannable image.
    let scale = scale.max(1);
    let dim = checked_output_dim(matrix, scale, margin)? as u32;
    let mut img = RgbImage::from_pixel(dim, dim, Rgb(bg));
    for y in 0..matrix.width {
        for x in 0..matrix.width {
            if !matrix.is_dark(x, y) {
                continue;
            }
            let px0 = (x + margin) * scale;
            let py0 = (y + margin) * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    img.put_pixel((px0 + dx) as u32, (py0 + dy) as u32, Rgb(fg));
                }
            }
        }
    }
    Ok(img)
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Render the matrix to a standalone SVG document. Dark modules are emitted as
/// a single `<path>` over a background rectangle for a compact result. Fails
/// if the total output dimension exceeds [`MAX_OUTPUT_DIM`].
pub fn render_svg(
    matrix: &Matrix,
    scale: usize,
    margin: usize,
    fg: [u8; 3],
    bg: [u8; 3],
) -> Result<String> {
    // Defensive guard: a zero scale would collapse the document to 1x1 px.
    let scale = scale.max(1);
    let total = matrix.width + 2 * margin;
    let px = checked_output_dim(matrix, scale, margin)?;
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{px}\" height=\"{px}\" \
viewBox=\"0 0 {total} {total}\" shape-rendering=\"crispEdges\">",
    ));
    s.push_str(&format!(
        "<rect width=\"{total}\" height=\"{total}\" fill=\"{}\"/>",
        hex(bg)
    ));
    s.push_str(&format!("<path fill=\"{}\" d=\"", hex(fg)));
    for y in 0..matrix.width {
        for x in 0..matrix.width {
            if matrix.is_dark(x, y) {
                s.push_str(&format!("M{} {}h1v1h-1z", x + margin, y + margin));
            }
        }
    }
    s.push_str("\"/></svg>\n");
    Ok(s)
}

/// Render the matrix as ANSI terminal art using black/white background blocks,
/// so it scans regardless of the terminal's color theme. `margin` is the
/// quiet-zone width in modules.
pub fn render_ansi(matrix: &Matrix, margin: usize) -> String {
    let total = matrix.width + 2 * margin;
    let dark_cell = "\x1b[40m  ";
    let light_cell = "\x1b[47m  ";
    let mut s = String::new();
    for y in 0..total {
        for x in 0..total {
            let dark = x >= margin
                && y >= margin
                && x < margin + matrix.width
                && y < margin + matrix.width
                && matrix.is_dark(x - margin, y - margin);
            s.push_str(if dark { dark_cell } else { light_cell });
        }
        s.push_str("\x1b[0m\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 2x2 checkerboard: dark at (0,0) and (1,1).
    fn checker() -> Matrix {
        Matrix {
            width: 2,
            dark: vec![true, false, false, true],
        }
    }

    #[test]
    fn parse_ec_accepts_levels_and_rejects_others() {
        assert_eq!(parse_ec("L").unwrap(), EcLevel::L);
        assert_eq!(parse_ec("h").unwrap(), EcLevel::H);
        assert!(parse_ec("x").is_err());
    }

    #[test]
    fn parse_rgb_handles_3_and_6_digit() {
        assert_eq!(parse_rgb("#fff").unwrap(), [255, 255, 255]);
        assert_eq!(parse_rgb("ff8800").unwrap(), [255, 136, 0]);
        assert!(parse_rgb("12345").is_err());
    }

    #[test]
    fn format_from_path_picks_by_extension() {
        use std::path::Path;
        assert_eq!(
            format_from_path(Path::new("a.PNG")).unwrap(),
            OutFormat::Png
        );
        assert_eq!(
            format_from_path(Path::new("a.svg")).unwrap(),
            OutFormat::Svg
        );
        assert!(format_from_path(Path::new("a.gif")).is_err());
    }

    #[test]
    fn wifi_payload_wpa_with_escaping() {
        let p = build_wifi_payload("My;Net", Some("p:a\\ss"), "wpa", false).unwrap();
        assert_eq!(p, "WIFI:T:WPA;S:My\\;Net;P:p\\:a\\\\ss;;");
    }

    #[test]
    fn wifi_payload_nopass_omits_password_and_hidden_flag() {
        let open = build_wifi_payload("cafe", None, "nopass", false).unwrap();
        assert_eq!(open, "WIFI:T:nopass;S:cafe;;");
        let hidden = build_wifi_payload("cafe", Some("pw"), "wpa", true).unwrap();
        assert_eq!(hidden, "WIFI:T:WPA;S:cafe;P:pw;H:true;;");
    }

    #[test]
    fn wifi_payload_requires_password_unless_nopass() {
        assert!(build_wifi_payload("net", None, "wpa", false).is_err());
        assert!(build_wifi_payload("net", None, "telnet", false).is_err());
    }

    #[test]
    fn encode_produces_a_square_matrix_with_a_finder_pattern() {
        let m = encode(b"https://printemps.tokyo", EcLevel::M).unwrap();
        assert!(m.width >= 21 && m.width % 2 == 1);
        assert_eq!(m.dark.len(), m.width * m.width);
        // The top-left finder pattern's outer ring is dark.
        assert!(m.is_dark(0, 0));
        assert!(
            m.dark.iter().any(|&d| !d),
            "should contain light modules too"
        );
    }

    #[test]
    fn render_png_sizes_and_colors_modules() {
        let img = render_png(&checker(), 3, 1, [0, 0, 0], [255, 255, 255]).unwrap();
        // total modules = 2 + 2*1 = 4, each 3px -> 12px square.
        assert_eq!(img.width(), 12);
        assert_eq!(img.height(), 12);
        // Module (0,0) is dark -> its pixel block starts at (margin*scale)=(3,3).
        assert_eq!(*img.get_pixel(3, 3), Rgb([0, 0, 0]));
        // The quiet zone (top-left corner) stays background.
        assert_eq!(*img.get_pixel(0, 0), Rgb([255, 255, 255]));
    }

    #[test]
    fn render_guards_clamp_zero_scale_to_one() {
        let img = render_png(&checker(), 0, 0, [0, 0, 0], [255, 255, 255]).unwrap();
        // scale 0 is clamped to 1: 2 modules with no margin -> 2x2 px.
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
        assert_eq!(*img.get_pixel(0, 0), Rgb([0, 0, 0]));

        let svg = render_svg(&checker(), 0, 0, [0, 0, 0], [255, 255, 255]).unwrap();
        assert!(svg.contains("width=\"2\" height=\"2\""));
    }

    #[test]
    fn render_rejects_output_over_the_dimension_cap() {
        // A version-40 QR (177 modules) at the CLI maxima would be
        // (177 + 2*64) * 256 = 78080 px square (~18 GB of RGB): must error
        // instead of allocating.
        let m = Matrix {
            width: 177,
            dark: vec![false; 177 * 177],
        };
        let err = render_png(&m, 256, 64, [0, 0, 0], [255, 255, 255]).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("78080x78080"), "message: {msg}");
        assert!(msg.contains("32768"), "message: {msg}");
        assert!(render_svg(&m, 256, 64, [0, 0, 0], [255, 255, 255]).is_err());
    }

    #[test]
    fn render_allows_output_up_to_the_dimension_cap() {
        // Exactly at the cap: 2 modules * 16384 px = 32768. The SVG document
        // stays tiny, so the boundary is exercised without a huge allocation.
        let svg = render_svg(&checker(), 16384, 0, [0, 0, 0], [255, 255, 255]).unwrap();
        assert!(svg.contains("width=\"32768\" height=\"32768\""));
        // One pixel step past the cap fails.
        assert!(render_svg(&checker(), 16385, 0, [0, 0, 0], [255, 255, 255]).is_err());
        // A large but sane PNG still renders: (2 + 2*4) * 256 = 2560 px.
        let img = render_png(&checker(), 256, 4, [0, 0, 0], [255, 255, 255]).unwrap();
        assert_eq!(img.width(), 2560);
    }

    #[test]
    fn render_svg_has_viewbox_and_one_path_per_dark_module() {
        let svg = render_svg(&checker(), 8, 1, [0, 0, 0], [255, 255, 255]).unwrap();
        assert!(svg.contains("viewBox=\"0 0 4 4\""));
        assert!(svg.contains("width=\"32\"")); // 4 modules * 8px
        assert!(svg.contains("M1 1h1v1h-1z")); // dark (0,0) offset by margin 1
        assert!(svg.contains("M2 2h1v1h-1z")); // dark (1,1) offset by margin 1
        assert!(svg.contains("fill=\"#000000\""));
    }

    #[test]
    fn render_ansi_emits_one_line_per_row_with_color_codes() {
        let art = render_ansi(&checker(), 1);
        let lines: Vec<&str> = art.lines().collect();
        assert_eq!(lines.len(), 4); // 2 + 2*1
        assert!(art.contains("\x1b[40m")); // dark cells present
        assert!(art.contains("\x1b[47m")); // light cells present
    }
}
