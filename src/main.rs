//! Command-line entry point for qrgen.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use qrgen::{
    build_wifi_payload, encode, format_from_path, parse_ec, parse_rgb, render_ansi, render_png,
    render_svg, OutFormat,
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
        /// The text or URL to encode.
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (data, out) = match cli.command {
        Command::Text { data, out } => (data, out),
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
                render_png(&matrix, out.scale, out.margin, fg, bg)
                    .save(path)
                    .with_context(|| format!("failed to write {}", path.display()))?;
            }
            OutFormat::Svg => {
                let svg = render_svg(&matrix, out.scale, out.margin, fg, bg);
                std::fs::write(path, svg)
                    .with_context(|| format!("failed to write {}", path.display()))?;
            }
            OutFormat::Ansi => unreachable!("file paths resolve to png or svg"),
        },
        None => {
            // No file: print scannable terminal art to stdout.
            print!("{}", render_ansi(&matrix, out.margin));
            std::io::stdout().flush().ok();
            return Ok(());
        }
    }

    eprintln!("qrgen: wrote {}", out.output.as_ref().unwrap().display());
    Ok(())
}
