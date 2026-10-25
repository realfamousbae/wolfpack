//! `wolf self uninstall` (spec chapter 8, section 2c): removes `wolf`, the `PATH` lines that the
//! installers added and, after a question, the wolfpack home.

use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use wolf_diag::{Diagnostic, codes};

use crate::tools;
use crate::update::{self, Install};

/// The comment that the terminal scripts write before the line that puts `wolf` on `PATH`.
const MARK: &str = "# Added by the wolfpack installer.";

/// The entries of the wolfpack home that `wolf` writes (spec chapter 8, sections 2a and 2b).
const HOME_ENTRIES: [&str; 3] = ["toolchains", "update-check.json", "config.toml"];

fn fail(message: impl Into<String>) -> Box<Diagnostic> {
    Box::new(Diagnostic::new(codes::E0808, message.into()))
}

fn say(text: &str) {
    let _ = writeln!(io::stderr(), "{text}");
}

/// Removes `wolf`; `yes` removes the wolfpack home without asking.
pub(crate) fn uninstall(yes: bool) -> Result<(), Box<Diagnostic>> {
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| fail(format!("cannot find the running wolf: {e}")))?;
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| tools::home().and_then(|h| h.parent().map(|p| p.join(".cargo"))));
    // A `wolf` of a package manager is removed by it (rule 3).
    let manager = match update::install_kind(&exe, cargo_home.as_deref()) {
        Install::Cargo => Some(("Cargo", "cargo uninstall wolfpack-lang".to_string())),
        Install::Homebrew => Some(("Homebrew", "brew uninstall wolfpack".to_string())),
        Install::Scoop => Some(("Scoop", "scoop uninstall wolfpack".to_string())),
        Install::Winget => Some((
            "winget",
            "winget uninstall realfamousbae.wolfpack".to_string(),
        )),
        Install::System => Some((
            "a package of the system",
            "sudo apt remove wolfpack` or `sudo dnf remove wolfpack".to_string(),
        )),
        Install::Own(_) => None,
    };
    if let Some((by, command)) = manager {
        say(&format!(
            "this wolf was installed by {by}; remove it with `{command}`"
        ));
        return home(yes);
    }
    let dir = exe
        .parent()
        .ok_or_else(|| fail("the running wolf has no directory"))?
        .to_path_buf();
    // The setup program of Windows has an uninstaller of its own (rule 4).
    if let Some(uninstaller) = dir.parent().map(|p| p.join("unins000.exe")) {
        if cfg!(windows) && uninstaller.is_file() {
            home(yes)?;
            say(&format!("starting {}", uninstaller.display()));
            return Command::new(&uninstaller)
                .spawn()
                .map(|_| ())
                .map_err(|e| fail(format!("cannot start {}: {e}", uninstaller.display())));
        }
    }
    // The order of rule 7: the lines, the home, `wolf` itself.
    remove_path_lines(&dir)?;
    home(yes)?;
    remove_wolf(&exe, &dir)?;
    say(&format!(
        "removed wolf {} from {}; a new terminal no longer finds it",
        update::Version::current(),
        dir.display()
    ));
    Ok(())
}

/// The `PATH` lines of the terminal scripts (rule 1): the marked lines of the shell profiles and
/// `wolfpack.fish` of fish, or the entry in the user's `Path` on Windows.
fn remove_path_lines(dir: &Path) -> Result<(), Box<Diagnostic>> {
    if cfg!(windows) {
        return remove_windows_path(dir);
    }
    let Some(user) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(());
    };
    let zdot = std::env::var_os("ZDOTDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| user.clone());
    let line = profile_line(dir, &user);
    let profiles = [
        user.join(".profile"),
        user.join(".bashrc"),
        user.join(".bash_profile"),
        zdot.join(".zshrc"),
        zdot.join(".zprofile"),
    ];
    for profile in profiles {
        remove_marked(&profile, &line, false)?;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| user.join(".config"));
    let fish = config.join("fish").join("conf.d").join("wolfpack.fish");
    let fish_line = format!("fish_add_path --global \"{}\"", dir.display());
    remove_marked(&fish, &fish_line, true)
}

/// The line of `install.sh` for the directory `dir`: with `$HOME` when `dir` is in it.
pub(crate) fn profile_line(dir: &Path, user: &Path) -> String {
    let shown = match dir.strip_prefix(user) {
        Ok(rest) => format!("$HOME/{}", rest.display()),
        Err(_) => dir.display().to_string(),
    };
    format!("export PATH=\"{shown}:$PATH\"")
}

