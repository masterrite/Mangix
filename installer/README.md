# Building the installer

NSIS, per-user install: no administrator prompt, everything under
`%LOCALAPPDATA%\Programs\Mangix` and `HKCU`.

```
cargo build --release
copy target\release\mangix.exe installer\
copy path\to\pdfium.dll       installer\
cd installer
makensis mangix.nsi
```

Produces `Mangix-1.0.0-setup.exe`. Without PDF support, skip the DLL and use
`makensis /DNO_PDFIUM mangix.nsi`.

## What it does

- Installs `mangix.exe` (and `pdfium.dll`) to `%LOCALAPPDATA%\Programs\Mangix`
- Start Menu shortcut, and an entry in Apps & features with a real size
- Associates `.cbz`, `.cbr` and `.cb7` with Mangix
- Adds Mangix to the "Open with" list for `.pdf`, but does not make it the
  default; taking over every PDF on the machine would be presumptuous
- Uninstaller that only releases an extension if it still points at Mangix,
  so it won't stomp on a reader you installed later
- Removes `%APPDATA%\mangix` on uninstall, so reading positions and settings
  go with it

## GitHub Actions

`windows-latest` already has NSIS, so no install step is needed:

```yaml
jobs:
  release:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - run: copy target\release\mangix.exe installer\
      - run: makensis /DNO_PDFIUM mangix.nsi
        working-directory: installer
      - uses: actions/upload-artifact@v4
        with:
          name: installer
          path: installer/Mangix-*-setup.exe
```

Drop the `/DNO_PDFIUM` and add a step fetching `pdfium.dll` once PDF support
is in. Note that an unsigned installer will show a SmartScreen warning on
other people's machines; for personal use that is a one-time "More info ->
Run anyway".

## Verified

`makensis` compiles the script cleanly both with and without the DLL. It has
not been run on Windows from here, so the install itself, the shortcut and
the file associations are unexercised.
