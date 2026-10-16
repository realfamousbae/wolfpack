//! Where `wolf` finds the tools of the targets (spec chapter 8, section 2a): first among those
//! that `wolf setup` installed in the wolfpack home, then in `node_modules/.bin` of the output
//! project, then on `PATH`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A tool and how to run it.
#[derive(Clone, Debug)]
pub(crate) struct Tool {
    /// The program.
    pub(crate) program: PathBuf,
    /// Arguments before the caller's, for tools that are scripts (`node tsc`).
    pub(crate) prefix: Vec<OsString>,
    /// Directories put first on `PATH` (the tool's own `bin`).
    pub(crate) path: Vec<PathBuf>,
    /// Further environment variables (`RUSTUP_HOME`, `CARGO_HOME`).
    pub(crate) env: Vec<(OsString, OsString)>,
}

impl Tool {
    fn plain(program: PathBuf) -> Tool {
        Tool {
            program,
            prefix: Vec::new(),
            path: Vec::new(),
            env: Vec::new(),
        }
    }

    /// A command that runs the tool in its environment.
    pub(crate) fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prefix);
        if !self.path.is_empty() {
            let old = std::env::var_os("PATH").unwrap_or_default();
            let mut dirs = self.path.clone();
            dirs.extend(std::env::split_paths(&old));
            if let Ok(joined) = std::env::join_paths(dirs) {
                cmd.env("PATH", joined);
            }
        }
        for (key, value) in &self.env {
            cmd.env(key, value);
        }
        cmd
    }
}

/// The wolfpack home: `WOLFPACK_HOME`, or `.wolfpack` in the home directory.
pub(crate) fn home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("WOLFPACK_HOME") {
        return Some(PathBuf::from(dir));
    }
    let user = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(user).join(".wolfpack"))
}

/// The file name of an executable on this platform.
pub(crate) fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// A tool that `wolf setup` installed, if it is there.
pub(crate) fn installed(name: &str) -> Option<Tool> {
    let toolchains = home()?.join("toolchains");
    let node = toolchains.join(crate::setup::NODE_DIR);
    let node_bin = if cfg!(windows) {
        node.clone()
    } else {
        node.join("bin")
    };
    let tool = match name {
        "node" => {
            let mut t = Tool::plain(node_bin.join(exe("node")));
            t.path.push(node_bin.clone());
            t
        }
        "npm" => {
            let script = if cfg!(windows) { "npm.cmd" } else { "npm" };
            let mut t = Tool::plain(node_bin.join(script));
            t.path.push(node_bin.clone());
            t
        }
        "tsc" => {
            let script = toolchains
                .join(crate::setup::TYPESCRIPT_DIR)
                .join("bin")
                .join("tsc");
            if !script.is_file() {
                return None;
            }
            let mut t = Tool::plain(node_bin.join(exe("node")));
            t.prefix.push(script.into_os_string());
            t.path.push(node_bin.clone());
            t
        }
        "go" => {
            let bin = toolchains.join(crate::setup::GO_DIR).join("go").join("bin");
            let mut t = Tool::plain(bin.join(exe("go")));
            t.path.push(bin);
            t
        }
        "cargo" => {
            let rust = toolchains.join(crate::setup::RUST_DIR);
            let bin = rust.join("cargo").join("bin");
            let mut t = Tool::plain(bin.join(exe("cargo")));
            t.path.push(bin);
            t.env
                .push(("RUSTUP_HOME".into(), rust.join("rustup").into_os_string()));
            t.env
                .push(("CARGO_HOME".into(), rust.join("cargo").into_os_string()));
            t.env
                .push(("RUSTUP_TOOLCHAIN".into(), crate::setup::RUST_VERSION.into()));
            t
        }
        _ => return None,
    };
    tool.program.is_file().then_some(tool)
}

/// A program on `PATH`; on Windows also with the extensions `.exe` and `.cmd`.
pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: Vec<String> = if cfg!(windows) {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            name.to_string(),
        ]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|candidate| candidate.is_file())
}

/// A tool for building in `out`: installed by `wolf setup`, in `node_modules/.bin` of `out`, or on
/// `PATH`.
pub(crate) fn find(name: &str, out: &Path) -> Option<Tool> {
    if let Some(tool) = installed(name) {
        return Some(tool);
    }
    let local = out.join("node_modules").join(".bin").join(name);
    if local.is_file() {
        return Some(Tool::plain(local));
    }
    on_path(name).map(Tool::plain)
}
