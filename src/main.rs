//! Command-line entry point for qrgen.

use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};

use qrgen::{
    build_wifi_payload, encode, format_from_path, parse_ec, parse_rgb, render_ansi,
    render_ansi_compact, render_png, render_svg, OutFormat,
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
}

/// Output and styling options shared by every subcommand.
#[derive(Args, Debug)]
struct OutputOpts {
    /// Output file; its extension (.png/.svg) picks the format. Omit to print
    /// the QR to the terminal.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Error-correction level: l, m, q, or h (higher = more redundancy).
    #[arg(long, default_value = "m")]
    ec: String,

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

    /// Terminal output only: pack two modules per text row using half-block
    /// characters, halving the height so large codes fit small terminals.
    #[arg(long)]
    compact: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // --compact only affects terminal art; combined with a file output it
    // would be silently ignored, so reject the combination up front (before
    // any stdin read or file write happens).
    let opts = match &cli.command {
        Command::Text { out, .. } | Command::Wifi { out, .. } => out,
    };
    if opts.compact && opts.output.is_some() {
        bail!("--compact applies to terminal output only; remove -o or --compact");
    }

    let (data, out) = match cli.command {
        Command::Text { data, out } => (resolve_text_data(data)?, out),
        Command::Wifi {
            ssid,
            password,
            auth,
            hidden,
            out,
        } => (
            build_wifi_payload(&ssid, password.as_deref(), &auth, hidden)?,
            out,
        ),
    };

    let ec = parse_ec(&out.ec)?;
    let fg = parse_rgb(&out.fg)?;
    let bg = parse_rgb(&out.bg)?;
    let matrix = encode(data.as_bytes(), ec)?;

    match &out.output {
        Some(path) => match format_from_path(path)? {
            OutFormat::Png => {
                render_png(&matrix, out.scale, out.margin, fg, bg)?
                    .save(path)
                    .with_context(|| format!("failed to write {}", path.display()))?;
            }
            OutFormat::Svg => {
                let svg = render_svg(&matrix, out.scale, out.margin, fg, bg)?;
                std::fs::write(path, svg)
                    .with_context(|| format!("failed to write {}", path.display()))?;
            }
            OutFormat::Ansi => unreachable!("file paths resolve to png or svg"),
        },
        None => {
            // No file: print scannable terminal art to stdout.
            let art = if out.compact {
                render_ansi_compact(&matrix, out.margin)
            } else {
                render_ansi(&matrix, out.margin)
            };
            print!("{art}");
            std::io::stdout().flush().ok();
            return Ok(());
        }
    }

    eprintln!("qrgen: wrote {}", out.output.as_ref().unwrap().display());
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
