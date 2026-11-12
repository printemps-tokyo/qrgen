//! Command-line entry point for qrgen.

use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};

use qrgen::{
    build_event, build_vcard, build_wifi_payload, clear_logo_area, draw_logo_png, encode,
    format_from_path, logo_box, parse_csv, parse_ec, parse_rgb, render_ansi, render_ansi_compact,
    render_png, render_svg, safe_file_stem, svg_logo_element, Event, OutFormat, VCard,
};

/// Generate QR codes locally as PNG, SVG, or terminal art.
#[derive(Parser, Debug)]
#[command(name = "qrgen", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Encode arbitrary text or a URL.
    Text {
        /// The text or URL to encode. Pass "-" to read it from stdin
        /// (keeps secrets out of your shell history).
        data: String,
        #[command(flatten)]
        out: OutputOpts,
    },
    /// Encode Wi-Fi credentials (phones offer to join the network on scan).
    Wifi {
        /// Network name (SSID).
        #[arg(long)]
        ssid: String,
        /// Password (required unless --auth nopass).
        #[arg(long)]
        password: Option<String>,
        /// Authentication type: wpa, wep, or nopass.
        #[arg(long, default_value = "wpa")]
        auth: String,
        /// Mark the network as hidden.
        #[arg(long)]
        hidden: bool,
        #[command(flatten)]
        out: OutputOpts,
    },
    /// Encode a contact card (vCard 3.0; phones offer to add the contact).
    Vcard {
        /// Display name.
        #[arg(long)]
        name: String,
        /// Family name (with --given; otherwise the display name is used).
        #[arg(long)]
        family: Option<String>,
        /// Given name.
        #[arg(long)]
        given: Option<String>,
        /// Organization.
        #[arg(long)]
        org: Option<String>,
        /// Job title.
        #[arg(long)]
        title: Option<String>,
        /// Phone number (repeatable).
        #[arg(long)]
        tel: Vec<String>,
        /// E-mail address (repeatable).
        #[arg(long)]
        email: Vec<String>,
        /// Web site.
        #[arg(long)]
        url: Option<String>,
        /// Street address, as one line.
        #[arg(long)]
        address: Option<String>,
        /// Free-form note.
        #[arg(long)]
        note: Option<String>,
        #[command(flatten)]
        out: OutputOpts,
    },
    /// Encode a calendar event (phones offer to add it to the calendar).
    Event {
        /// Title of the event.
        #[arg(long)]
        summary: String,
        /// Start: YYYY-MM-DD (all day) or "YYYY-MM-DD HH:MM" (local time; add
        /// Z for UTC).
        #[arg(long)]
        start: String,
        /// End, in the same form (default: the next day, or one hour later).
        #[arg(long)]
        end: Option<String>,
        /// Place.
        #[arg(long)]
        location: Option<String>,
        /// Description.
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        out: OutputOpts,
    },
    /// Make one image per row of a CSV file (columns: data, and optionally file).
    Batch {
        /// CSV file with a header row; "-" reads stdin. The "data" column is
        /// encoded; "file" names the image (default: the row number).
        csv: PathBuf,
        /// Folder to write the images to (created if missing).
        #[arg(long)]
        out_dir: PathBuf,
        /// Image format.
        #[arg(long, value_parser = ["png", "svg"], default_value = "png")]
        format: String,
        /// Replace images that already exist (default: refuse).
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        style: StyleOpts,
    },
}

/// Where to write a single code, and terminal options.
#[derive(Args, Debug)]
struct OutputOpts {
    /// Output file; its extension (.png/.svg) picks the format. Omit to print
    /// the QR to the terminal.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Terminal output only: pack two modules per text row using half-block
    /// characters, halving the height so large codes fit small terminals.
    #[arg(long)]
    compact: bool,

    #[command(flatten)]
    style: StyleOpts,
}

/// How a code looks, shared by every subcommand.
#[derive(Args, Debug)]
struct StyleOpts {
    /// Error-correction level: l, m, q, or h (higher = more redundancy;
    /// default m, or h with --logo).
    #[arg(long)]
    ec: Option<String>,

    /// Module size in pixels for PNG / SVG output (1-256).
    #[arg(long, default_value_t = 8, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=256))]
    scale: usize,

    /// Quiet-zone (margin) width in modules (0-64).
    #[arg(long, default_value_t = 4, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(0..=64))]
    margin: usize,

    /// Foreground (dark module) color as hex.
    #[arg(long, default_value = "000000")]
    fg: String,

    /// Background color as hex.
    #[arg(long, default_value = "ffffff")]
    bg: String,

