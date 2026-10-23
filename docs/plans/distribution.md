# Plan: installing wolf everywhere

Status: stage D1 is done; `wolf self update` of stage D4 is done in 0.2 (2026-10-18). Stage D2
and the static builds of D4 are done after 0.3.0 (2026-10-23, the author chose all of D2): the tap
and the bucket exist with 0.3.0, the release workflow builds the static archives and the `.deb`
and `.rpm` packages, and `packages.yml` updates the managers when a release is published. What is
left for the author: the secrets `PACKAGES_TOKEN` and `WINGET_TOKEN`, and the first winget pull
request (section 3). Stage D3 waits for paid accounts; `wolf self uninstall` and the AUR wait.

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

| Manager | What is there | Update on release |
| --- | --- | --- |
| Homebrew (macOS, Linux) | done: the tap `realfamousbae/homebrew-wolfpack`, `Formula/wolfpack.rb` written by `release/homebrew/formula.sh` (it passes `brew audit --strict`) | `packages.yml` writes the formula of the published release and pushes it to the tap (`PACKAGES_TOKEN`) |
| Scoop (Windows) | done: the bucket `realfamousbae/scoop-wolfpack`, `bucket/wolfpack.json` written by `release/scoop/manifest.sh`, with `checkver` and `autoupdate` | `packages.yml`, as for Homebrew |
| winget (Windows) | `release/winget/manifests.sh` writes the manifests: the zip as a portable command `wolf`, because the setup program runs `wolf setup` (about 1 GB) | `packages.yml` opens a pull request with `release/winget/submit.sh` (`WINGET_TOKEN`); Microsoft reviews each version |
| Debian/Ubuntu, Fedora | done: `.deb` and `.rpm` of the static builds, made with nfpm (`release/linux/nfpm.yaml`) | built in the release workflow, tried on Ubuntu and Fedora |
| Arch Linux | an AUR package `wolfpack-bin` | not yet |
| Cargo | done: `cargo install --locked wolfpack-lang` | `cargo publish` of every crate in order |

Answers of the author (2026-10-23): separate repositories for the tap and the bucket are
acceptable, and winget is wanted with its pull request for every release.

The first winget pull request is opened by the author, because Microsoft asks its author to sign
the contributor license agreement: `release/winget/manifests.sh 0.3.0 SHA256SUMS.txt out`, then
`release/winget/submit.sh 0.3.0 out new`, with `gh` signed in. The later ones are opened by
`packages.yml`.

The tokens are fine-grained personal access tokens of the owner, added under **Settings, Secrets
and variables, Actions** of `realfamousbae/wolfpack`: `PACKAGES_TOKEN` with **Contents: read and
write** on the tap and the bucket, `WINGET_TOKEN` a classic token with `public_repo`, which can
fork `microsoft/winget-pkgs` and open pull requests there.

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
- Done after 0.3.0: a Linux build for musl (`x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-musl`), static, so Alpine and old distributions are supported;
  `install.sh` picks it where glibc 2.35 is missing, and the `.deb` and `.rpm` packages hold it.