/// Removes every [`MARK`] followed by `line`, and the empty line before it, from `file`; a file
/// that is left empty is removed when `whole` is set. Other lines are never changed.
fn remove_marked(file: &Path, line: &str, whole: bool) -> Result<(), Box<Diagnostic>> {
    let Ok(text) = fs::read_to_string(file) else {
        return Ok(());
    };
    let Some(left) = without_marked(&text, line) else {
        return Ok(());
    };
    let result = if whole && left.trim().is_empty() {
        fs::remove_file(file)
    } else {
        fs::write(file, left)
    };
    result.map_err(|e| fail(format!("cannot change {}: {e}", file.display())))?;
    say(&format!("removed the PATH line from {}", file.display()));
    Ok(())
}

/// `text` without the lines of the installer, if it has any.
pub(crate) fn without_marked(text: &str, line: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut keep: Vec<&str> = Vec::new();
    let mut found = false;
    let mut i = 0;
    while let Some(&l) = lines.get(i) {
        if l.trim_end() == MARK && lines.get(i + 1).is_some_and(|n| n.trim_end() == line) {
            // The script writes an empty line, the comment and the line.
            if keep.last().is_some_and(|k| k.trim().is_empty()) {
                keep.pop();
            }
            found = true;
            i += 2;
            continue;
        }
        keep.push(l);
        i += 1;
    }
    found.then(|| keep.join("\n"))
}

