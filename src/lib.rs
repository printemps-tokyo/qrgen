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

/// Contact details for a vCard 3.0 payload, the form zxing documents for QR
/// codes and phones' cameras offer to add to contacts.
#[derive(Debug, Clone, Default)]
pub struct VCard {
    /// Display name (FN).
    pub name: String,
    /// Family and given names (N); without them the display name is used as
    /// the family name, since the order of names differs by language.
    pub family: Option<String>,
    pub given: Option<String>,
    pub org: Option<String>,
    pub title: Option<String>,
    pub tel: Vec<String>,
    pub email: Vec<String>,
    pub url: Option<String>,
    /// Street address, as one line (the street part of ADR).
    pub address: Option<String>,
    pub note: Option<String>,
}

/// Escape a vCard / iCalendar text value: backslash, semicolon, comma and
/// line breaks (RFC 2426 section 4, RFC 5545 section 3.3.11).
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// Build a vCard 3.0 payload, lines ending in CRLF as the format requires.
pub fn build_vcard(v: &VCard) -> Result<String> {
    if v.name.trim().is_empty() {
        bail!("--name must not be empty");
    }
    let mut lines = vec!["BEGIN:VCARD".to_string(), "VERSION:3.0".to_string()];
    let family = v.family.as_deref().unwrap_or(&v.name);
    let given = v.given.as_deref().unwrap_or("");
    lines.push(format!(
        "N:{};{};;;",
        escape_text(family),
        escape_text(given)
    ));
    lines.push(format!("FN:{}", escape_text(&v.name)));
    if let Some(org) = &v.org {
        lines.push(format!("ORG:{}", escape_text(org)));
    }
    if let Some(title) = &v.title {
        lines.push(format!("TITLE:{}", escape_text(title)));
    }
    for tel in &v.tel {
        lines.push(format!("TEL:{}", escape_text(tel)));
    }
    for email in &v.email {
        lines.push(format!("EMAIL;TYPE=INTERNET:{}", escape_text(email)));
    }
    if let Some(url) = &v.url {
        // URL is a URI value, not text: it is not escaped.
        lines.push(format!("URL:{url}"));
    }
    if let Some(adr) = &v.address {
        lines.push(format!("ADR:;;{};;;;", escape_text(adr)));
    }
    if let Some(note) = &v.note {
        lines.push(format!("NOTE:{}", escape_text(note)));
    }
    lines.push("END:VCARD".to_string());
    Ok(lines.join("\r\n") + "\r\n")
}

/// A calendar event for a bare VEVENT payload, the form zxing documents.
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub summary: String,
    /// `YYYY-MM-DD` (all day), or `YYYY-MM-DD HH:MM[:SS]` / `...THH:MM[:SS]`,
    /// local time, or UTC with a trailing `Z`.
    pub start: String,
    /// Same forms as `start`; defaults to the next day (all day) or one hour
    /// later.
    pub end: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
}

/// A parsed event time: a date, or a date and time, local or UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventTime {
    /// Days since 1970-01-01.
    days: i64,
    /// Seconds into the day; None for an all-day date.
    secs: Option<u32>,
    utc: bool,
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Parse an event time; see [`Event::start`] for the accepted forms.
pub fn parse_event_time(input: &str) -> Result<EventTime> {
    let s = input.trim();
    let (s, utc) = match s.strip_suffix('Z').or_else(|| s.strip_suffix('z')) {
        Some(rest) => (rest, true),
        None => (s, false),
    };
    let bad = || anyhow!("invalid time {input:?} (use YYYY-MM-DD or YYYY-MM-DD HH:MM, Z for UTC)");
    let (date, time) = match s.find([' ', 'T', 't']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let parts: Vec<&str> = date.split('-').collect();
    let [y, m, d] = parts.as_slice() else {
        return Err(bad());
    };
    if y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return Err(bad());
    }
    let (y, m, d): (i64, u32, u32) = (
        y.parse().map_err(|_| bad())?,
        m.parse().map_err(|_| bad())?,
        d.parse().map_err(|_| bad())?,
    );
    let days = days_from_civil(y, m, d);
    // Reject dates such as 2026-02-30 that would roll over.
    if !(1..=12).contains(&m) || civil_from_days(days) != (y, m, d) {
        return Err(bad());
    }
    let secs = match time {
        None => {
            if utc {
                return Err(anyhow!("an all-day date takes no Z: {input:?}"));
            }
            None
        }
        Some(t) => {
            let f: Vec<&str> = t.split(':').collect();
            if !(2..=3).contains(&f.len()) || f.iter().any(|x| x.len() != 2) {
                return Err(bad());
            }
            let n: Vec<u32> = f
                .iter()
                .map(|x| x.parse().map_err(|_| bad()))
                .collect::<Result<_>>()?;
            let (h, mi, se) = (n[0], n[1], n.get(2).copied().unwrap_or(0));
            if h > 23 || mi > 59 || se > 59 {
                return Err(bad());
            }
            Some(h * 3600 + mi * 60 + se)
        }
    };
    Ok(EventTime { days, secs, utc })
}