    /// A PNG image to put in the middle of the code (PNG / SVG output only).
    #[arg(long, value_name = "PNG")]
    logo: Option<PathBuf>,

    /// Logo width as a percentage of the code's width (5-30).
    #[arg(long, default_value_t = 20, value_parser = clap::builder::RangedU64ValueParser::<u8>::new().range(5..=30))]
    logo_size: u8,
}

/// A loaded logo: decoded for PNG output, raw bytes for the SVG data URI.
struct Logo {
    image: image::DynamicImage,
    png: Vec<u8>,
}

/// Everything needed to draw codes, resolved once from StyleOpts.
struct Style {
    ec: qrcode::EcLevel,
    scale: usize,
    margin: usize,
    fg: [u8; 3],
    bg: [u8; 3],
    logo: Option<Logo>,
    logo_fraction: f64,
}

impl StyleOpts {
    fn resolve(&self) -> Result<Style> {
        let logo = match &self.logo {
            None => None,
            Some(path) => {
                let png = std::fs::read(path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                    .with_context(|| format!("{} is not a PNG image", path.display()))?;
                Some(Logo { image, png })
            }
        };
        let ec = match (&self.ec, logo.is_some()) {
            (Some(e), has_logo) => {
                let level = parse_ec(e)?;
                if has_logo && matches!(level, qrcode::EcLevel::L | qrcode::EcLevel::M) {
                    bail!(
                        "a logo hides part of the code; use --ec q or h (or leave --ec out for h)"
                    );
                }
                level
            }
            (None, true) => qrcode::EcLevel::H,
            (None, false) => qrcode::EcLevel::M,
        };
        Ok(Style {
            ec,
            scale: self.scale,
            margin: self.margin,
            fg: parse_rgb(&self.fg)?,
            bg: parse_rgb(&self.bg)?,
            logo,
            logo_fraction: f64::from(self.logo_size) / 100.0,
        })
    }
}

impl Style {
    /// Render `data` as a PNG or SVG file's bytes.
    fn render(&self, data: &str, format: OutFormat) -> Result<Vec<u8>> {
        let mut matrix = encode(data.as_bytes(), self.ec)?;
        let logo = match &self.logo {
            Some(l) => {
                let b = logo_box(matrix.width, self.logo_fraction)?;
                clear_logo_area(&mut matrix, b);
                Some((l, b))
            }
            None => None,
        };
        match format {
            OutFormat::Png => {
                let mut img = render_png(&matrix, self.scale, self.margin, self.fg, self.bg)?;
                if let Some((l, b)) = logo {
                    draw_logo_png(&mut img, &l.image, b, self.scale, self.margin);
                }
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, image::ImageFormat::Png)?;
                Ok(buf.into_inner())
            }
            OutFormat::Svg => {
                let mut svg = render_svg(&matrix, self.scale, self.margin, self.fg, self.bg)?;
                if let Some((l, b)) = logo {
                    let at = svg.rfind("</svg>").expect("render_svg closes the document");
                    svg.insert_str(at, &svg_logo_element(&l.png, b, self.margin));
                }
                Ok(svg.into_bytes())
            }
            OutFormat::Ansi => unreachable!("terminal output is drawn by the caller"),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (data, out) = match cli.command {
        Command::Batch {
            csv,
            out_dir,
            format,
            force,
            style,
        } => {
            return run_batch(&csv, &out_dir, &format, force, &style.resolve()?);
        }
        Command::Text { data, out } => {
            check_output_opts(&out)?;
            (resolve_text_data(data)?, out)
        }
        Command::Wifi {
            ssid,
            password,
            auth,
            hidden,
            out,
        } => {
            check_output_opts(&out)?;
            (
                build_wifi_payload(&ssid, password.as_deref(), &auth, hidden)?,
                out,
            )
        }
        Command::Vcard {
            name,
            family,
            given,
            org,
            title,
            tel,
            email,
            url,
            address,
            note,
            out,
        } => {
            check_output_opts(&out)?;
            let card = VCard {
                name,
                family,
                given,
                org,
                title,
                tel,
                email,
                url,
                address,
                note,
            };
            (build_vcard(&card)?, out)
        }
        Command::Event {
            summary,
            start,
            end,
            location,
            description,
            out,
        } => {
            check_output_opts(&out)?;
            (
                build_event(&Event {
                    summary,
                    start,
                    end,
                    location,
                    description,
                })?,
                out,
            )
        }
    };

    let style = out.style.resolve()?;
    match &out.output {
        Some(path) => {
            let bytes = style.render(&data, format_from_path(path)?)?;
            std::fs::write(path, bytes)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("qrgen: wrote {}", path.display());
        }
        None => {
            // No file: print scannable terminal art to stdout.
            let matrix = encode(data.as_bytes(), style.ec)?;
            let art = if out.compact {
                render_ansi_compact(&matrix, style.margin)
            } else {
                render_ansi(&matrix, style.margin)
            };
            print!("{art}");
            std::io::stdout().flush().ok();
        }
    }
    Ok(())
}

/// Reject option combinations that would be silently ignored, before any
/// stdin read or file write happens.
fn check_output_opts(out: &OutputOpts) -> Result<()> {
    if out.compact && out.output.is_some() {
        bail!("--compact applies to terminal output only; remove -o or --compact");
    }
    if out.style.logo.is_some() && out.output.is_none() {
        bail!("--logo needs an image output (-o code.png or -o code.svg)");
    }
    Ok(())
}

/// One image per CSV row. Every row is checked, and every code encoded,
/// before the first file is written, so a bad row leaves nothing half done.
fn run_batch(
    csv: &std::path::Path,
    out_dir: &std::path::Path,
    format: &str,
    force: bool,
    style: &Style,
) -> Result<()> {
    let text = if csv.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read CSV from stdin")?;
        buf
    } else {
        std::fs::read_to_string(csv).with_context(|| format!("failed to read {}", csv.display()))?
    };
    let rows = parse_csv(&text)?;
    let Some((header, body)) = rows.split_first() else {
        bail!("the CSV is empty (it needs a header row with a \"data\" column)");
    };
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let data_col =
        col("data").ok_or_else(|| anyhow::anyhow!("the CSV header has no \"data\" column"))?;
    let file_col = col("file");
    let (ext, fmt) = if format == "svg" {
        ("svg", OutFormat::Svg)
    } else {
        ("png", OutFormat::Png)
    };
    let width = body.len().to_string().len().max(3);

