# qrgen

> Generate QR codes locally as PNG, SVG, or terminal art. Fast, offline Rust CLI.

[![CI](https://github.com/printemps-tokyo/qrgen/actions/workflows/ci.yml/badge.svg)](https://github.com/printemps-tokyo/qrgen/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)

`qrgen` turns text, a URL, or Wi-Fi credentials into a QR code from one command.
It writes crisp PNG or SVG files, or prints a scannable code straight to your
terminal. Everything runs locally with no network access, so the data you encode
never leaves your machine.

## Requirements

- A Rust toolchain (to build from source)

## Install

```bash
cargo install --git https://github.com/printemps-tokyo/qrgen
# or, from a clone:
cargo install --path .
```

Prebuilt binaries will be attached to each [release](https://github.com/printemps-tokyo/qrgen/releases) once a version is tagged.

## Usage

```bash
qrgen text <data> [options]    # encode arbitrary text or a URL
qrgen wifi --ssid <name> ...   # encode Wi-Fi credentials
```

Run `qrgen --help` or `qrgen <command> --help` for the full option list.

### text

```bash
qrgen text "https://printemps.tokyo"               # print to the terminal
qrgen text "https://printemps.tokyo" -o qr.png     # 8px modules PNG
qrgen text "hello world" -o qr.svg                 # scalable SVG
qrgen text "https://example.com" -o qr.png --scale 12 --ec h --fg "#1a1a2e"
echo "https://printemps.tokyo" | qrgen text -           # read the data from stdin
```

When `--output` is omitted, the code is printed to the terminal using black and
white background blocks, so it scans regardless of your terminal's color theme.
With `--output`, the file extension picks the format: `.png` or `.svg`.

Pass `-` as the data to read it from stdin (one trailing newline is trimmed).
This keeps secrets such as tokens or passwords out of your shell history.

### wifi

Encode Wi-Fi credentials so a phone offers to join the network when it scans the
code. Special characters in the SSID and password are escaped automatically.

```bash
qrgen wifi --ssid "Cafe Guest" --password "latte123" -o wifi.png
qrgen wifi --ssid "Office" --password "s3cret" --auth wpa --hidden -o wifi.svg
qrgen wifi --ssid "FreeSpot" --auth nopass -o open.png   # open network
```

| Option | Description |
| --- | --- |
| `--ssid <name>` | Network name (required) |
| `--password <pw>` | Password (required unless `--auth nopass`) |
| `--auth <type>` | `wpa` (default), `wep`, or `nopass` |
| `--hidden` | Mark the network as hidden |

## Options (both commands)

| Option | Description |
| --- | --- |
| `-o, --output <file>` | Output file; `.png`/`.svg` extension picks the format (default: terminal) |
| `--ec <l\|m\|q\|h>` | Error-correction level (default `m`; higher = more redundancy) |
| `--scale <n>` | Module size in pixels for PNG/SVG (default `8`) |
| `--margin <n>` | Quiet-zone width in modules (default `4`) |
| `--fg <hex>` | Foreground (dark module) color (default `000000`) |
| `--bg <hex>` | Background color (default `ffffff`) |

Colors accept 3- or 6-digit hex, with or without a leading `#`.

## Notes

- Higher error-correction levels (`q`, `h`) make a denser code that still scans
  when partly obscured or when you place a small logo over the center.
- The standard quiet zone is 4 modules; reducing `--margin` can hurt scanning.
- SVG output is resolution-independent and tiny, which suits print and the web.

## Programmatic API

```rust
use qrgen::{encode, render_svg, build_wifi_payload};
use qrcode::EcLevel;

let matrix = encode(b"https://printemps.tokyo", EcLevel::M)?;
let svg = render_svg(&matrix, 8, 4, [0, 0, 0], [255, 255, 255]);

let payload = build_wifi_payload("Cafe", Some("latte123"), "wpa", false)?;
let wifi = encode(payload.as_bytes(), EcLevel::M)?;
```

## License

[MIT](./LICENSE) (c) printemps.tokyo
