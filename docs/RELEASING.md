# Releasing fnscope

The product is one Rust binary with the React UI embedded. A release is a
version tag; GitHub Actions does the rest.

## Versioning

- The version lives once in `Cargo.toml` (`[workspace.package].version`).
- Releases use SemVer tags: `v0.1.0`, `v0.2.0`, `v1.0.0`.
- A prerelease uses a suffix: `v0.2.0-rc.1`. The release job marks any tag with
  a `-` as a GitHub prerelease.
- `fnscope --version` prints the Cargo version.
- The first public release is **0.1.0**.

## First release (already at 0.1.0)

```bash
bash scripts/release.sh 0.1.0
git push origin main --follow-tags
```

Because the workspace is already at `0.1.0`, the script only creates the
annotated tag. It refuses a dirty tree or an existing tag, and it never pushes.

## Later releases

```bash
bash scripts/release.sh 0.2.0
cargo test --workspace
git push origin main --follow-tags
```

The script updates `Cargo.toml`, runs `cargo check` to refresh `Cargo.lock`,
commits `chore(release): v0.2.0`, and creates the tag. Run the tests before
pushing if the release commit is more than a version bump.

## What the workflows do

### CI (`.github/workflows/ci.yml`)

Runs on every push to `main` and every pull request:

- rebuilds `ui/dist` and fails if the committed artifacts do not match source;
- runs the UI checks and helper tests;
- runs `cargo fmt --all -- --check` and `cargo test --workspace`;
- runs the culture gate;
- builds the release binary on Linux, Windows, macOS Intel and macOS ARM.

### Release (`.github/workflows/release.yml`)

Runs on a `v*` tag:

1. Rebuilds and verifies the embedded UI, and checks that the tag matches
   `Cargo.toml`.
2. Builds natively on each runner - no cross-compilation.
3. Packages each binary with `README.md` and `LICENSE`.
4. Publishes the archives, `SHA256SUMS`, and generated release notes.

## Artifacts

| Target | File |
| --- | --- |
| Windows x64 | `fnscope-<version>-x86_64-pc-windows-msvc.zip` |
| macOS Apple Silicon | `fnscope-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `fnscope-<version>-x86_64-apple-darwin.tar.gz` |
| Linux x64 | `fnscope-<version>-x86_64-unknown-linux-gnu.tar.gz` |

Every archive contains the `fnscope` binary (or `fnscope.exe`), `README.md` and
`LICENSE`.

## Verify a download

```bash
sha256sum -c SHA256SUMS
```

On macOS use `shasum -a 256 -c SHA256SUMS`. The builds are **unsigned** for
0.1.0:

- Windows SmartScreen may warn; choose "More info" then "Run anyway", or verify
  the checksum first.
- macOS quarantines downloaded unsigned binaries; after extracting, run
  `xattr -d com.apple.quarantine fnscope` if Gatekeeper refuses it.
- Linux is a glibc build from Ubuntu 22.04; older/musl-only systems should build
  from source.

## Install channels

0.1.0 ships through GitHub Releases only. Later options, in order of effort:

- **winget** (Windows): a manifest in `microsoft/winget-pkgs`; needs a stable
  installer URL and review.
- **Homebrew** (macOS/Linux): a tap with a formula pointing at the release
  archive + SHA-256.
- **crates.io**: `cargo install fnscope` once the CLI is published; the binary
  builds the UI from the committed `ui/dist`, so no Node is needed.

Add signing/notarization before promoting those channels; the workflow already
packages the archives they need.

## Manual fallback

If the script is unavailable:

```bash
git tag -a v0.1.0 -m "fnscope v0.1.0"
git push origin main --follow-tags
```

The release job enforces that the tag matches `Cargo.toml`, so bump the version
first when it is not the initial release.

## Publishing checklist

- [ ] Working tree clean and `main` up to date.
- [ ] `cargo test --workspace` and the UI checks green.
- [ ] `Cargo.toml` version equals the tag without `v`.
- [ ] `LICENSE` and `README.md` present and current.
- [ ] Tag created; push the branch and tag.
- [ ] GitHub Release shows four archives and `SHA256SUMS`.
- [ ] Download one archive, verify its checksum, run `fnscope --version`.
