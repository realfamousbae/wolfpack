//! `wolf setup`: installs the tools of the targets into the wolfpack home (spec chapter 8,
//! section 2a). Every download comes from the official site of the tool and is checked against
//! the SHA-256 sum below; each tool is unpacked into a temporary directory that is renamed into
//! place only when it is complete.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use wolf_diag::{Diagnostic, codes};
use wolf_project::Target;

use crate::tools::{self, exe};

/// The version of Node.js (with npm) that `wolf setup` installs.
pub(crate) const NODE_VERSION: &str = "24.22.0";
/// Its directory in `<home>/toolchains`.
pub(crate) const NODE_DIR: &str = "node-24.22.0";
/// The version of TypeScript.
pub(crate) const TYPESCRIPT_VERSION: &str = "5.9.3";
/// Its directory.
pub(crate) const TYPESCRIPT_DIR: &str = "typescript-5.9.3";
/// The version of Go.
pub(crate) const GO_VERSION: &str = "1.27.2";
/// Its directory.
pub(crate) const GO_DIR: &str = "go-1.27.2";
/// The version of Rust.
pub(crate) const RUST_VERSION: &str = "1.99.1";
/// Its directory, with `rustup/` and `cargo/` inside.
pub(crate) const RUST_DIR: &str = "rust-1.99.1";
/// The version of rustup that installs Rust.
const RUSTUP_VERSION: &str = "1.29.1";

/// The operating system and processor of this machine, as the downloads name them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Platform {
    MacArm,
    MacX64,
    LinuxArm,
    LinuxX64,
    WindowsX64,
}

fn platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(Platform::MacArm),
        ("macos", "x86_64") => Some(Platform::MacX64),
        ("linux", "aarch64") => Some(Platform::LinuxArm),
        ("linux", "x86_64") => Some(Platform::LinuxX64),
        ("windows", "x86_64") => Some(Platform::WindowsX64),
        _ => None,
    }
}

/// How a download is unpacked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// A `.tar.gz`; the number of leading path components to drop.
    TarGz(usize),
    /// A `.zip`; the number of leading path components to drop.
    Zip(usize),
    /// A single program, stored under this name.
    Program(&'static str),
}

/// One file to download.
#[derive(Clone, Debug)]
struct Download {
    url: String,
    sha256: &'static str,
    kind: Kind,
}

fn node(p: Platform) -> Download {
    let (name, sha256, kind) = match p {
        Platform::MacArm => (
            "darwin-arm64.tar.gz",
            "6a9724d9b50a5f22b4816cded441b5a8d9ddae546191a5e5b7efa5a5e2ee26d5",
            Kind::TarGz(1),
        ),
        Platform::MacX64 => (
            "darwin-x64.tar.gz",
            "495e3f273c645f3c4677d438304cdfc701729ea5dec5728f5c8e76a2dece4b91",
            Kind::TarGz(1),
        ),
        Platform::LinuxArm => (
            "linux-arm64.tar.gz",
            "b7e7991d5407815adfa74a2a9cd4db683552c8ce32ff4a295c22e972849c3f5d",
            Kind::TarGz(1),
        ),
        Platform::LinuxX64 => (
            "linux-x64.tar.gz",
            "28ea0a438de9a13bd7891fd758eee6c7819ee8af5442dfd8d45bd65e56d4228c",
            Kind::TarGz(1),
        ),
        Platform::WindowsX64 => (
            "win-x64.zip",
            "f9683a19877e12a9b167305c8db71040d812876359de21f0d2e97c3f321f80c3",
            Kind::Zip(1),
        ),
    };
    Download {
        url: format!("https://nodejs.org/dist/v{NODE_VERSION}/node-v{NODE_VERSION}-{name}"),
        sha256,
        kind,
    }
}

fn typescript() -> Download {
    Download {
        url: format!("https://registry.npmjs.org/typescript/-/typescript-{TYPESCRIPT_VERSION}.tgz"),
        sha256: "10e108c9cf7d5f2879053dff18515fb405abf2ccef63eaaf017d9c571687a1d3",
        kind: Kind::TarGz(1),
    }
}

