# Installation Guide

This guide covers all supported ways to install `bibr` as an end user.

## 1) Install from GitHub Releases (recommended)

Go to the latest release page:

`https://github.com/0froq/bibr/releases/latest`

Download the archive matching your platform:

- Linux: `bibr-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- macOS (Intel): `bibr-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- Windows: `bibr-vX.Y.Z-x86_64-pc-windows-msvc.zip`

### Verify checksum (recommended)

Each release archive has a matching `.sha256` file.

Linux/macOS:

```bash
shasum -a 256 -c bibr-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Windows (PowerShell):

```powershell
Get-FileHash .\bibr-vX.Y.Z-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

Compare the hash with the value in the `.sha256` file.

### Install binary into PATH

Linux/macOS:

```bash
tar -xzf bibr-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
sudo mv bibr /usr/local/bin/
```

Windows (PowerShell):

```powershell
Expand-Archive .\bibr-vX.Y.Z-x86_64-pc-windows-msvc.zip -DestinationPath .\bibr-unpacked
Move-Item .\bibr-unpacked\bibr.exe "$env:USERPROFILE\bin\bibr.exe"
```

## 2) Install directly from GitHub using Cargo

Requires Rust/Cargo.

```bash
cargo install --git https://github.com/0froq/bibr.git
```

## 3) Install from local source checkout

```bash
git clone https://github.com/0froq/bibr.git
cd bibr
cargo install --path .
```

## Post-install check

```bash
bibr --help
bibr doctor
```

If `bibr` is not found, ensure your Cargo/bin or install location is in your PATH.