/// Removes `dir` from the user's `Path` in the registry, as `install.ps1` added it.
fn remove_windows_path(dir: &Path) -> Result<(), Box<Diagnostic>> {
    const SCRIPT: &str = "$dir = $env:WOLFPACK_UNINSTALL_DIR
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
try {
    $path = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $entries = @($path -split ';' | Where-Object { $_ -ne '' })
    $kept = @($entries | Where-Object { $_ -ne $dir })
    if ($kept.Count -ne $entries.Count) {
        $key.SetValue('Path', ($kept -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
        [Environment]::SetEnvironmentVariable('WOLFPACK_PATH_CHANGED', '1', 'User')
        [Environment]::SetEnvironmentVariable('WOLFPACK_PATH_CHANGED', $null, 'User')
        'removed'
    }
} finally {
    $key.Close()
}";
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("WOLFPACK_UNINSTALL_DIR", dir)
        .output()
        .map_err(|e| fail(format!("cannot change the user's Path: {e}")))?;
    if !output.status.success() {
        return Err(fail(format!(
            "cannot change the user's Path: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if String::from_utf8_lossy(&output.stdout).contains("removed") {
        say(&format!("removed {} from the user's Path", dir.display()));
    }
    Ok(())
}

/// The wolfpack home (rule 2): removed after a question, or with `yes`; kept without a terminal.
fn home(yes: bool) -> Result<(), Box<Diagnostic>> {
    let Some(home) = tools::home().filter(|h| h.is_dir()) else {
        return Ok(());
    };
    let shown = shown_path(&home);
    let remove = if yes {
        true
    } else if io::stdin().is_terminal() && io::stderr().is_terminal() {
        let mut err = io::stderr();
        let _ = write!(
            err,
            "Remove {shown} too, with the tools of the targets ({})? [y/N] ",
            size_text(size(&home))
        );
        let _ = err.flush();
        let mut answer = String::new();
        let _ = io::stdin().lock().read_line(&mut answer);
        answer.trim().eq_ignore_ascii_case("y")
    } else {
        say(&format!(
            "kept {shown}; remove it with `wolf self uninstall --yes`, or by hand"
        ));
        return Ok(());
    };
    if !remove {
        say(&format!("kept {shown}"));
        return Ok(());
    }
    for entry in HOME_ENTRIES {
        let path = home.join(entry);
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else if path.exists() {
            fs::remove_file(&path)
        } else {
            continue;
        };
        result.map_err(|e| fail(format!("cannot remove {}: {e}", path.display())))?;
    }
    let others: Vec<String> = fs::read_dir(&home)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    if others.is_empty() {
        fs::remove_dir(&home)
            .map_err(|e| fail(format!("cannot remove {}: {e}", home.display())))?;
        say(&format!("removed {shown}"));
    } else {
        say(&format!(
            "removed the tools from {shown}; kept what wolf did not put there: {}",
            others.join(", ")
        ));
    }
    Ok(())
}

/// A path with `~` for the home directory of the user.
fn shown_path(path: &Path) -> String {
    let user = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    match user.and_then(|u| path.strip_prefix(&u).ok().map(Path::to_path_buf)) {
        Some(rest) if !cfg!(windows) => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// The bytes of the files under `path`.
fn size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// `1.3 GB`, `250 MB`, `12 KB`.
pub(crate) fn size_text(bytes: u64) -> String {
    const KB: u64 = 1000;
    const MB: u64 = 1000 * KB;
    const GB: u64 = 1000 * MB;
    if bytes >= GB {
        let tenths = (bytes + GB / 20) / (GB / 10);
        format!("{}.{} GB", tenths / 10, tenths % 10)
    } else if bytes >= MB {
        format!("{} MB", (bytes + MB / 2) / MB)
    } else {
        format!("{} KB", bytes.div_ceil(KB))
    }
}

/// Removes the running `wolf` (rules 5 and 6) and, when it is then empty, the directory of the
/// Windows installer script.
fn remove_wolf(exe: &Path, dir: &Path) -> Result<(), Box<Diagnostic>> {
    let cannot = |e: io::Error| {
        let package = cfg!(target_os = "macos") && exe == Path::new("/usr/local/bin/wolf");
        let commands = if package {
            "`sudo rm /usr/local/bin/wolf` and `sudo pkgutil --forget io.github.realfamousbae.wolfpack`".to_string()
        } else {
            format!("`sudo rm {}`", exe.display())
        };
        Diagnostic::new(
            codes::E0808,
            format!("cannot remove wolf from {}: {e}", dir.display()),
        )
        .help(format!("remove it with {commands}"))
    };
    if cfg!(windows) {
        // A running program cannot be removed on Windows, but it can be renamed; a command
        // started now removes it a moment after `wolf` exits.
        let old = exe.with_file_name("wolf.old.exe");
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old).map_err(|e| Box::new(cannot(e)))?;
        let ours = dir.ends_with(Path::new("Programs").join("wolfpack").join("bin"));
        let mut script = format!("ping -n 3 127.0.0.1 >nul & del /f /q \"{}\"", old.display());
        if ours {
            script.push_str(&format!(" & rmdir \"{}\"", dir.display()));
            if let Some(parent) = dir.parent() {
                script.push_str(&format!(" & rmdir \"{}\"", parent.display()));
            }
        }
        hidden(Command::new("cmd").args(["/c", &script]))
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                fail(format!(
                    "cannot start the removal of {}: {e}",
                    old.display()
                ))
            })
    } else {
        fs::remove_file(exe).map_err(|e| Box::new(cannot(e)))
    }
}

/// A command without a window of its own on Windows.
fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW)
    }
    #[cfg(not(windows))]
    {
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_of_the_installer_are_removed_and_nothing_else() {
        let line = "export PATH=\"$HOME/.local/bin:$PATH\"";
        let text =
            format!("alias ll='ls -l'\nexport PATH=\"$HOME/.local/bin:$PATH\"\n\n{MARK}\n{line}\n");
        assert_eq!(
            without_marked(&text, line).as_deref(),
            Some("alias ll='ls -l'\nexport PATH=\"$HOME/.local/bin:$PATH\"\n")
        );
        // A marked line for another directory, and an unmarked one, stay.
        let other = format!("{MARK}\nexport PATH=\"/opt/wolf:$PATH\"\n");
        assert_eq!(without_marked(&other, line), None);
        assert_eq!(without_marked(line, line), None);
    }

    #[test]
    fn the_line_names_the_directory_with_home() {
        let user = Path::new("/home/ann");
        assert_eq!(
            profile_line(Path::new("/home/ann/.local/bin"), user),
            "export PATH=\"$HOME/.local/bin:$PATH\""
        );
        assert_eq!(
            profile_line(Path::new("/opt/wolf"), user),
            "export PATH=\"/opt/wolf:$PATH\""
        );
    }

    #[test]
    fn sizes_are_short() {
        assert_eq!(size_text(1_340_000_000), "1.3 GB");
        assert_eq!(size_text(250_400_000), "250 MB");
        assert_eq!(size_text(1_200), "2 KB");
    }
}
