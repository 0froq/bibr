# Release & CI/CD Guide

This project uses GitHub Actions for continuous integration and release automation.

## Workflows

### CI workflow

File: `.github/workflows/ci.yml`

Triggers:

- Push to `main`
- Any pull request

Checks performed:

- `cargo clippy --all-targets`
- `cargo test`
- `cargo build --release`

### Release workflow

File: `.github/workflows/release.yml`

Trigger:

- Push a version tag matching `v*` (for example `v0.1.0`)

Build targets:

- `x86_64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`

Produced assets per target:

- Archive (`.tar.gz` on Unix, `.zip` on Windows)
- SHA256 checksum file (`.sha256`)

## How to cut a release

1. Ensure `Cargo.toml` version is correct.
2. Ensure CI is passing on `main`.
3. Create and push a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

4. Wait for the release workflow to finish.
5. Verify assets and checksums on GitHub Releases.

## Notes

- The release page URL format is:
  `https://github.com/0froq/bibr/releases/tag/vX.Y.Z`
- End-user installation steps are documented in `docs/installation.md`.
