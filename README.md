# Mangix

A lightweight comic/manga reader for personal use. Written by Claude Opus 5 in Rust and [Slint](https://slint.dev) front end. Opens .cbz, .zip, and image files/folders natively. Supports .cbz, .rar, .cb7 and .7z using 7-Zip, and supports .pdf using pdfium.

## Features

- Multi-file support
- Keyboard navigation
- Light/dark mode toggle
- Saves reading progress
- Single/double page layout
- Left-to-right/right-to-left direction switch

## Installation

### Installer

Download the Mangix_vx.x.x_setup.exe and run. Run as administrator if you want to install in C:/Program Files.

### Portable version

Download the "Mangix portable.zip" and extract Mangix.exe and pdfium.dll into the same folder.

## Building

```sh
cargo run --release
```


On Linux you'll need the usual GUI development packages. For Debian/Ubuntu:

```sh
sudo apt install build-essential libfontconfig-dev libxkbcommon-dev \
                 libxcb1-dev libx11-dev libgl1-mesa-dev
```

### PDF

PDF pages are rendered by pdfium, Google's PDF engine. Put the library file in the same folder as the executable.

### RAR and 7z

For Windows, install 7-zip.

For Linux, **Debian and Ubuntu ship 7-Zip without the RAR codec.** To install 7zip-rar:

```sh
sudo apt install 7zip 7zip-rar      # or p7zip-full p7zip-rar on older releases
```

## Controls

| | |
|---|---|
| Turn the page | `→` `←`, `Space`, `PgUp` / `PgDn`, or click the left/right third of the page |
| Jump | `Home`, `End`, the thumbnail rail, or click anywhere on the progress strip |
| Zoom | `+` / `-`, or `Ctrl` + wheel. Drag to pan. |
| Fit | `0` whole page · `W` width · `H` height · `1` actual pixels |
| Two-up (facing pages) | `D` |
| Right-to-left | `M` |
| Thumbnails | `T` |
| Light / dark | `L` |
| Hide the chrome | `F`, `Esc` to bring it back |
| Settings | `,` |

Arrow keys follow the screen, so in right-to-left mode left arrow advances the story. Spacebar and the page keys always move forward regardless.

Reading progress is saved per comic in `progress.tsv` under your data directory (`%APPDATA%\mangix` on Windows, `~/.local/share/mangix` on Linux).

## File structure

```
mangix/
├── Cargo.toml          name = "mangix", version = "1.0.0"
├── build.rs
├── LICENSE             MIT
├── THIRD-PARTY.md
├── README.md
├── .gitignore
├── .github/workflows/build.yml
├── src/                main, book, worker, pdf, settings, progress
├── ui/                 app.slint, icon.png
├── assets/mangix.ico
└── installer/          mangix.nsi, README.md
```

## License

This project is built in Rust and Slint, and follows MIT license.
