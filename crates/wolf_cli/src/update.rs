//! `wolf self update` and the daily question at start (spec chapter 8, section 2b).

use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use wolf_diag::{Diagnostic, codes};

use crate::{setup, tools};

/// The repository whose releases are `wolf`.
const REPOSITORY: &str = "https://github.com/realfamousbae/wolfpack";

/// How long the answer of the background check is used.
const DAY: u64 = 24 * 60 * 60;

/// A release version: three numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u64, u64, u64);

impl Version {
    /// `1.2.3` or `v1.2.3`; anything else (a prerelease such as `1.2.3-rc.1`) is `None`.
    pub(crate) fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.').map(|p| p.parse::<u64>().ok());
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }

    /// The version of this `wolf`.
    pub(crate) fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The origin of the releases: in debug builds `WOLF_TEST_UPDATE_MIRROR` replaces GitHub, so
/// that the tests can serve releases.
fn origin() -> String {
    #[cfg(debug_assertions)]
    if let Some(mirror) = std::env::var_os("WOLF_TEST_UPDATE_MIRROR") {
        return mirror.to_string_lossy().trim_end_matches('/').to_string();
    }
    REPOSITORY.to_string()
}

/// The version of a `.../releases/tag/v1.2.3` address.
fn tag_version(location: &str) -> Option<Version> {
    let (_, tag) = location.trim_end_matches('/').rsplit_once("/tag/")?;
    Version::parse(tag)
}

/// The latest release: the redirect of `/releases/latest` names its tag.
pub(crate) fn latest() -> Result<Version, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let url = format!("{}/releases/latest", origin());
    let response = agent.get(&url).call().map_err(|e| e.to_string())?;
    let location = response
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .ok_or_else(|| {
            format!(
                "{url} did not name a release (status {})",
                response.status()
            )
        })?;
    tag_version(location).ok_or_else(|| format!("{url} named `{location}`, not a release"))
}

/// How this `wolf` was installed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Install {
    /// By an installer or by hand: `wolf` replaces this file itself.
    Own(PathBuf),
    /// By `cargo install`.
    Cargo,
    /// By Homebrew.
    Homebrew,
}

/// How the `wolf` at `exe` was installed; `cargo_home` is `CARGO_HOME` or `~/.cargo`.
pub(crate) fn install_kind(exe: &Path, cargo_home: Option<&Path>) -> Install {
    let text = exe.to_string_lossy();
    if cargo_home.is_some_and(|c| exe.starts_with(c.join("bin"))) {
        return Install::Cargo;
    }
    if ["/opt/homebrew/", "/Cellar/", "/linuxbrew/"]
        .iter()
        .any(|p| text.contains(p))
    {
        return Install::Homebrew;
    }
    Install::Own(exe.to_path_buf())
}

/// The release archive for this system.
fn asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => Some("wolfpack-macos-universal.tar.gz"),
        ("linux", "x86_64") => Some("wolfpack-linux-x64.tar.gz"),
        ("linux", "aarch64") => Some("wolfpack-linux-arm64.tar.gz"),
        ("windows", "x86_64" | "aarch64") => Some("wolfpack-windows-x64.zip"),
        _ => None,
    }
}

fn fail(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(codes::E0805, message.into())
}

/// Downloads `url` into memory.
fn download(url: &str) -> Result<Vec<u8>, Box<Diagnostic>> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| fail(format!("cannot download {url}: {e}")))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| fail(format!("cannot download {url}: {e}")))?;
    Ok(bytes)
}

/// The SHA-256 sum of `asset` in a `SHA256SUMS.txt`.
fn listed_sum(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == asset).then(|| sum.to_lowercase())
    })
}