impl EventTime {
    fn property(&self, name: &str) -> String {
        let (y, m, d) = civil_from_days(self.days);
        match self.secs {
            None => format!("{name};VALUE=DATE:{y:04}{m:02}{d:02}"),
            Some(t) => format!(
                "{name}:{y:04}{m:02}{d:02}T{:02}{:02}{:02}{}",
                t / 3600,
                t / 60 % 60,
                t % 60,
                if self.utc { "Z" } else { "" }
            ),
        }
    }

    /// The default end: the next day for a date, one hour later for a time.
    fn default_end(&self) -> EventTime {
        match self.secs {
            None => EventTime {
                days: self.days + 1,
                ..*self
            },
            Some(t) => {
                let t = t + 3600;
                EventTime {
                    days: self.days + i64::from(t / 86_400),
                    secs: Some(t % 86_400),
                    utc: self.utc,
                }
            }
        }
    }
}

/// Build a VEVENT payload (lines ending in CRLF). Times without `Z` are
/// floating local times, read in the scanning phone's time zone.
pub fn build_event(e: &Event) -> Result<String> {
    if e.summary.trim().is_empty() {
        bail!("--summary must not be empty");
    }
    let start = parse_event_time(&e.start)?;
    let end = match &e.end {
        Some(s) => parse_event_time(s)?,
        None => start.default_end(),
    };
    if start.secs.is_some() != end.secs.is_some() || start.utc != end.utc {
        bail!("--start and --end must be the same kind of time (both dates, both local times, or both UTC)");
    }
    if end <= start {
        bail!("--end must be after --start");
    }
    let mut lines = vec![
        "BEGIN:VEVENT".to_string(),
        format!("SUMMARY:{}", escape_text(&e.summary)),
        start.property("DTSTART"),
        end.property("DTEND"),
    ];
    if let Some(loc) = &e.location {
        lines.push(format!("LOCATION:{}", escape_text(loc)));
    }
    if let Some(desc) = &e.description {
        lines.push(format!("DESCRIPTION:{}", escape_text(desc)));
    }
    lines.push("END:VEVENT".to_string());
    Ok(lines.join("\r\n") + "\r\n")
}

/// The square of modules a logo covers, padding included, in module units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoBox {
    pub x: usize,
    pub size: usize,
}

/// Smallest and largest logo side as a fraction of the code's width. Above
/// 0.3 the hidden area (about 9% of the modules) eats into what level H can
/// recover (about 30% of the codewords) too far to be safe.
pub const LOGO_FRACTION_RANGE: std::ops::RangeInclusive<f64> = 0.05..=0.3;

/// The centred box a logo of `fraction` of the width covers. Its side has the
/// width's parity, so it sits exactly in the middle.
pub fn logo_box(width: usize, fraction: f64) -> Result<LogoBox> {
    if !LOGO_FRACTION_RANGE.contains(&fraction) {
        bail!(
            "logo size must be between 5% and 30% of the code (got {:.0}%)",
            fraction * 100.0
        );
    }
    let mut size = ((width as f64) * fraction).round() as usize;
    if size % 2 != width % 2 {
        size += 1;
    }
    let size = size.max(3).min(width);
    Ok(LogoBox {
        x: (width - size) / 2,
        size,
    })
}

/// Turn every module under the logo box light, so the code is drawn around it.
pub fn clear_logo_area(matrix: &mut Matrix, b: LogoBox) {
    for y in b.x..b.x + b.size {
        for x in b.x..b.x + b.size {
            matrix.dark[y * matrix.width + x] = false;
        }
    }
}

/// Draw `logo` into the box (one module of padding on each side), fitted
/// without stretching and blended over the background by its alpha.
pub fn draw_logo_png(
    img: &mut RgbImage,
    logo: &image::DynamicImage,
    b: LogoBox,
    scale: usize,
    margin: usize,
) {
    let scale = scale.max(1);
    let inner = (b.size.saturating_sub(2)).max(1) * scale;
    let rgba = logo.to_rgba8();
    let (lw, lh) = rgba.dimensions();
    let ratio = (inner as f64 / lw.max(1) as f64).min(inner as f64 / lh.max(1) as f64);
    let (w, h) = (
        ((lw as f64 * ratio).round() as u32).max(1),
        ((lh as f64 * ratio).round() as u32).max(1),
    );
    let fitted = image::imageops::resize(&rgba, w, h, image::imageops::FilterType::Lanczos3);
    let origin = (b.x + 1 + margin) * scale;
    let ox = origin as u32 + (inner as u32 - w) / 2;
    let oy = origin as u32 + (inner as u32 - h) / 2;
    for (x, y, p) in fitted.enumerate_pixels() {
        let a = u32::from(p[3]);
        let dst = img.get_pixel_mut(ox + x, oy + y);
        for c in 0..3 {
            dst[c] = ((u32::from(p[c]) * a + u32::from(dst[c]) * (255 - a)) / 255) as u8;
        }
    }
}

