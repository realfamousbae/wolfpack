//! `wolf self uninstall` (spec chapter 8, section 2c), run on a copy of `wolf` with a home
//! directory of its own: the marked `PATH` lines go, other lines stay, the wolfpack home is kept
//! without a terminal and removed with `--yes`, and `wolf` itself is removed.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg_attr(windows, allow(dead_code))]
const MARK: &str = "# Added by the wolfpack installer.";

/// A fresh directory for one test.
fn tmp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("uninstall-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A home directory with a copy of `wolf` in `~/.local/bin`, profiles with the installer's
/// lines and a wolfpack home with tools.
#[cfg_attr(windows, allow(dead_code))]
fn installed(name: &str) -> (PathBuf, PathBuf) {
    let home = tmp(name);
    let bin = home.join(".local").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wolf = bin.join(if cfg!(windows) { "wolf.exe" } else { "wolf" });
    fs::copy(env!("CARGO_BIN_EXE_wolf"), &wolf).unwrap();
    let line = "export PATH=\"$HOME/.local/bin:$PATH\"";
    fs::write(
        home.join(".profile"),
        format!("alias ll='ls -l'\n\n{MARK}\n{line}\n"),
    )
    .unwrap();
    fs::write(home.join(".bashrc"), format!("export EDITOR=vi\n{line}\n")).unwrap();
    let fish = home.join(".config").join("fish").join("conf.d");
    fs::create_dir_all(&fish).unwrap();
    fs::write(
        fish.join("wolfpack.fish"),
        format!("\n{MARK}\nfish_add_path --global \"{}\"\n", bin.display()),
    )
    .unwrap();
    let tools = home.join(".wolfpack").join("toolchains").join("node");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("node"), "not really node").unwrap();
    fs::write(home.join(".wolfpack").join("update-check.json"), "{}").unwrap();
    (home, wolf)
}

fn uninstall(home: &Path, wolf: &Path, args: &[&str]) -> Output {
    Command::new(wolf)
        .args(["self", "uninstall"])
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("WOLFPACK_HOME")
        .env_remove("ZDOTDIR")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("CARGO_HOME")
        .env("WOLFPACK_NO_UPDATE_CHECK", "1")
        .output()
        .unwrap()
}

#[test]
#[cfg(unix)]
fn the_installed_wolf_and_its_lines_are_removed() {
    let (home, wolf) = installed("lines");
    let out = uninstall(&home, &wolf, &[]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(!wolf.exists(), "{err}");
    // The marked line went with the empty line before it; the unmarked line stays.
    assert_eq!(
        fs::read_to_string(home.join(".profile")).unwrap(),
        "alias ll='ls -l'\n"
    );
    assert_eq!(
        fs::read_to_string(home.join(".bashrc")).unwrap(),
        "export EDITOR=vi\nexport PATH=\"$HOME/.local/bin:$PATH\"\n"
    );
    assert!(!home.join(".config/fish/conf.d/wolfpack.fish").exists());
    // Without a terminal and without `--yes` the home stays.
    assert!(home.join(".wolfpack/toolchains").is_dir(), "{err}");
    assert!(err.contains("kept ~/.wolfpack"), "{err}");
    // `~/.local/bin` is shared: it stays.
    assert!(home.join(".local/bin").is_dir());
}

#[test]
#[cfg(unix)]
fn yes_removes_the_home_but_not_what_wolf_did_not_put_there() {
    let (home, wolf) = installed("yes");
    fs::write(home.join(".wolfpack").join("notes.txt"), "mine").unwrap();
    let out = uninstall(&home, &wolf, &["--yes"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(!home.join(".wolfpack/toolchains").exists(), "{err}");
    assert!(!home.join(".wolfpack/update-check.json").exists());
    assert!(home.join(".wolfpack/notes.txt").is_file());
    assert!(err.contains("notes.txt"), "{err}");
    fs::remove_file(home.join(".wolfpack/notes.txt")).unwrap();
    // A second home without other files goes away entirely.
    let (home, wolf) = installed("yes-empty");
    let out = uninstall(&home, &wolf, &["--yes"]);
    assert!(out.status.success());
    assert!(!home.join(".wolfpack").exists());
}

/// E0808: a directory that cannot be written keeps `wolf`, and the other steps are done.
#[test]
#[cfg(unix)]
fn e0808_wolf_cannot_be_removed() {
    use std::os::unix::fs::PermissionsExt;
    let (home, wolf) = installed("e0808");
    let bin = wolf.parent().unwrap().to_path_buf();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();
    let out = uninstall(&home, &wolf, &[]);
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains("error[E0808]: cannot remove wolf from"),
        "{err}"
    );
    assert!(err.contains("sudo rm"), "{err}");
    assert!(wolf.exists());
    assert_eq!(
        fs::read_to_string(home.join(".profile")).unwrap(),
        "alias ll='ls -l'\n"
    );
}

/// Windows, as `install.ps1` installs: `wolf.exe` in `Programs\wolfpack\bin` under the local
/// application data and that directory in the user's `Path`. The test changes the registry of the
/// user, so it runs only where `WOLFPACK_TEST_WINDOWS_PATH` is set (CI).
#[test]
#[cfg(windows)]
fn windows_the_path_entry_and_the_directory_are_removed() {
    use std::time::{Duration, Instant};
    if std::env::var_os("WOLFPACK_TEST_WINDOWS_PATH").is_none() {
        eprintln!("skipped: set WOLFPACK_TEST_WINDOWS_PATH to change the user's Path");
        return;
    }
    // The installer writes the directory without the `\\?\` of a canonical path.
    let home = PathBuf::from(tmp("windows").to_string_lossy().trim_start_matches(r"\\?\"));
    let programs = home.join("AppData").join("Local").join("Programs");
    let bin = programs.join("wolfpack").join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wolf = bin.join("wolf.exe");
    fs::copy(env!("CARGO_BIN_EXE_wolf"), &wolf).unwrap();
    let tools = home.join(".wolfpack").join("toolchains").join("node");
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("node.exe"), "not really node").unwrap();
    let entry = bin.display().to_string();
    let path = |script: &str| {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("WOLFPACK_TEST_DIR", &entry)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let read = "[Environment]::GetEnvironmentVariable('Path', 'User')";
    path(&format!(
        "[Environment]::SetEnvironmentVariable('Path', ({read}) + ';' + $env:WOLFPACK_TEST_DIR, 'User')"
    ));
    assert!(path(read).split(';').any(|e| e.trim() == entry));

    let out = uninstall(&home, &wolf, &["--yes"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("from the user's Path"), "{err}");
    assert!(!path(read).split(';').any(|e| e.trim() == entry), "{err}");
    assert!(!home.join(".wolfpack").exists(), "{err}");
    // `wolf.exe` is removed a moment after it exits, then its two directories.
    let start = Instant::now();
    while programs.join("wolfpack").exists() && start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(!programs.join("wolfpack").exists(), "{err}");
}