fn go(p: Platform) -> Download {
    let (name, sha256, kind) = match p {
        Platform::MacArm => (
            "darwin-arm64.tar.gz",
            "76812b213b1b2302c978d28fa52fa92d541704b9e7d9d5db8002c50e4018c4c5",
            Kind::TarGz(0),
        ),
        Platform::MacX64 => (
            "darwin-amd64.tar.gz",
            "587b59182488b23aa6e5fc25110405a3e0e5b38ed2f5b2f46ed13c32aee356fe",
            Kind::TarGz(0),
        ),
        Platform::LinuxArm => (
            "linux-arm64.tar.gz",
            "94f3e30b8e374bc285e7dadc11e0865726b9bc6e85b841ccceaabc0214c6b7c8",
            Kind::TarGz(0),
        ),
        Platform::LinuxX64 => (
            "linux-amd64.tar.gz",
            "ecbadb99091a3f46e31f5f934b068b1864eafa7995211b39eaddf76996045fe5",
            Kind::TarGz(0),
        ),
        Platform::WindowsX64 => (
            "windows-amd64.zip",
            "1314008898bd40df77af4b014f777f08873dbdfbcd3d92308728ee03304fe04f",
            Kind::Zip(0),
        ),
    };
    Download {
        url: format!("https://go.dev/dl/go{GO_VERSION}.{name}"),
        sha256,
        kind,
    }
}

/// The rustup host triple of Rust on this platform.
fn rust_host(p: Platform) -> &'static str {
    match p {
        Platform::MacArm => "aarch64-apple-darwin",
        Platform::MacX64 => "x86_64-apple-darwin",
        Platform::LinuxArm => "aarch64-unknown-linux-gnu",
        Platform::LinuxX64 => "x86_64-unknown-linux-gnu",
        // Brings its own linker: no Visual Studio needed (spec chapter 8, section 2a, rule 3).
        Platform::WindowsX64 => "x86_64-pc-windows-gnu",
    }
}

fn rustup(p: Platform) -> Download {
    let sha256 = match p {
        Platform::MacArm => "ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a",
        Platform::MacX64 => "259e2b84274434085163fe8d556510571772cda2aa6d87ca6aa664f57bc644e3",
        Platform::LinuxArm => "15f6e4ce9f583b929c996c91562bad6d4454f3281de858b02cdfdef615fac433",
        Platform::LinuxX64 => "dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71",
        Platform::WindowsX64 => "6d5b5709addc0122c916d8c810da8d8a7b086a5d64fa805ef404d506392aadc8",
    };
    let file = if p == Platform::WindowsX64 {
        "rustup-init.exe"
    } else {
        "rustup-init"
    };
    Download {
        url: format!(
            "https://static.rust-lang.org/rustup/archive/{RUSTUP_VERSION}/{}/{file}",
            rust_host(p)
        ),
        sha256,
        kind: Kind::Program(if p == Platform::WindowsX64 {
            "rustup-init.exe"
        } else {
            "rustup-init"
        }),
    }
}

/// A tool `wolf setup` knows: its name in messages, its directory and how to install it.
struct Item {
    target: Target,
    name: &'static str,
    version: &'static str,
    dir: &'static str,
}

const ITEMS: &[Item] = &[
    Item {
        target: Target::Ts,
        name: "Node.js",
        version: NODE_VERSION,
        dir: NODE_DIR,
    },
    Item {
        target: Target::Ts,
        name: "TypeScript",
        version: TYPESCRIPT_VERSION,
        dir: TYPESCRIPT_DIR,
    },
    Item {
        target: Target::Go,
        name: "Go",
        version: GO_VERSION,
        dir: GO_DIR,
    },
    Item {
        target: Target::Rust,
        name: "Rust",
        version: RUST_VERSION,
        dir: RUST_DIR,
    },
];

/// Why an installation failed: the tool, the URL and the reason.
struct Failure {
    name: &'static str,
    url: String,
    reason: String,
}

fn failure(item: &Item, url: &str, reason: impl Into<String>) -> Failure {
    Failure {
        name: item.name,
        url: url.to_string(),
        reason: reason.into(),
    }
}