/// Standard base64 (RFC 4648) with padding, for the SVG data URI.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The SVG element for a PNG logo in the box (inside the padding).
pub fn svg_logo_element(png: &[u8], b: LogoBox, margin: usize) -> String {
    let inner = b.size.saturating_sub(2).max(1);
    format!(
        "<image x=\"{x}\" y=\"{x}\" width=\"{inner}\" height=\"{inner}\" preserveAspectRatio=\"xMidYMid meet\" href=\"data:image/png;base64,{}\"/>",
        base64(png),
        x = b.x + 1 + margin,
    )
}

/// Parse CSV text (RFC 4180: quoted fields, doubled quotes, CRLF or LF) into
/// rows of fields. Blank lines are skipped.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut line = 1usize;
    let mut started = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            '"' => bail!("CSV line {line}: a quote inside an unquoted field"),
            ',' => {
                row.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                line += 1;
                if started || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                started = false;
            }
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if quoted {
        bail!("CSV: a quoted field is not closed");
    }
    if started || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// Whether `name` is safe as a file name stem in the output folder: no path
/// separators, no leading dot, nothing a shell or file system treats
/// specially.
pub fn safe_file_stem(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        && name.trim() == name
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

/// Render the matrix as compact ANSI terminal art, packing two vertical
/// modules into each text row with the upper-half-block character (U+2580).
/// The foreground color paints the top module and the background color the
/// bottom one, using the same black/white palette as [`render_ansi`] so the
/// code scans regardless of the terminal's color theme. When the total height
/// is odd, the missing bottom half of the last row is treated as light.
/// `margin` is the quiet-zone width in modules.
pub fn render_ansi_compact(matrix: &Matrix, margin: usize) -> String {
    let total = matrix.width + 2 * margin;
    let module_dark = |x: usize, y: usize| {
        x >= margin
            && y >= margin
            && x < margin + matrix.width
            && y < margin + matrix.width
            && matrix.is_dark(x - margin, y - margin)
    };
    let mut s = String::new();
    for y in (0..total).step_by(2) {
        for x in 0..total {
            let top = module_dark(x, y);
            // Past the bottom edge (odd total) counts as light.
            let bottom = y + 1 < total && module_dark(x, y + 1);
            s.push_str(match (top, bottom) {
                (true, true) => "\x1b[30;40m",
                (true, false) => "\x1b[30;47m",
                (false, true) => "\x1b[37;40m",
                (false, false) => "\x1b[37;47m",
            });
            s.push('\u{2580}');
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
    fn render_ansi_compact_packs_two_rows_per_line() {
        // 2x2 checkerboard, no margin: one line pairing rows 0 and 1.
        // Column 0 is dark-over-light, column 1 light-over-dark.
        let art = render_ansi_compact(&checker(), 0);
        assert_eq!(art, "\x1b[30;47m\u{2580}\x1b[37;40m\u{2580}\x1b[0m\n");
    }

    #[test]
    fn render_ansi_compact_covers_all_combinations_and_odd_height() {
        // 3x3 pattern chosen so the first line hits dark/dark, dark/light and
        // light/dark, and the odd last row pairs with an implicit light row.
        let m = Matrix {
            width: 3,
            dark: vec![
                true, true, false, // row 0
                true, false, true, // row 1
                false, true, false, // row 2
            ],
        };
        let art = render_ansi_compact(&m, 0);
        assert_eq!(
            art,
            concat!(
                "\x1b[30;40m\u{2580}\x1b[30;47m\u{2580}\x1b[37;40m\u{2580}\x1b[0m\n",
                "\x1b[37;47m\u{2580}\x1b[30;47m\u{2580}\x1b[37;47m\u{2580}\x1b[0m\n",
            )
        );
    }

    #[test]
    fn render_ansi_compact_margin_stays_light() {
        // Margin 1 around the checkerboard: total 4 -> 2 lines, and the
        // outer quiet zone renders as light-over-light cells.
        let art = render_ansi_compact(&checker(), 1);
        let lines: Vec<&str> = art.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("\x1b[37;47m\u{2580}")); // top-left corner
        assert!(lines[1].ends_with("\x1b[37;47m\u{2580}\x1b[0m")); // bottom-right corner
    }

    #[test]
    fn render_ansi_compact_halves_the_rows_of_a_real_qr() {
        let m = encode(b"https://printemps.tokyo", EcLevel::M).unwrap();
        let margin = 4;
        let total = m.width + 2 * margin;
        let normal_lines = render_ansi(&m, margin).lines().count();
        let compact = render_ansi_compact(&m, margin);
        let compact_lines: Vec<&str> = compact.lines().collect();
        assert_eq!(normal_lines, total);
        assert_eq!(compact_lines.len(), total.div_ceil(2));
        // Every line carries one half-block character per module column.
        for line in &compact_lines {
            assert_eq!(line.matches('\u{2580}').count(), total);
        }
    }

    #[test]
    fn render_ansi_emits_one_line_per_row_with_color_codes() {
        let art = render_ansi(&checker(), 1);
        let lines: Vec<&str> = art.lines().collect();
        assert_eq!(lines.len(), 4); // 2 + 2*1
        assert!(art.contains("\x1b[40m")); // dark cells present
        assert!(art.contains("\x1b[47m")); // light cells present
    }

    #[test]
    fn vcard_escapes_text_and_keeps_urls() {
        let v = VCard {
            name: "A; B".into(),
            note: Some("line1\nline2, ok\\".into()),
            url: Some("https://example.com/a,b".into()),
            ..VCard::default()
        };
        let card = build_vcard(&v).unwrap();
        assert!(card.contains("N:A\\; B;;;;\r\n"));
        assert!(card.contains("FN:A\\; B\r\n"));
        assert!(card.contains("NOTE:line1\\nline2\\, ok\\\\\r\n"));
        assert!(card.contains("URL:https://example.com/a,b\r\n"));
        assert!(build_vcard(&VCard::default()).is_err());
    }

    #[test]
    fn event_times_and_defaults() {
        let ev = |start: &str, end: Option<&str>| {
            build_event(&Event {
                summary: "x".into(),
                start: start.into(),
                end: end.map(Into::into),
                ..Event::default()
            })
        };
        assert!(ev("2026-12-31", None)
            .unwrap()
            .contains("DTSTART;VALUE=DATE:20261231\r\nDTEND;VALUE=DATE:20270101\r\n"));
        assert!(ev("2026-12-31T23:30Z", None)
            .unwrap()
            .contains("DTSTART:20261231T233000Z\r\nDTEND:20270101T003000Z\r\n"));
        assert!(ev("2028-02-28 10:00:15", Some("2028-02-29 09:00"))
            .unwrap()
            .contains("DTEND:20280229T090000\r\n"));
        assert!(ev("2026-02-30", None).is_err());
        assert!(ev("2026-12-01 24:00", None).is_err());
        assert!(ev("2026-12-01 10:00", Some("2026-12-01 09:00")).is_err());
        assert!(ev("2026-12-01 10:00", Some("2026-12-02")).is_err());
        assert!(ev("2026-12-01 10:00Z", Some("2026-12-01 11:00")).is_err());
        assert!(ev("2026-12-01Z", None).is_err());
    }

    #[test]
    fn csv_quotes_and_line_endings() {
        let rows = parse_csv("\u{feff}a,b\r\n\"x, y\",\"he said \"\"hi\"\"\"\n\n\"multi\nline\",z")
            .unwrap();
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["x, y".to_string(), "he said \"hi\"".to_string()],
                vec!["multi\nline".to_string(), "z".to_string()],
            ]
        );
        assert!(parse_csv("a,\"open").is_err());
        assert!(parse_csv("a\"b").is_err());
    }

    #[test]
    fn safe_file_stems() {
        for ok in ["home", "card-01", "名刺 山田", "v1.2"] {
            assert!(safe_file_stem(ok), "{ok}");
        }
        for bad in ["", ".hidden", "a/b", "..\\x", " lead", "a:b"] {
            assert!(!safe_file_stem(bad), "{bad}");
        }
    }

    #[test]
    fn logo_box_is_centred_and_bounded() {
        let b = logo_box(25, 0.2).unwrap();
        assert_eq!(b, LogoBox { x: 10, size: 5 });
        assert_eq!(b.x * 2 + b.size, 25);
        assert!(logo_box(25, 0.31).is_err());
        let mut m = encode(b"hello", EcLevel::H).unwrap();
        let b = logo_box(m.width, 0.3).unwrap();
        clear_logo_area(&mut m, b);
        assert!(!m.is_dark(b.x, b.x) && !m.is_dark(b.x + b.size - 1, b.x + b.size - 1));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }
}
