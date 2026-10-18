# Plan: installing wolf everywhere

Status: **draft plan** (2026-10-17). Stage D1 is done; `wolf self update` of stage D4 is done in
0.2 (2026-10-18); the other stages wait for the author.

## 1. Goal

One command in a terminal installs `wolf` on macOS, Linux and Windows, works on unusual machines
(no `curl`, a proxy, a non-default shell, an old PowerShell, no administrator rights), never leaves
a half-installed `wolf`, and says clearly what went wrong when it cannot work.

## 2. Stage D1: installer scripts (done)

| Platform | Command | Script |
| --- | --- | --- |
| macOS, Linux | `curl -fsSL .../install.sh \| sh` (or `wget -qO- ... \| sh`) | `release/install.sh` |
| Windows | `irm .../install.ps1 \| iex` | `release/install.ps1` |

What makes them sturdy:

| Problem | `install.sh` | `install.ps1` |
| --- | --- | --- |
| A download that breaks off halfway | the script runs only from `main` at its last line | the script runs only from `Install-Wolfpack` at its last line |
| A damaged or replaced archive | checked against `SHA256SUMS.txt` (`sha256sum`, `shasum` or `openssl`) | checked with `Get-FileHash` |
| No `curl` | `wget` | built in (`Invoke-WebRequest`) |
| Network errors | three tries | three tries |
| A proxy | `https_proxy` and friends (curl and wget) | the system proxy with the Windows sign-in |
| Old TLS defaults | `--proto =https --tlsv1.2` | TLS 1.2 forced in PowerShell 5.1 |
| Unsupported systems | clear errors for musl (Alpine), glibc older than 2.35, 32-bit ARM, BSD, Windows shells | clear errors for 32-bit Windows; a note for ARM64 (x64 under emulation) |
| A `wolf` that is running | copy next to it, then rename | rename the old `wolf.exe`, then copy |
| A `wolf` that cannot run | `wolf --version` is tried before installing (`noexec` `/tmp`, wrong libc) | `wolf --version` after installing (antivirus) |
| `PATH` | `.profile`, `.bashrc`, `.bash_profile`, `.zshrc`, `.zprofile` (only the ones that exist, plus `.profile`, and `.zshrc` when zsh is the login shell), fish `conf.d`; every line once | the user's `Path` in the registry (not `setx`, which cuts at 1024 characters), kept unexpanded; every entry once |
| Running twice | safe; updates `wolf` | safe; updates `wolf` |
| Options | `--version`, `--dir`, `--no-modify-path`, `--no-setup`, environment variables | the same environment variables |

The release workflow tries both scripts on clean macOS, Ubuntu and Windows machines (Windows
PowerShell 5.1 and PowerShell 7) with the files of the run, before a release is created.

## 3. Stage D2: package managers

Ordered by how many users each one reaches and how little it costs to keep.

| Manager | What is needed | Update on release |
| --- | --- | --- |
| Homebrew (macOS, Linux) | a tap repository `realfamousbae/homebrew-wolfpack` with `Formula/wolfpack.rb` that downloads the release archives | a workflow step that rewrites the version and the sums and pushes to the tap |
| Scoop (Windows) | a bucket repository with `wolfpack.json` (the zip, `bin: wolf.exe`, `checkver`, `autoupdate`) | Scoop's `autoupdate` reads the release; nothing to push |
| winget (Windows) | a manifest in `microsoft/winget-pkgs` for the setup program | `wingetcreate update` in the release workflow; each version is a pull request reviewed by Microsoft |
| Debian/Ubuntu, Fedora | `.deb` and `.rpm` built with `cargo-deb` / `cargo-generate-rpm`, attached to the release | built in the release workflow |
| Arch Linux | an AUR package `wolfpack-bin` | a workflow step pushing to the AUR |
| Cargo | done: `cargo install --locked wolfpack-lang` (version 0.1.0 on crates.io) | `cargo publish` of every crate in order, in the release workflow |

Questions for the author:

1. Are separate repositories (the Homebrew tap, the Scoop bucket) acceptable?
2. winget needs a public pull request to a Microsoft repository for every release. Is that wanted?

## 4. Stage D3: trust

- **macOS**: sign the package and the binary with a Developer ID and notarize them, so Gatekeeper
  opens the package with a double-click. Needs an Apple Developer account (paid).
- **Windows**: sign `wolf.exe` and the setup program, so SmartScreen does not warn. Needs a code
  signing certificate, or Azure Trusted Signing.
- **Everywhere**: build provenance (`actions/attest-build-provenance`), so
  `gh attestation verify` proves that a file was built by this repository's workflow.

## 5. Stage D4: `wolf self update` and a static Linux build

- `wolf self update` downloads the latest release, checks the sum and replaces itself, with the
  same rules as the scripts. `wolf self uninstall` removes `wolf`, the `PATH` lines and, after a
  question, `~/.wolfpack`.
- A Linux build for musl (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`), static, so
  Alpine and old distributions are supported and the glibc check of `install.sh` goes away.