/// `wolf self update [--version v]`: replaces this `wolf` with a release. Returns the new
/// version, or `None` when this one is already the latest.
pub(crate) fn update(wanted: Option<&str>) -> Result<Option<Version>, Box<Diagnostic>> {
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| fail(format!("cannot find the running wolf: {e}")))?;
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| tools::home().and_then(|h| h.parent().map(|p| p.join(".cargo"))));
    match install_kind(&exe, cargo_home.as_deref()) {
        Install::Cargo => {
            return Err(Box::new(
                fail("this wolf was installed by Cargo")
                    .help("update it with `cargo install --locked wolfpack-lang`"),
            ));
        }
        Install::Homebrew => {
            return Err(Box::new(
                fail("this wolf was installed by Homebrew")
                    .help("update it with `brew upgrade wolfpack`"),
            ));
        }
        Install::Own(_) => {}
    }
    let current = Version::current();
    let target = match wanted {
        Some(text) => Version::parse(text)
            .ok_or_else(|| fail(format!("`{text}` is not a release version such as 0.2.0")))?,
        None => latest().map_err(|e| fail(format!("cannot find the latest release: {e}")))?,
    };
    if wanted.is_none() && target <= current {
        return Ok(None);
    }
    let asset = asset().ok_or_else(|| {
        fail(format!(
            "there is no release of wolf for {} on {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
        .help("build it from source: `cargo install --locked wolfpack-lang`")
    })?;
    let base = format!("{}/releases/download/v{target}", origin());
    let mut out = io::stderr();
    let _ = writeln!(out, "downloading wolf {target}");
    let sums = download(&format!("{base}/SHA256SUMS.txt"))?;
    let sums = String::from_utf8_lossy(&sums);
    let want = listed_sum(&sums, asset)
        .ok_or_else(|| fail(format!("SHA256SUMS.txt of {target} does not list {asset}")))?;
    let archive = download(&format!("{base}/{asset}"))?;
    let got: String = Sha256::digest(&archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != want {
        return Err(Box::new(
            fail(format!(
                "the download of {asset} is damaged (SHA-256 {got}, expected {want})"
            ))
            .help("run `wolf self update` again"),
        ));
    }
    let dir = exe
        .parent()
        .ok_or_else(|| fail("the running wolf has no directory"))?;
    replace(&exe, dir, asset, &archive)?;
    let _ = writeln!(out, "installed wolf {target} into {}", dir.display());
    // The tools of the new version (spec chapter 8, section 2a).
    let setup = Command::new(&exe).arg("setup").status();
    if !setup.is_ok_and(|s| s.success()) {
        let _ = writeln!(
            out,
            "some tools could not be installed; run `wolf setup` again later"
        );
    }
    Ok(Some(target))
}

/// Unpacks the new `wolf` next to `exe`, checks that it runs and puts it in the place of `exe`.
fn replace(exe: &Path, dir: &Path, asset: &str, archive: &[u8]) -> Result<(), Box<Diagnostic>> {
    let work = dir.join(format!(".wolf-update-{}", std::process::id()));
    let cannot_write = |e: io::Error| {
        fail(format!("cannot write into {}: {e}", dir.display())).help(
            "install the new release with the installer instead: https://github.com/realfamousbae/wolfpack#install",
        )
    };
    fs::create_dir_all(&work).map_err(cannot_write)?;
    let result = (|| -> Result<(), Box<Diagnostic>> {
        let file = work.join(asset);
        fs::write(&file, archive).map_err(cannot_write)?;
        let unpacked = if asset.ends_with(".zip") {
            setup::unzip(&file, &work, 0)
        } else {
            setup::untar(&file, &work, 0)
        };
        unpacked.map_err(|e| fail(format!("cannot unpack {asset}: {e}")))?;
        let new = work.join(tools::exe("wolf"));
        if !new.is_file() {
            return Err(Box::new(fail(format!("{asset} does not contain wolf"))));
        }
        setup::executable(&new).map_err(cannot_write)?;
        let runs = Command::new(&new)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !runs {
            return Err(Box::new(fail(
                "the downloaded wolf does not run on this system",
            )));
        }
        if cfg!(windows) {
            // A running program cannot be overwritten on Windows, but it can be renamed.
            let old = exe.with_file_name("wolf.old.exe");
            let _ = fs::remove_file(&old);
            fs::rename(exe, &old).map_err(cannot_write)?;
            if let Err(e) = fs::rename(&new, exe) {
                let _ = fs::rename(&old, exe);
                return Err(Box::new(cannot_write(e)));
            }
        } else {
            fs::rename(&new, exe).map_err(cannot_write)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&work);
    result
}

// ----- the daily question ---------------------------------------------------------------------

/// What the background check remembers, in `<home>/update-check.json`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    /// When GitHub was last asked (seconds since 1970).
    pub(crate) checked: u64,
    /// The latest release it named.
    pub(crate) latest: Option<Version>,
    /// The release the user was last asked about, and when.
    pub(crate) asked: Option<(Version, u64)>,
}

impl State {
    pub(crate) fn parse(text: &str) -> State {
        let field = |name: &str| -> Option<String> {
            let key = format!("\"{name}\":");
            let start = text.find(&key)? + key.len();
            let rest = text.get(start..)?.trim_start();
            let end = rest.find([',', '}', '\n']).unwrap_or(rest.len());
            Some(rest.get(..end)?.trim().trim_matches('"').to_string())
        };
        let number = |name: &str| field(name).and_then(|v| v.parse::<u64>().ok());
        let version = |name: &str| field(name).and_then(|v| Version::parse(&v));
        State {
            checked: number("checked").unwrap_or(0),
            latest: version("latest"),
            asked: version("asked_version").zip(number("asked_at")),
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut fields = vec![format!("  \"checked\": {}", self.checked)];
        if let Some(v) = self.latest {
            fields.push(format!("  \"latest\": \"{v}\""));
        }
        if let Some((v, at)) = self.asked {
            fields.push(format!("  \"asked_version\": \"{v}\""));
            fields.push(format!("  \"asked_at\": {at}"));
        }
        format!("{{\n{}\n}}\n", fields.join(",\n"))
    }
}

fn state_file() -> Option<PathBuf> {
    tools::home().map(|h| h.join("update-check.json"))
}

fn load() -> State {
    state_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .map(|t| State::parse(&t))
        .unwrap_or_default()
}

fn save(state: &State) {
    if let Some(file) = state_file() {
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(file, state.render());
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether the checks of section 2b, rule 6 allow asking and fetching at all.
fn allowed() -> bool {
    let env_set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    if env_set("CI") || std::env::var_os("WOLFPACK_NO_UPDATE_CHECK").is_some_and(|v| v == "1") {
        return false;
    }
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        return false;
    }
    let config = tools::home()
        .and_then(|h| fs::read_to_string(h.join("config.toml")).ok())
        .unwrap_or_default();
    !check_disabled(&config)
}

/// Whether `config.toml` sets `update.check = false` (also as `check = false` in `[update]`).
pub(crate) fn check_disabled(config: &str) -> bool {
    let mut section = String::new();
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']).trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let full = if section.is_empty() {
            key.to_string()
        } else {
            format!("{section}.{key}")
        };
        if full == "update.check" && value.trim() == "false" {
            return true;
        }
    }
    false
}

/// Whether to ask about the latest release now (section 2b, rule 5).
pub(crate) fn should_ask(state: &State, current: Version, now: u64) -> Option<Version> {
    let latest = state.latest.filter(|l| *l > current)?;
    match state.asked {
        Some((v, at)) if v == latest && now.saturating_sub(at) < DAY => None,
        _ => Some(latest),
    }
}

/// `wolf self update --refresh-cache`: the background check.
pub(crate) fn refresh() {
    let mut state = load();
    state.checked = now();
    if let Ok(v) = latest() {
        state.latest = Some(v);
    }
    save(&state);
}

/// Before a command: starts the background check when it is due, and asks about a newer release.
/// Returns the exit code when the command was run again by the new `wolf`.
pub(crate) fn at_start() -> Option<std::process::ExitCode> {
    if !allowed() {
        return None;
    }
    let mut state = load();
    let time = now();
    if time.saturating_sub(state.checked) >= DAY {
        if let Ok(exe) = std::env::current_exe() {
            let _ = Command::new(exe)
                .args(["self", "update", "--refresh-cache"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
    let latest = should_ask(&state, Version::current(), time)?;
    state.asked = Some((latest, time));
    save(&state);
    let mut err = io::stderr();
    let _ = write!(
        err,
        "wolf {latest} is available (you have {}). Update now? [y/N] ",
        Version::current()
    );
    let _ = err.flush();
    let mut answer = String::new();
    let _ = io::stdin().lock().read_line(&mut answer);
    if !answer.trim().eq_ignore_ascii_case("y") {
        return None;
    }
    match update(Some(&latest.to_string())) {
        Ok(_) => {
            // The command that was asked for, with the new `wolf`.
            let exe = std::env::current_exe().ok()?;
            let status = Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env("WOLFPACK_NO_UPDATE_CHECK", "1")
                .status()
                .ok()?;
            let code = status
                .code()
                .and_then(|c| u8::try_from(c).ok())
                .unwrap_or(1);
            Some(std::process::ExitCode::from(code))
        }
        Err(d) => {
            crate::render_plain(&[*d]);
            None
        }
    }
}

/// Removes the `wolf.old.exe` that an update on Windows leaves behind.
pub(crate) fn clean_old() {
    if cfg!(windows) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = fs::remove_file(exe.with_file_name("wolf.old.exe"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_three_numbers() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse("1.10.3"), Some(Version(1, 10, 3)));
        assert_eq!(Version::parse("1.2.3-rc.1"), None);
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert_eq!(Version(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn the_latest_tag_is_read_from_the_redirect() {
        let v = tag_version("https://github.com/realfamousbae/wolfpack/releases/tag/v0.3.1");
        assert_eq!(v, Some(Version(0, 3, 1)));
        assert_eq!(
            tag_version("https://github.com/realfamousbae/wolfpack/releases"),
            None
        );
    }

    #[test]
    fn package_managers_update_their_own_wolf() {
        let cargo = Path::new("/home/ann/.cargo");
        assert_eq!(
            install_kind(Path::new("/home/ann/.cargo/bin/wolf"), Some(cargo)),
            Install::Cargo
        );
        assert_eq!(
            install_kind(
                Path::new("/opt/homebrew/Cellar/wolfpack/0.2.0/bin/wolf"),
                None
            ),
            Install::Homebrew
        );
        let own = Path::new("/home/ann/.local/bin/wolf");
        assert_eq!(
            install_kind(own, Some(cargo)),
            Install::Own(own.to_path_buf())
        );
    }

    #[test]
    fn sums_are_found_by_name() {
        let sums = "abc  wolfpack-linux-x64.tar.gz\nDEF *wolfpack-windows-x64.zip\n";
        assert_eq!(
            listed_sum(sums, "wolfpack-linux-x64.tar.gz").as_deref(),
            Some("abc")
        );
        assert_eq!(
            listed_sum(sums, "wolfpack-windows-x64.zip").as_deref(),
            Some("def")
        );
        assert_eq!(listed_sum(sums, "wolfpack-macos-universal.tar.gz"), None);
    }

    #[test]
    fn the_state_survives_a_round_trip() {
        let state = State {
            checked: 100,
            latest: Some(Version(0, 3, 0)),
            asked: Some((Version(0, 3, 0), 90)),
        };
        assert_eq!(State::parse(&state.render()), state);
        assert_eq!(State::parse("not json"), State::default());
    }

    #[test]
    fn the_same_release_is_asked_about_once_a_day() {
        let current = Version(0, 2, 0);
        let mut state = State {
            checked: 0,
            latest: Some(Version(0, 3, 0)),
            asked: None,
        };
        assert_eq!(should_ask(&state, current, 1000), Some(Version(0, 3, 0)));
        state.asked = Some((Version(0, 3, 0), 1000));
        assert_eq!(should_ask(&state, current, 1000 + DAY - 1), None);
        assert_eq!(
            should_ask(&state, current, 1000 + DAY),
            Some(Version(0, 3, 0))
        );
        assert_eq!(should_ask(&state, Version(0, 3, 0), 0), None);
    }

    #[test]
    fn the_check_can_be_turned_off() {
        assert!(check_disabled("[update]\ncheck = false\n"));
        assert!(check_disabled("update.check = false # quiet\n"));
        assert!(!check_disabled("[update]\ncheck = true\n"));
        assert!(!check_disabled("[other]\ncheck = false\n"));
    }
}
