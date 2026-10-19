//! End-to-end tests of the TypeScript backend: `wolf run` builds with `tsc --strict` and runs
//! with Node.js, and the output must match exactly. These tests need `node` and `tsc` on `PATH`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("run")
        .join(name)
}

fn wolf_run(project: &Path, out: &Path, extra: &[&str], path: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wolf"));
    cmd.arg("run")
        .arg("--out")
        .arg(out)
        .args(extra)
        .current_dir(project);
    if let Some(path) = path {
        // Only this PATH: no tools installed by `wolf setup` either.
        cmd.env("PATH", path).env(
            "WOLFPACK_HOME",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-wolfpack-home"),
        );
    }
    cmd.output().unwrap()
}

fn tool_dir(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|d| d.join(name).is_file())
        .unwrap_or_else(|| panic!("`{name}` must be installed to run these tests"))
}

/// Milestone M6: every example runs in Node.js and prints `expected.txt`.
#[test]
fn every_example_runs_and_prints_expected_output() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut projects: Vec<PathBuf> = fs::read_dir(&examples)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wolfpack.toml").is_file())
        .collect();
    projects.sort();
    assert_eq!(projects.len(), 13);
    let mut failures = Vec::new();
    for project in projects {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let extra: &[&str] = if name == "textkit" {
            &["--example", "demo"]
        } else {
            &[]
        };
        let output = wolf_run(&project, &tmp(&name), extra, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let expected = fs::read_to_string(project.join("expected.txt")).unwrap();
        if stdout != expected || output.status.code() != Some(0) {
            failures.push(format!(
                "--- {name}: exit {:?}\nstdout:\n{stdout}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Programs under `tests/run/` with their standard output, standard error and exit code.
#[test]
fn run_programs_match_expected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run");
    let bless = std::env::var_os("WOLF_BLESS").is_some();
    let mut failures = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("E08") {
            continue;
        }
        let output = wolf_run(&dir, &tmp(&name), &[], None);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let code = output.status.code().unwrap_or(-1).to_string();
        if bless {
            fs::write(dir.join("expected.txt"), &stdout).unwrap();
            if stderr.is_empty() {
                let _ = fs::remove_file(dir.join("expected.stderr"));
            } else {
                fs::write(dir.join("expected.stderr"), &stderr).unwrap();
            }
            if code == "0" {
                let _ = fs::remove_file(dir.join("expected.code"));
            } else {
                fs::write(dir.join("expected.code"), &code).unwrap();
            }
            continue;
        }
        let read = |f: &str, default: &str| {
            fs::read_to_string(dir.join(f)).unwrap_or_else(|_| default.to_string())
        };
        let (e_out, e_err, e_code) = (
            read("expected.txt", ""),
            read("expected.stderr", ""),
            read("expected.code", "0"),
        );
        if stdout != e_out || stderr != e_err || code != e_code.trim() {
            failures.push(format!(
                "--- {name}\nexpected (exit {e_code}):\n{e_out}{e_err}\nactual (exit {code}):\n{stdout}{stderr}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// E0801: a tool of the target is missing.
#[test]
fn e0801_missing_tool() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0801_missing_tool");
    let empty = tmp("empty_path");
    fs::create_dir_all(&empty).unwrap();
    let output = wolf_run(&dir, &tmp("E0801"), &[], Some(empty.to_str().unwrap()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// E0802: the target's tool rejects the generated code (simulated by a fake `tsc`).
#[test]
fn e0802_rejected_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0802_rejected_code");
    let fake = tmp("fake_tsc");
    fs::create_dir_all(&fake).unwrap();
    let tsc = fake.join("tsc");
    fs::write(
        &tsc,
        "#!/bin/sh\necho \"main.ts(1,1): error TS0000: simulated rejection\"\nexit 2\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tsc, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", fake.display(), tool_dir("node").display());
    let output = wolf_run(&dir, &tmp("E0802"), &[], Some(&path));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// E0806: `tsc` rejects a project with code in `native/ts/`: here a native function returns a
/// number where its declaration says `String`.
#[test]
fn e0806_rejected_native_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0806_rejected_native");
    let output = wolf_run(&dir, &tmp("E0806"), &[], None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0806]: TypeScript rejected the code of `native/ts/`"),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// E0803: the packages of `[extern.ts.dependencies]` cannot be fetched (simulated by a fake
/// `npm`).
#[test]
fn e0803_dependencies_not_fetched() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0803_dependencies_not_fetched");
    let fake = tmp("fake_npm");
    fs::create_dir_all(&fake).unwrap();
    let npm = fake.join("npm");
    fs::write(
        &npm,
        "#!/bin/sh\necho \"npm error 404 Not Found - no-such-package\" >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&npm, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = fake.display().to_string();
    let output = wolf_run(&dir, &tmp("E0803"), &[], Some(&path));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

fn wolf_test(project: &Path, out: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wolf"))
        .arg("test")
        .args(extra)
        .arg("--out")
        .arg(out)
        .current_dir(project)
        .output()
        .unwrap()
}

/// Milestone M7: the tests of every example pass.
#[test]
fn every_example_passes_its_tests() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut failures = Vec::new();
    for entry in fs::read_dir(&examples).unwrap().flatten() {
        let project = entry.path();
        if !project.join("wolfpack.toml").is_file() {
            continue;
        }
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let output = wolf_test(&project, &tmp(&format!("test-{name}")), &[]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.code() != Some(0) || !stdout.contains("test result: ok.") {
            failures.push(format!(
                "--- {name}\n{stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Projects under `tests/test/`: the report of `wolf test` (and of `wolf test <filter>` when
/// `filter.txt` exists) with its exit code.
#[test]
fn test_reports_match_expected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test");
    let bless = std::env::var_os("WOLF_BLESS").is_some();
    let mut failures = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut runs = vec![(None, "expected.txt")];
        if let Ok(filter) = fs::read_to_string(dir.join("filter.txt")) {
            runs.push((Some(filter.trim().to_string()), "expected_filtered.txt"));
        }
        for (filter, file) in runs {
            let extra: Vec<&str> = filter.iter().map(String::as_str).collect();
            let output = wolf_test(&dir, &tmp(&format!("t-{name}")), &extra);
            let actual = format!(
                "{}exit code: {}\n",
                String::from_utf8_lossy(&output.stdout),
                output.status.code().unwrap_or(-1)
            );
            if bless {
                fs::write(dir.join(file), &actual).unwrap();
                continue;
            }
            let expected = fs::read_to_string(dir.join(file)).unwrap_or_default();
            if actual != expected {
                failures.push(format!(
                    "--- {name} ({file})\nexpected:\n{expected}\nactual:\n{actual}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Tests and test helpers are not part of `wolf build`.
#[test]
fn tests_are_not_in_the_normal_build() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test/mixed");
    let out = tmp("build-mixed");
    let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["build", "--out"])
        .arg(&out)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let math = fs::read_to_string(out.join("src/util/math.ts")).unwrap();
    assert!(
        !math.contains("twice") && !math.contains("$tests"),
        "{math}"
    );
    assert!(!out.join("src/main.test.ts").exists());
}

/// `wolf fmt --check` lists the unformatted files and changes nothing; `wolf fmt` formats them.
#[test]
fn fmt_checks_and_formats_a_project() {
    let dir = tmp("fmt_project");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("wolfpack.toml"),
        "[package]\nname = \"fmt-demo\"\nversion = \"0.1.0\"\nkind = \"cli\"\n",
    )
    .unwrap();
    let messy = "module main\nmain { print(\"hi\") }\n";
    fs::write(dir.join("src/main.wolf"), messy).unwrap();
    let fmt = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wolf"))
            .arg("fmt")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
    };
    let check = fmt(&["--check"]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&check.stdout), "src/main.wolf\n");
    assert_eq!(
        fs::read_to_string(dir.join("src/main.wolf")).unwrap(),
        messy
    );
    let write = fmt(&[]);
    assert_eq!(write.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(dir.join("src/main.wolf")).unwrap(),
        "module main\n\nmain { print(\"hi\") }\n"
    );
    assert_eq!(fmt(&["--check"]).status.code(), Some(0));
}

/// E0804: a download of `wolf setup` fails (the tests send it to a port where nothing listens);
/// nothing half-installed is left behind.
#[test]
fn e0804_setup_cannot_download() {
    let home = tmp("E0804_home");
    let _ = fs::remove_dir_all(&home);
    let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["setup", "--target", "go"])
        .env("WOLFPACK_HOME", &home)
        .env("WOLF_TEST_SETUP_MIRROR", "http://127.0.0.1:9")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0804]: Go could not be installed: "),
        "{stderr}"
    );
    assert!(
        stderr.contains("= note: downloading http://127.0.0.1:9/dl/go"),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
    let left: Vec<_> = fs::read_dir(home.join("toolchains"))
        .unwrap()
        .flatten()
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// E0805: `wolf self update` cannot reach the releases (the tests send it to a port where nothing
/// listens); the running `wolf` stays as it is.
#[test]
fn e0805_update_cannot_download() {
    let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["self", "update", "--version", "0.2.0"])
        .env("WOLF_TEST_UPDATE_MIRROR", "http://127.0.0.1:9")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "error[E0805]: cannot download http://127.0.0.1:9/releases/download/v0.2.0/SHA256SUMS.txt"
        ),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
    let check = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["self", "update", "--check"])
        .env("WOLF_TEST_UPDATE_MIRROR", "http://127.0.0.1:9")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.starts_with("error[E0805]: cannot find the latest release"),
        "{stderr}"
    );
}

/// `wolf setup --check` lists the missing tools and exits with 1.
#[test]
fn setup_check_lists_missing_tools() {
    let empty = tmp("empty_path_check");
    fs::create_dir_all(&empty).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["setup", "--check", "--target", "go"])
        .env("WOLFPACK_HOME", tmp("no_home_check"))
        .env("PATH", &empty)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "go    go     missing\n\nrun `wolf setup` to install the missing tools\n"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// A minimal release server for `wolf self update`: `/releases/latest` redirects to the tag of
/// `version`, and the files of the release are served from memory.
#[cfg(unix)]
fn release_server(version: &str, files: Vec<(String, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let tag = format!("{origin}/releases/tag/v{version}");
    let prefix = format!("/releases/download/v{version}/");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut stream = stream;
            if path == "/releases/latest" {
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nLocation: {tag}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                continue;
            }
            let body = path
                .strip_prefix(&prefix)
                .and_then(|name| files.iter().find(|(n, _)| n == name))
                .map(|(_, b)| b.clone());
            match body {
                Some(b) => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    );
                    let _ = stream.write_all(&b);
                }
                None => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                }
            }
        }
    });
    origin
}

/// `wolf self update` replaces a copy of `wolf` with the release of the server: the archive is
/// checked, its `wolf` runs, and `wolf self update --check` sees the newer release.
#[cfg(unix)]
#[test]
fn self_update_replaces_wolf() {
    use std::os::unix::fs::PermissionsExt;
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", _) => "wolfpack-macos-universal.tar.gz",
        ("linux", "x86_64") => "wolfpack-linux-x64.tar.gz",
        ("linux", "aarch64") => "wolfpack-linux-arm64.tar.gz",
        _ => return,
    };
    // The new `wolf` of the release: a script that answers `--version` and `setup`.
    let script = b"#!/bin/sh\necho wolf 9.9.9\n";
    let mut archive = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut archive, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "wolf", &script[..]).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    use sha2::Digest;
    let sum: String = sha2::Sha256::digest(&archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let sums = format!("{sum}  {asset}\n").into_bytes();
    let origin = release_server(
        "9.9.9",
        vec![
            (asset.to_string(), archive),
            ("SHA256SUMS.txt".to_string(), sums),
        ],
    );

    let dir = tmp("self_update");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin")).unwrap();
    let copy = dir.join("bin/wolf");
    fs::copy(env!("CARGO_BIN_EXE_wolf"), &copy).unwrap();
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o755)).unwrap();

    let check = Command::new(&copy)
        .args(["self", "update", "--check"])
        .env("WOLF_TEST_UPDATE_MIRROR", &origin)
        .output()
        .unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert!(String::from_utf8_lossy(&check.stdout).contains("latest release 9.9.9"));

    let update = Command::new(&copy)
        .args(["self", "update"])
        .env("WOLF_TEST_UPDATE_MIRROR", &origin)
        .env("WOLFPACK_HOME", dir.join("home"))
        .output()
        .unwrap();
    assert!(update.status.success(), "{update:?}");
    let version = Command::new(&copy).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&version.stdout), "wolf 9.9.9\n");
    let left: Vec<_> = fs::read_dir(dir.join("bin")).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "{left:?}");
}
