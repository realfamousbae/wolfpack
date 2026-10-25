# Wolfpack

> ## ⚠️ Work in progress: expect big, breaking changes
>
> Wolfpack is under active development and is going to change a lot, including the language
> itself. Syntax, the standard library and the command line may change from one version to the
> next without a deprecation period: 0.2, for example, replaced `fn main` with a `main` section
> above `functions`. Do not build anything you cannot rewrite.

[![CI](https://github.com/realfamousbae/wolfpack/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/realfamousbae/wolfpack/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/realfamousbae/wolfpack)](https://github.com/realfamousbae/wolfpack/releases/latest)
[![Playground](https://img.shields.io/badge/try-playground-blue)](https://realfamousbae.github.io/wolfpack/)
[![License: MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%20or%20Apache--2.0-green)](#license)

Wolfpack is a small, statically typed programming language whose compiler, `wolf`, translates
your program into **readable TypeScript, Go and Rust**. You write the program once; the compiler
checks it and generates a project in the target language that looks as if a person had written it,
and that builds with that language's own tools.

```wolf
module main

types {
    enum Shape {
        Circle(Float)
        Rect { w: Float, h: Float }
    }
}

main {
    let shapes = [Shape.Circle(1.0), Shape.Rect { w: 2.0, h: 3.0 }]
    for s in shapes {
        print(area(s).to_string())
    }
}

functions {
    fn area(s: Shape) -> Float {
        match s {
            Shape.Circle(r) => 3.0 * r * r
            Shape.Rect { w, h } => w * h
        }
    }
}
```

```output
3.0
6.0
```

This program prints the same output on all three targets.
[Try it in the browser](https://realfamousbae.github.io/wolfpack/) without installing anything.

## Contents

- [Why wolfpack](#why-wolfpack)
- [Install](#install)
  - [Quick install](#quick-install)
  - [What the installer does](#what-the-installer-does)
  - [Installer options](#installer-options)
  - [Other ways to install](#other-ways-to-install)
  - [Package managers](#package-managers)
  - [Check the installation](#check-the-installation)
  - [Update](#update)
  - [Uninstall](#uninstall)
  - [Troubleshooting](#troubleshooting)
- [Your first program](#your-first-program)
- [Commands](#commands)
- [Using TypeScript, Go and Rust libraries](#using-typescript-go-and-rust-libraries)
- [Editors](#editors)
- [Learn more](#learn-more)
- [Status and roadmap](#status-and-roadmap)
- [Contributing](#contributing)
- [License](#license)

## Why wolfpack

- **Every file has the same shape.** A file is a list of sections in a fixed order (`imports`,
  `types`, `impl`, `functions`, `tests`, ...), so you always know where to look.
- **Ownership without lifetimes.** Values have one owner, and parameters say whether a function
  reads, changes or takes a value (`mut`, `own`, marked at the call too). The compiler checks
  this, which lets it emit borrowed references in Rust and plain shared objects in Go and
  TypeScript, without defensive copies.
- **One meaning on every target.** 64-bit integers that panic on overflow, strings measured in
  Unicode characters, maps that keep insertion order, the same panic messages: the differences of
  the three languages are hidden, and the tests of the compiler compare the output of every example
  on all three.
- **Errors you can read.** Every diagnostic has a code, points at the exact place and often
  suggests a fix; `wolf explain E0501` describes any code with a wrong and a fixed example.
- **No lock-in.** The generated project is ordinary TypeScript, Go or Rust. You can read it, ship
  it, or keep developing it without wolfpack.

## Install

### Quick install

Open a terminal and run one command.

**macOS and Linux**

```bash
curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh
```

No `curl`? Use `wget`:

```bash
wget -qO- https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh
```

**Windows** (PowerShell, no administrator rights needed)

```powershell
irm https://github.com/realfamousbae/wolfpack/releases/latest/download/install.ps1 | iex
```

Then **open a new terminal** (so it sees the new `PATH`) and check:

```bash
wolf --version
```

| Platform | Supported | Notes |
| --- | --- | --- |
| macOS 11 and newer | Apple silicon and Intel | one universal binary |
| Linux | x64 and arm64, glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and newer) | for Alpine and other musl systems, [build from source](#build-from-source) |
| Windows 10 and 11 | x64 | on Windows on ARM, the x64 build runs under emulation |

### What the installer does

1. Picks the build for your system and downloads it from the
   [GitHub release](https://github.com/realfamousbae/wolfpack/releases/latest).
2. Checks the download against `SHA256SUMS.txt` of the same release. A damaged or replaced file is
   never installed.
3. Puts `wolf` into your user's directory, without `sudo` or administrator rights:
   `~/.local/bin/wolf` on macOS and Linux, `%LOCALAPPDATA%\Programs\wolfpack\bin\wolf.exe` on
   Windows.
4. Adds that directory to your `PATH`: in `~/.profile` and in the profiles of bash, zsh and fish
   that you use (macOS, Linux), or in your user's `Path` (Windows). It adds each line only once.
5. Runs **`wolf setup`**, which downloads the tools that `wolf run` uses for each target: Node.js
   and TypeScript, Go and Rust. They go into `~/.wolfpack` (`%USERPROFILE%\.wolfpack` on Windows),
   take about 1 GB, and **do not change** any versions of these tools that you installed yourself.
   Each of them is also checked against a SHA-256 sum built into `wolf`.

The installer is safe to run again: it replaces `wolf` with the new version and keeps the rest.
If the network breaks off while the script is downloading, nothing runs, because the scripts do
all their work in a function called on their last line.

Prefer to read a script before running it? Download it, read it, then run it:

```bash
curl -fsSLO https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh
less install.sh
sh install.sh
```

### Installer options

**macOS and Linux.** Pass options after `sh -s --`:

```bash
curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh -s -- --version 0.1.0 --no-setup
```

| Option | Environment variable | Meaning |
| --- | --- | --- |
| `--version <x.y.z>` | `WOLFPACK_VERSION` | install this release instead of the latest one |
| `--dir <path>` | `WOLFPACK_INSTALL_DIR` | install `wolf` into this directory |
| `--no-modify-path` | `WOLFPACK_NO_MODIFY_PATH=1` | do not change shell profiles |
| `--no-setup` | `WOLFPACK_NO_SETUP=1` | do not run `wolf setup`; run it later yourself |
| | `WOLFPACK_DOWNLOAD_URL` | download from a mirror instead of GitHub |
| | `WOLFPACK_SKIP_CHECKSUM=1` | install without the SHA-256 check, when the system has no tool for it |

**Windows.** Set environment variables first, then run the installer:

```powershell
$env:WOLFPACK_VERSION = '0.1.0'; $env:WOLFPACK_NO_SETUP = '1'
irm https://github.com/realfamousbae/wolfpack/releases/latest/download/install.ps1 | iex
```

The same variables work: `WOLFPACK_VERSION`, `WOLFPACK_INSTALL_DIR`, `WOLFPACK_NO_MODIFY_PATH`,
`WOLFPACK_NO_SETUP` and `WOLFPACK_DOWNLOAD_URL`.

`wolf setup` itself installs into `~/.wolfpack`, or into the directory named by `WOLFPACK_HOME`.

### Other ways to install

Every way to install, for each system:

| System | Terminal script | Graphical installer | Archive (only `wolf`) | Package manager | From source |
| --- | --- | --- | --- | --- | --- |
| macOS | `install.sh` | `wolfpack-<version>-macos.pkg` | `wolfpack-macos-universal.tar.gz` | Homebrew | `cargo install` |
| Linux | `install.sh` | none | `wolfpack-linux-x64.tar.gz`, `wolfpack-linux-arm64.tar.gz`, and their static builds `...-musl.tar.gz` | Homebrew, `.deb`, `.rpm` | `cargo install` |
| Windows | `install.ps1` | `wolfpack-<version>-windows-x64-setup.exe` | `wolfpack-windows-x64.zip` | Scoop, winget | `cargo install` |

#### Graphical installers

Each [release](https://github.com/realfamousbae/wolfpack/releases/latest) also has installers you
can double-click. They do the same as the scripts, including `wolf setup`.

- **macOS**: `wolfpack-<version>-macos.pkg`, which installs `wolf` into `/usr/local/bin`. The
  package is not signed yet, so macOS refuses to open it with a double-click: open it with a
  right-click and **Open**, or allow it under **System Settings, Privacy & Security**.
- **Windows**: `wolfpack-<version>-windows-x64-setup.exe`. It installs for your user, without
  administrator rights. Windows SmartScreen warns about an unknown publisher: choose
  **More info** and **Run anyway**.

#### Archives

`wolfpack-macos-universal.tar.gz`, `wolfpack-linux-x64.tar.gz`, `wolfpack-linux-arm64.tar.gz` and
`wolfpack-windows-x64.zip` contain only the `wolf` binary. Unpack it into any directory on your
`PATH` and run `wolf setup`. `wolfpack-linux-x64-musl.tar.gz` and `wolfpack-linux-arm64-musl.tar.gz`
(from 0.4.0 on) are static builds that need no glibc: they run on Alpine and on
older distributions, and `install.sh` picks them there.

### Package managers

| Manager | Install | Update |
| --- | --- | --- |
| Homebrew (macOS, Linux) | `brew install realfamousbae/wolfpack/wolfpack` | `brew upgrade wolfpack` |
| Scoop (Windows) | `scoop bucket add wolfpack https://github.com/realfamousbae/scoop-wolfpack`, then `scoop install wolfpack` | `scoop update wolfpack` |
| winget (Windows) | `winget install realfamousbae.wolfpack` | `winget upgrade realfamousbae.wolfpack` |
| apt (Debian, Ubuntu) | `sudo apt install ./wolfpack-linux-x64.deb` | the `.deb` of a newer release |
| dnf (Fedora, RHEL) | `sudo dnf install ./wolfpack-linux-x64.rpm` | the `.rpm` of a newer release |

Package managers do not run `wolf setup`: run it yourself for the tools of the targets, or run
programs with `wolf run --vm`, which needs no tools. The `.deb` and `.rpm` files (`-x64` and
`-arm64`) are attached to the releases from 0.4.0 on and hold the static build, so they work on every
distribution. winget offers the package once Microsoft has accepted it into its catalog.

#### Build from source

You need [Rust](https://rustup.rs) 1.85 or later. The package on
[crates.io](https://crates.io/crates/wolfpack-lang) is `wolfpack-lang`:

```bash
cargo install --locked wolfpack-lang
wolf setup
```

This works on every system Rust supports, including Alpine and other musl-based Linux
distributions.

#### Use your own Node.js, Go and Rust

`wolf setup` is optional. When it has not installed a tool, `wolf` looks for it on `PATH`. The
oldest supported versions are Node.js 22, TypeScript 5, Go 1.24 and Rust 1.85. You only need the
tools of the targets you use: `wolf setup --target go` installs just Go.

### Check the installation

```text
$ wolf --version
wolf 0.5.0
$ wolf setup --check
```

`wolf setup --check` installs nothing. It lists every tool, where it was found and its version,
and exits with code 1 if a tool of an enabled target is missing. Run `wolf setup` to install what
is missing.

### Update

From 0.2 on, `wolf` updates itself:

```bash
wolf self update
```

It downloads the latest release, checks its SHA-256 sum, replaces `wolf` and runs `wolf setup`
for the tools of the new version. `wolf self update --check` only says whether a newer release
exists, and `wolf self update --version 0.2.0` installs a given release. Once a day `wolf` also
looks for a new release in the background and asks `Update now? [y/N]` before the next command,
only in a terminal. To turn the question off, set `WOLFPACK_NO_UPDATE_CHECK=1`, or write
`update.check = false` into `~/.wolfpack/config.toml`; it is never asked in CI.

A `wolf` installed with Cargo, Homebrew, Scoop, winget or a `.deb` or `.rpm` package is updated
with that tool, and `wolf self update` says which command to use. Versions older than 0.2 are
updated by running the installer again: the [quick install](#quick-install) command, or the newest
graphical installer.

### Uninstall

```bash
wolf self uninstall
```

It removes `wolf` and the lines that the installer added to put it on `PATH`, and asks before it
removes `~/.wolfpack` with the tools of the targets (`--yes` removes it without asking). For a
`wolf` of a package manager it prints the command of that manager instead, and for the Windows
setup program it starts its uninstaller. To do the same by hand:

**macOS and Linux** (installed with the script):

```bash
rm ~/.local/bin/wolf
rm -rf ~/.wolfpack
```

Then remove the lines after `# Added by the wolfpack installer.` from `~/.profile`, `~/.bashrc`,
`~/.bash_profile`, `~/.zshrc` and `~/.zprofile`, and delete
`~/.config/fish/conf.d/wolfpack.fish` if it exists.

**macOS** (installed with the `.pkg`):

```bash
sudo rm /usr/local/bin/wolf
sudo pkgutil --forget io.github.realfamousbae.wolfpack
rm -rf ~/.wolfpack
```

**A package manager**: `brew uninstall wolfpack`, `scoop uninstall wolfpack`,
`winget uninstall realfamousbae.wolfpack`, `sudo apt remove wolfpack` or `sudo dnf remove
wolfpack`, then `rm -rf ~/.wolfpack` for the target tools.

**Windows**: if you used the setup program, uninstall **wolfpack** under **Settings, Apps**. If
you used `install.ps1`, delete `%LOCALAPPDATA%\Programs\wolfpack` and remove it from your user's
`Path` (**Settings, System, About, Advanced system settings, Environment Variables**). In both
cases, delete `%USERPROFILE%\.wolfpack` to remove the target tools.

### Troubleshooting

<details>
<summary><code>wolf: command not found</code> right after installing</summary>

Open a new terminal: the old one does not see the changed `PATH`. If it still fails, add the
directory yourself, for example `export PATH="$HOME/.local/bin:$PATH"` at the end of the profile
of your shell, or run `~/.local/bin/wolf` directly.
</details>

<details>
<summary>The download fails behind a corporate proxy</summary>

On macOS and Linux, set `https_proxy` (for example
`export https_proxy=http://proxy.example.com:8080`) before running the installer. On Windows,
the installer uses the proxy of the system and your Windows sign-in. If a mirror of the release
files is available inside your network, point `WOLFPACK_DOWNLOAD_URL` at it.
</details>

<details>
<summary><code>the download ... is damaged</code></summary>

The file did not arrive intact, or a proxy changed it. Run the installer again. If it keeps
failing, download the archive and `SHA256SUMS.txt` from the release page by hand and compare them
with `shasum -a 256` (macOS), `sha256sum` (Linux) or `Get-FileHash` (Windows).
</details>

<details>
<summary><code>this Linux uses musl</code> or <code>wolf needs glibc 2.35 or newer</code></summary>

The installers of 0.3.0 and older say this on Alpine and on distributions older than Ubuntu 22.04.
From 0.4.0 on, `install.sh` installs the static build there instead, so the current installer
works. For 0.3.0 or older, [build wolf from source](#build-from-source); it takes a few minutes.
</details>

<details>
<summary>Windows says that running scripts is disabled</summary>

`irm ... | iex` does not run a script file, so the execution policy does not apply to it. If your
organisation blocks it anyway, use the setup program from the release page instead.
</details>

<details>
<summary><code>wolf setup</code> failed, or <code>wolf run</code> cannot find <code>node</code>, <code>go</code> or <code>cargo</code></summary>

Run `wolf setup` again: it installs only what is missing and says which download failed and why.
On macOS, Rust needs the Command Line Tools of Xcode; if macOS offers to install them, accept,
then run `wolf setup` again. `wolf setup --check` shows what is installed and where.
</details>

<details>
<summary>macOS refuses to open the <code>.pkg</code></summary>

The package is not signed yet. Right-click it and choose **Open**, or allow it under
**System Settings, Privacy & Security**. The terminal installer does not have this problem.
</details>

## Your first program

```text
$ wolf new hello
created project `hello`
$ cd hello
$ wolf run
Hello, wolfpack!
$ wolf run --target go
Hello, wolfpack!
$ wolf run --target rust
Hello, wolfpack!
$ wolf run --vm
Hello, wolfpack!
```

`wolf new` creates a project with `src/main.wolf`, and `wolf run` builds and runs it. Edit
`src/main.wolf` and run it again. The generated code is in `out/<target>/`;
`wolf build --target all` writes all three. `wolf run --vm` runs the program in the virtual
machine of `wolf` itself, without Node.js, Go or Rust.

A project always has the same layout:

```text
hello/
  wolfpack.toml     name, version, targets, dependencies
  src/
    main.wolf       the program (lib.wolf for a library)
  tests/            integration tests
  examples/         small programs that use the project
  out/              generated TypeScript, Go and Rust (not in git)
```

## Commands

| Command | What it does |
| --- | --- |
| `wolf new <name>` | create a project (`--kind lib` for a library) |
| `wolf check` | check the project without generating code; `--bytecode` prints what the VM runs |
| `wolf build` | generate the target project in `out/<target>/` |
| `wolf run` | build and run the program (`--target ts\|go\|rust`, `--example <name>`); `--vm` runs it in the VM of `wolf` |
| `wolf test` | run the tests of the project (`--vm` in the VM) |
| `wolf fmt` | format the source files (`--check` only reports) |
| `wolf explain <code>` | explain an error code with a wrong and a fixed example |
| `wolf setup` | install the tools of the targets; `--check` lists them |
| `wolf lsp` | the language server for editors, on standard input and output |

`wolf <command> --help` shows every option. The [examples](examples/) of this repository are
complete projects; run one with `wolf run` in its directory.

## Using TypeScript, Go and Rust libraries

The standard library of wolfpack is small on purpose. Everything else comes from the target
languages through the `extern` section of a module: you declare a function or a type with a
wolfpack signature and its path in the target, and call it like any other function.

| What | Supported |
| --- | --- |
| The standard library of a target (`strings.ToUpper`, `Math.max`, `std::cmp::max`) | yes |
| Packages from npm, Go modules and crates.io, listed in `wolfpack.toml` | yes |
| Values at the boundary: `Int`, `Float`, `Bool`, `String`, `List`, `Option`, `Result`, opaque types | yes |
| `Map`, callbacks and records at the boundary | yes |
| Your own `.ts`, `.go` and `.rs` files inside the project, named by `native:` paths | yes |
| Native modules: your own target code imported like a module, with no `extern` lines (`imports { native.text.slug }`) | yes |
| Declarations read from packages of npm, Go and crates.io, without `extern` lines | later |

[Chapter 12 of the guide](docs/guide/12-extern.md) shows how to use `extern` and native modules.
The [`fuzzy`](examples/fuzzy/) example uses a package of each target, and
[`native-slug`](examples/native-slug/) imports code written in each target language as a native
module; `wolf check --native` prints what the compiler read from that code. The design is in
[docs/plans/native-code.md](docs/plans/native-code.md).

## Editors

`wolf lsp` is a language server: an editor with a client for the Language Server Protocol shows
the errors of `wolf check` as you type, the type or signature of a name on hover, goes to the
declaration of a name and formats with `wolf fmt`. [`editors/vscode/`](editors/vscode/) is an
extension for VS Code with highlighting that starts it.

## Learn more

- [The playground](https://realfamousbae.github.io/wolfpack/) runs the compiler in the browser:
  edit a program, run it, and read the TypeScript, Go and Rust it becomes.
- [The guide](docs/guide/README.md) teaches the language step by step, from the first program to
  ownership, generics, modules and target libraries.
- [The specification](docs/spec/) is the exact definition of the language.
- [The examples](examples/README.md) show the language on small programs.
- [Decisions](docs/decisions/) record why the language and the compiler are the way they are.

## Status and roadmap

Wolfpack is a young language: expect changes between versions. The compiler has a lexer, a
parser, name resolution, a type checker with generics and interfaces, an ownership checker, and
backends for TypeScript, Go and Rust, plus `extern` declarations for libraries of the target
languages and your own target code, a bytecode virtual machine (`wolf run --vm`), a formatter,
`wolf explain` and a language server (`wolf lsp`). The [changelog](CHANGELOG.md) lists what each
milestone added.

Next:

- package managers (Homebrew, Scoop, winget) and signed installers
  ([plan](docs/plans/distribution.md));
- later: async, and web and GUI projects.

## Contributing

Issues and pull requests are welcome. The branches are:

| Branch | Purpose |
| --- | --- |
| `master` | released code; the playground is published from it, and release tags are made on it |
| `night` | the integration branch, where new work lands first; it may be unstable |
| other branches | single features and experiments, merged into `night` |

Send pull requests to `night`: GitHub suggests `master` as the base branch, so choose `night`
when you open one. Before a commit, run `cargo fmt`, `cargo clippy -- -D warnings` and
`cargo test`; the end-to-end tests need `node`, `tsc`, `go` and `cargo` on `PATH` (`wolf setup`
installs them). Any change of the language starts with a change of [the specification](docs/spec/).

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.