/// `wolf setup [--target t]`: installs what is missing. Returns the diagnostics of failures.
pub(crate) fn install(targets: &[Target]) -> Vec<Diagnostic> {
    let mut out = io::stdout();
    let Some(platform) = platform() else {
        return vec![
            Diagnostic::new(
                codes::E0804,
                format!(
                    "`wolf setup` does not support {} on {}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            )
            .help("install Node.js 22, TypeScript, Go 1.24 and Rust 1.85 or later yourself; `wolf` finds them on `PATH`"),
        ];
    };
    let Some(home) = tools::home() else {
        return vec![Diagnostic::new(
            codes::E0804,
            "the wolfpack home cannot be found: neither `WOLFPACK_HOME` nor the home directory is set",
        )];
    };
    let toolchains = home.join("toolchains");
    let mut diagnostics = Vec::new();
    for item in ITEMS.iter().filter(|i| targets.contains(&i.target)) {
        let dir = toolchains.join(item.dir);
        if dir.is_dir() {
            let _ = writeln!(out, "{} {} is installed", item.name, item.version);
            continue;
        }
        let _ = writeln!(out, "installing {} {}", item.name, item.version);
        let partial = toolchains.join(format!(".partial-{}", item.dir));
        let _ = fs::remove_dir_all(&partial);
        let result = fs::create_dir_all(&partial)
            .map_err(|e| {
                failure(
                    item,
                    "",
                    format!("cannot create `{}`: {e}", partial.display()),
                )
            })
            .and_then(|()| install_item(item, platform, &partial))
            .and_then(|()| {
                fs::rename(&partial, &dir).map_err(|e| {
                    failure(item, "", format!("cannot create `{}`: {e}", dir.display()))
                })
            });
        if let Err(f) = result {
            let _ = fs::remove_dir_all(&partial);
            let mut d = Diagnostic::new(
                codes::E0804,
                format!("{} could not be installed: {}", f.name, f.reason),
            )
            .help("check the network connection and run `wolf setup` again");
            if !f.url.is_empty() {
                d = d.note(format!("downloading {}", f.url));
            }
            diagnostics.push(d);
        } else {
            let _ = writeln!(out, "installed {} {}", item.name, item.version);
        }
    }
    if targets.contains(&Target::Rust)
        && matches!(platform, Platform::MacArm | Platform::MacX64)
        && !command_line_tools()
    {
        let _ = writeln!(
            out,
            "Rust needs the Command Line Tools of Xcode for linking; macOS now asks to install them"
        );
        let _ = Command::new("xcode-select").arg("--install").status();
    }
    diagnostics
}

fn command_line_tools() -> bool {
    Command::new("xcode-select")
        .arg("-p")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn install_item(item: &Item, platform: Platform, dir: &Path) -> Result<(), Failure> {
    match item.target {
        Target::Ts if item.dir == NODE_DIR => fetch(item, &node(platform), dir),
        Target::Ts => fetch(item, &typescript(), dir),
        Target::Go => fetch(item, &go(platform), dir),
        Target::Rust => {
            let download = rustup(platform);
            fetch(item, &download, dir)?;
            let program = match download.kind {
                Kind::Program(name) => dir.join(name),
                _ => dir.join("rustup-init"),
            };
            let _ = writeln!(io::stdout(), "  installing the toolchain with rustup");
            // Quiet, and without the check for another Rust on `PATH`: this one stays private.
            let output = Command::new(&program)
                .args(["-y", "-q", "--no-modify-path", "--profile", "minimal"])
                .args(["--default-toolchain", RUST_VERSION])
                .args(["--default-host", rust_host(platform)])
                .env("RUSTUP_HOME", dir.join("rustup"))
                .env("CARGO_HOME", dir.join("cargo"))
                .env("RUSTUP_INIT_SKIP_PATH_CHECK", "yes")
                .output()
                .map_err(|e| {
                    failure(item, &download.url, format!("rustup could not start: {e}"))
                })?;
            let _ = fs::remove_file(&program);
            if !output.status.success() {
                let text = String::from_utf8_lossy(&output.stderr);
                let last = text
                    .trim()
                    .lines()
                    .last()
                    .unwrap_or("no message")
                    .to_string();
                return Err(failure(
                    item,
                    &download.url,
                    format!("rustup failed: {last}"),
                ));
            }
            if !dir.join("cargo").join("bin").join(exe("cargo")).is_file() {
                return Err(failure(item, &download.url, "rustup did not install cargo"));
            }
            Ok(())
        }
    }
}

/// The URL actually downloaded: in debug builds `WOLF_TEST_SETUP_MIRROR` replaces the origin, so
/// that the tests can make downloads fail.
fn url_of(download: &Download) -> String {
    #[cfg(debug_assertions)]
    if let Some(mirror) = std::env::var_os("WOLF_TEST_SETUP_MIRROR") {
        let path = download
            .url
            .splitn(4, '/')
            .nth(3)
            .unwrap_or_default()
            .to_string();
        return format!("{}/{path}", mirror.to_string_lossy());
    }
    download.url.clone()
}

/// Downloads, checks and unpacks one file into `dir`.
fn fetch(item: &Item, download: &Download, dir: &Path) -> Result<(), Failure> {
    let url = url_of(download);
    let fail = |reason: String| failure(item, &url, reason);
    let response = ureq::get(&url).call().map_err(|e| fail(e.to_string()))?;
    let (_, body) = response.into_parts();
    let mut reader = body.into_reader();
    let file_path = dir.join(".download");
    let mut file = fs::File::create(&file_path).map_err(|e| fail(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| fail(e.to_string()))?;
        if n == 0 {
            break;
        }
        let chunk = buffer.get(..n).unwrap_or_default();
        hasher.update(chunk);
        file.write_all(chunk).map_err(|e| fail(e.to_string()))?;
        total += n as u64;
    }
    drop(file);
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != download.sha256 {
        return Err(fail(format!(
            "the SHA-256 sum of the download is {digest}, expected {}",
            download.sha256
        )));
    }
    let _ = writeln!(io::stdout(), "  downloaded {:.1} MB", total as f64 / 1e6);
    let unpacked = match download.kind {
        Kind::TarGz(strip) => untar(&file_path, dir, strip),
        Kind::Zip(strip) => unzip(&file_path, dir, strip),
        Kind::Program(name) => {
            let target = dir.join(name);
            fs::rename(&file_path, &target).and_then(|()| executable(&target))
        }
    };
    let _ = fs::remove_file(&file_path);
    unpacked.map_err(|e| fail(format!("cannot unpack: {e}")))
}

/// The path of an archive entry below `dir` without its first `strip` components, or `None` for
/// an entry that would end up outside `dir` or is stripped away.
fn entry_path(dir: &Path, name: &Path, strip: usize) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = dir.to_path_buf();
    let mut parts = 0;
    for c in name.components().skip(strip) {
        match c {
            Component::Normal(part) => {
                out.push(part);
                parts += 1;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (parts > 0).then_some(out)
}

pub(crate) fn untar(archive: &Path, dir: &Path, strip: usize) -> io::Result<()> {
    let file = fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.into_owned();
        let Some(target) = entry_path(dir, &name, strip) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
    }
    Ok(())
}

pub(crate) fn unzip(archive: &Path, dir: &Path, strip: usize) -> io::Result<()> {
    let file = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(io::Error::other)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(io::Error::other)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Some(target) = entry_path(dir, &name, strip) else {
            continue;
        };
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&target)?;
        io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
pub(crate) fn executable(_: &Path) -> io::Result<()> {
    Ok(())
}

/// `wolf setup --check`: each tool, where it is found and its version. Returns `true` if every
/// tool of `targets` is found.
pub(crate) fn check(targets: &[Target]) -> bool {
    let mut out = io::stdout();
    let mut complete = true;
    let rows: [(Target, &str, &[&str]); 5] = [
        (Target::Ts, "node", &["--version"]),
        (Target::Ts, "npm", &["--version"]),
        (Target::Ts, "tsc", &["--version"]),
        (Target::Go, "go", &["version"]),
        (Target::Rust, "cargo", &["--version"]),
    ];
    for (target, name, args) in rows {
        if !targets.contains(&target) {
            continue;
        }
        let tool = tools::installed(name)
            .map(|t| (t, "installed by wolf setup"))
            .or_else(|| tools::on_path(name).map(|p| (tool_on_path(p), "on PATH")));
        match tool {
            Some((tool, place)) => {
                let version = tool
                    .command()
                    .args(args)
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "version unknown".to_string());
                // For a script run by `node` (`tsc`), the script is the interesting path.
                let path = tool
                    .prefix
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| tool.program.clone());
                let _ = writeln!(
                    out,
                    "{:<5} {name:<6} {version}  ({place}: {})",
                    target.name(),
                    path.display()
                );
            }
            None => {
                complete = false;
                let _ = writeln!(out, "{:<5} {name:<6} missing", target.name());
            }
        }
    }
    if !complete {
        let _ = writeln!(out, "\nrun `wolf setup` to install the missing tools");
    }
    complete
}

fn tool_on_path(program: PathBuf) -> tools::Tool {
    tools::Tool {
        program,
        prefix: Vec::new(),
        path: Vec::new(),
        env: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_stay_inside_the_directory() {
        let dir = Path::new("/home/wolf/toolchains/x");
        assert_eq!(
            entry_path(dir, Path::new("node-v24/bin/node"), 1),
            Some(dir.join("bin/node"))
        );
        assert_eq!(entry_path(dir, Path::new("node-v24"), 1), None);
        assert_eq!(entry_path(dir, Path::new("a/../../etc/passwd"), 1), None);
        assert_eq!(entry_path(dir, Path::new("/etc/passwd"), 0), None);
        assert_eq!(
            entry_path(dir, Path::new("go/bin/go"), 0),
            Some(dir.join("go/bin/go"))
        );
    }

    #[test]
    fn every_platform_has_every_download() {
        for p in [
            Platform::MacArm,
            Platform::MacX64,
            Platform::LinuxArm,
            Platform::LinuxX64,
            Platform::WindowsX64,
        ] {
            for d in [node(p), go(p), rustup(p), typescript()] {
                assert!(d.url.starts_with("https://"), "{}", d.url);
                assert_eq!(d.sha256.len(), 64, "{}", d.url);
            }
        }
    }
}