    let mut jobs: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, row) in body.iter().enumerate() {
        let line = i + 2;
        let data = row.get(data_col).map(String::as_str).unwrap_or("");
        if data.is_empty() {
            bail!("CSV row {line}: the data column is empty");
        }
        let stem = match file_col
            .and_then(|c| row.get(c))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            Some(name) => {
                if !safe_file_stem(name) {
                    bail!("CSV row {line}: {name:?} is not a safe file name (letters, digits, - _ . and spaces; no leading dot)");
                }
                name.to_string()
            }
            None => format!("{:0width$}", i + 1),
        };
        if !seen.insert(stem.to_lowercase()) {
            bail!("CSV row {line}: the file name {stem:?} is used twice");
        }
        let bytes = style
            .render(data, fmt)
            .with_context(|| format!("CSV row {line}"))?;
        jobs.push((out_dir.join(format!("{stem}.{ext}")), bytes));
    }

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    if !force {
        if let Some((path, _)) = jobs.iter().find(|(p, _)| p.exists()) {
            bail!("{} already exists; pass --force to replace", path.display());
        }
    }
    for (path, bytes) in &jobs {
        let mut f = if force {
            std::fs::File::create(path)
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        }
        .with_context(|| format!("failed to write {}", path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    eprintln!(
        "qrgen: wrote {} image(s) to {}",
        jobs.len(),
        out_dir.display()
    );
    Ok(())
}

/// Resolve the `text` data argument: `-` means read the data from stdin,
/// with one trailing newline trimmed.
fn resolve_text_data(data: String) -> Result<String> {
    if data != "-" {
        return Ok(data);
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read data from stdin")?;
    trim_trailing_newline(&mut buf);
    if buf.is_empty() {
        bail!("no data received on stdin");
    }
    Ok(buf)
}

/// Remove a single trailing newline (`\n` or `\r\n`), as appended by `echo`
/// or line-buffered pipes. Interior newlines are preserved.
fn trim_trailing_newline(s: &mut String) {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::trim_trailing_newline;

    fn trimmed(input: &str) -> String {
        let mut s = input.to_string();
        trim_trailing_newline(&mut s);
        s
    }

    #[test]
    fn trims_one_trailing_newline_only() {
        assert_eq!(trimmed("https://example.com\n"), "https://example.com");
        assert_eq!(trimmed("secret\r\n"), "secret");
        assert_eq!(trimmed("two\n\n"), "two\n");
        assert_eq!(trimmed("plain"), "plain");
        assert_eq!(trimmed("multi\nline\n"), "multi\nline");
        assert_eq!(trimmed(""), "");
    }
}
