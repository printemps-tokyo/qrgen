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

Tagged [releases](https://github.com/printemps-tokyo/qrgen/releases) carry prebuilt binaries for Linux (x86_64)
and macOS (x86_64, Apple silicon), with SHA-256 checksums.

## Usage

```bash
qrgen text <data> [options]    # encode arbitrary text or a URL
qrgen wifi --ssid <name> ...   # encode Wi-Fi credentials
qrgen vcard --name <name> ...  # a contact card
qrgen event --summary <t> ...  # a calendar event
qrgen batch <file.csv> ...     # one image per CSV row
```

Run `qrgen --help` or `qrgen <command> --help` for the full option list.

### text

```bash
qrgen text "https://printemps.tokyo"               # print to the terminal
qrgen text "https://printemps.tokyo" --compact     # half-height terminal art
qrgen text "https://printemps.tokyo" -o qr.png     # 8px modules PNG
qrgen text "hello world" -o qr.svg                 # scalable SVG
qrgen text "https://example.com" -o qr.png --scale 12 --ec h --fg "#1a1a2e"
echo "https://printemps.tokyo" | qrgen text -           # read the data from stdin
```

When `--output` is omitted, the code is printed to the terminal using black and
white background blocks, so it scans regardless of your terminal's color theme.
With `--output`, the file extension picks the format: `.png` or `.svg`.
Add `--compact` to pack two modules into each text row with half-block
characters, so large codes fit small terminal windows.

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

### vcard

A contact card as vCard 3.0, the form the zxing project documents for QR codes;
phone cameras offer to add the contact. Text is escaped as the format requires.

```bash
qrgen vcard --name "山田 太郎" --family 山田 --given 太郎 --org "Printemps" \
  --tel +81-3-0000-0000 --email taro@example.com --url https://printemps.tokyo -o card.png
```

| Option | Description |
| --- | --- |
| `--name <name>` | Display name (required) |
| `--family <name>`, `--given <name>` | Family and given names; without them the display name is the family name, since name order differs by language |
| `--org`, `--title` | Organization and job title |
| `--tel <n>`, `--email <a>` | Phone numbers and e-mail addresses (repeatable) |
| `--url <url>`, `--address <line>`, `--note <text>` | Web site, street address as one line, note |

### event

A calendar event as a bare `VEVENT`, again the form zxing documents.

```bash
qrgen event --summary "Release party" --start "2026-12-24 19:00" --location Tokyo -o party.png
qrgen event --summary "Conference" --start 2027-03-01 --end 2027-03-03 -o conf.svg
qrgen event --summary "Call" --start 2026-12-01T01:00Z -o call.png   # UTC
```

`--start` and `--end` take `YYYY-MM-DD` (all day) or `YYYY-MM-DD HH:MM[:SS]`
(also with `T`). A time without `Z` is a floating local time, read in the
scanning phone's time zone; add `Z` for UTC. Without `--end`, an all-day event
lasts one day and a timed one an hour. An all-day `--end` is exclusive, as in
iCalendar: `--start 2027-03-01 --end 2027-03-03` is two days.

### batch

One image per row of a CSV file with a header row. The `data` column is
encoded; the optional `file` column names the image (without extension),
otherwise rows are numbered `001`, `002`, ...

```bash
qrgen batch codes.csv --out-dir qr                 # qr/<file>.png
qrgen batch codes.csv --out-dir qr --format svg --logo logo.png
```

```csv
file,data
home,https://printemps.tokyo
menu,"https://example.com/menu?table=1,2"
```

Every row is checked and encoded before the first image is written, so a bad
row (an empty `data`, a duplicate or unsafe file name such as `../x`) stops the
run with nothing written. Existing images are not replaced unless `--force` is
given. Fields may be quoted, with `""` for a quote inside. All the styling
options below apply to every image.

### Logo

`--logo logo.png` puts a PNG in the middle of a PNG or SVG code: the modules
under it (plus one module of padding) are left light, and the logo is fitted
inside without stretching, blended by its transparency. `--logo-size` sets its
width as a percentage of the code (5-30, default 20). The hidden modules are
recovered by error correction, so a logo makes `--ec h` the default and refuses
`l` and `m`. The tests decode codes with 20% and 30% logos using an independent
reader (rqrr); still scan a code before printing it.

## Options (every command)

| Option | Description |
| --- | --- |
| `-o, --output <file>` | Output file; `.png`/`.svg` extension picks the format (default: terminal) |
| `--ec <l\|m\|q\|h>` | Error-correction level (default `m`, or `h` with `--logo`; higher = more redundancy) |
| `--scale <n>` | Module size in pixels for PNG/SVG (default `8`) |
| `--margin <n>` | Quiet-zone width in modules (default `4`) |
| `--fg <hex>` | Foreground (dark module) color (default `000000`) |
| `--bg <hex>` | Background color (default `ffffff`) |
| `--compact` | Terminal output only: half-height art using half-block characters |
| `--logo <png>` | A PNG logo in the middle (PNG/SVG output only) |
| `--logo-size <pct>` | Logo width in percent of the code (5-30, default `20`) |

Colors accept 3- or 6-digit hex, with or without a leading `#`.

## Notes

- Higher error-correction levels (`q`, `h`) make a denser code that still scans
  when partly obscured, which is what lets `--logo` cover the center.
- The standard quiet zone is 4 modules; reducing `--margin` can hurt scanning.
- SVG output is resolution-independent and tiny, which suits print and the web.

## Programmatic API

```rust
use qrgen::{encode, render_svg, build_wifi_payload};
use qrcode::EcLevel;

let matrix = encode(b"https://printemps.tokyo", EcLevel::M)?;
let svg = render_svg(&matrix, 8, 4, [0, 0, 0], [255, 255, 255])?;

let payload = build_wifi_payload("Cafe", Some("latte123"), "wpa", false)?;
let wifi = encode(payload.as_bytes(), EcLevel::M)?;
```

## License

[MIT](./LICENSE) (c) printemps.tokyo
