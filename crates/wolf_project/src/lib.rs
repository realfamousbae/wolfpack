//! Projects: the manifest `wolfpack.toml`, the fixed layout, and `wolf new` (spec chapter 6).

mod layout;
mod manifest;

use std::fs;
use std::path::Path;

pub use layout::{ModuleFile, Role, discover, find_root, is_snake_case};
pub use manifest::{
    ConfigOverride, ConfigValue, Kind, Manifest, Target, is_kebab_case, parse_manifest,
};

/// Creates a new project in `dir` (spec chapter 6, rule 7.6). `dir` must not exist or be empty.
pub fn create(dir: &Path, name: &str, kind: Kind) -> Result<(), String> {
    if !is_kebab_case(name) {
        return Err(format!(
            "invalid project name `{name}`: use lower-case words separated by `-`, for example `my-app`"
        ));
    }
    if dir.exists() {
        let empty = fs::read_dir(dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            return Err(format!(
                "`{}` already exists and is not empty",
                dir.display()
            ));
        }
    }
    let kind_name = match kind {
        Kind::Cli => "cli",
        Kind::Lib => "lib",
    };
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nkind = \"{kind_name}\"\n\n[targets]\ndefault = \"ts\"\nenabled = [\"ts\", \"go\", \"rust\"]\n"
    );
    let (entry, source) = match kind {
        Kind::Cli => (
            "main.wolf",
            "module main\n\nfunctions {\n    fn main() {\n        print(\"Hello, wolfpack!\")\n    }\n}\n",
        ),
        Kind::Lib => (
            "lib.wolf",
            "module lib\n\nfunctions {\n    /// Returns a greeting for `name`.\n    pub fn greet(name: String) -> String {\n        \"Hello, \" + name + \"!\"\n    }\n}\n",
        ),
    };
    let write = |path: &Path, text: &str| {
        fs::write(path, text).map_err(|e| format!("cannot write `{}`: {e}", path.display()))
    };
    let mkdir = |path: &Path| {
        fs::create_dir_all(path).map_err(|e| format!("cannot create `{}`: {e}", path.display()))
    };
    mkdir(&dir.join("src"))?;
    mkdir(&dir.join("tests"))?;
    mkdir(&dir.join("examples"))?;
    write(&dir.join("wolfpack.toml"), &manifest)?;
    write(&dir.join("src").join(entry), source)?;
    write(&dir.join(".gitignore"), "/out/\n")?;
    Ok(())
}
