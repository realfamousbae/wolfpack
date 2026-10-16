//! `wolfpack.toml` (spec chapter 6, section 6).

use std::collections::BTreeMap;
use std::ops::Range;

use toml::Spanned;
use toml::de::{DeTable, DeValue};
use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, Span};

/// The two kinds of projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A program with `src/main.wolf`.
    Cli,
    /// A library with `src/lib.wolf`.
    Lib,
}

/// The three translation targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// TypeScript
    Ts,
    /// Go
    Go,
    /// Rust
    Rust,
}

impl Target {
    /// All targets.
    pub const ALL: [Target; 3] = [Target::Ts, Target::Go, Target::Rust];

    /// The name used in the manifest and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Target::Ts => "ts",
            Target::Go => "go",
            Target::Rust => "rust",
        }
    }

    fn from_name(name: &str) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// The value of a `[config]` override.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    /// A TOML integer, within 64 bits.
    Int(i64),
    /// A TOML float.
    Float(f64),
    /// A TOML boolean.
    Bool(bool),
    /// A TOML string.
    Str(String),
}

impl ConfigValue {
    /// The wolfpack type a constant must have to take this value.
    pub fn type_name(&self) -> &'static str {
        match self {
            ConfigValue::Int(_) => "Int",
            ConfigValue::Float(_) => "Float",
            ConfigValue::Bool(_) => "Bool",
            ConfigValue::Str(_) => "String",
        }
    }
}

/// One override from `[config.<module path>]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigOverride {
    /// The module path, for example `["users", "storage"]`.
    pub module: Vec<String>,
    /// The constant name.
    pub name: String,
    /// The new value.
    pub value: ConfigValue,
    /// The key in the manifest.
    pub key_span: Span,
    /// The value in the manifest.
    pub value_span: Span,
}

/// A validated manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The manifest file.
    pub file: FileId,
    /// `[package] name`, in kebab-case.
    pub name: String,
    /// `[package] version`.
    pub version: String,
    /// `[package] kind`.
    pub kind: Kind,
    /// Where `kind` is written, for layout errors about the entry file.
    pub kind_span: Span,
    /// `[targets] default`.
    pub default_target: Target,
    /// `[targets] enabled`.
    pub enabled: Vec<Target>,
    /// All `[config.*]` overrides, sorted by module and name.
    pub config: Vec<ConfigOverride>,
    /// `[extern.<target>.dependencies]`: package and version, sorted by package.
    pub dependencies: BTreeMap<Target, Vec<(String, String)>>,
}

struct Reader {
    file: FileId,
    diagnostics: Vec<Diagnostic>,
}

impl Reader {
    fn span(&self, range: Range<usize>) -> Span {
        let start = u32::try_from(range.start).unwrap_or(u32::MAX);
        let end = u32::try_from(range.end).unwrap_or(u32::MAX);
        Span::new(self.file, start, end)
    }

    fn error(&mut self, range: Range<usize>, message: impl Into<String>, label: impl Into<String>) {
        let span = self.span(range);
        self.diagnostics
            .push(Diagnostic::new(codes::E0611, message).primary(span, label));
    }

    /// Reports every key of `table` that is not in `allowed`.
    fn unknown_keys(&mut self, table: &DeTable<'_>, allowed: &[&str], place: &str) {
        for (key, _) in table.iter() {
            if !allowed.contains(&key.get_ref().as_ref()) {
                let name = key.get_ref().to_string();
                let range = key.span();
                let message = format!("unknown key `{name}` in {place}");
                let label = format!("allowed here: {}", allowed.join(", "));
                self.error(range, message, label);
            }
        }
    }

    fn table<'a, 'i>(
        &mut self,
        value: &'a Spanned<DeValue<'i>>,
        what: &str,
    ) -> Option<&'a DeTable<'i>> {
        match value.get_ref() {
            DeValue::Table(table) => Some(table),
            other => {
                let found = other.type_str();
                self.error(
                    value.span(),
                    format!("`{what}` must be a table"),
                    format!("found {found}"),
                );
                None
            }
        }
    }

    fn string(&mut self, value: &Spanned<DeValue<'_>>, what: &str) -> Option<String> {
        match value.get_ref() {
            DeValue::String(text) => Some(text.to_string()),
            other => {
                let found = other.type_str();
                self.error(
                    value.span(),
                    format!("`{what}` must be a string"),
                    format!("found {found}"),
                );
                None
            }
        }
    }
}

fn get<'a, 'i>(table: &'a DeTable<'i>, key: &str) -> Option<&'a Spanned<DeValue<'i>>> {
    table
        .iter()
        .find(|(k, _)| k.get_ref().as_ref() == key)
        .map(|(_, v)| v)
}

/// Parses and validates the manifest text. Returns `None` if it is unusable; every problem is
/// reported as E0611 or E0613 with the exact key.
pub fn parse_manifest(file: FileId, text: &str) -> (Option<Manifest>, Vec<Diagnostic>) {
    let mut reader = Reader {
        file,
        diagnostics: Vec::new(),
    };
    let document = match DeTable::parse(text) {
        Ok(document) => document,
        Err(error) => {
            let range = error.span().unwrap_or(0..0);
            let message = error.message().to_string();
            reader.error(range, "invalid TOML in `wolfpack.toml`", message);
            return (None, reader.diagnostics);
        }
    };
    let root = document.get_ref();
    reader.unknown_keys(
        root,
        &["package", "targets", "config", "extern"],
        "`wolfpack.toml`",
    );

    let manifest = read(&mut reader, root);
    (manifest, reader.diagnostics)
}

fn read(reader: &mut Reader, root: &DeTable<'_>) -> Option<Manifest> {
    let Some(package_value) = get(root, "package") else {
        reader.error(
            0..0,
            "missing `[package]` table",
            "every manifest starts with `[package]`",
        );
        return None;
    };
    let package = reader.table(package_value, "package")?;
    reader.unknown_keys(package, &["name", "version", "kind"], "`[package]`");

    let mut ok = true;
    let name = match get(package, "name") {
        Some(value) => match reader.string(value, "name") {
            Some(name) if is_kebab_case(&name) => Some(name),
            Some(name) => {
                reader.error(
                    value.span(),
                    format!("invalid package name `{name}`"),
                    "use lower-case words separated by `-`, for example `my-app`",
                );
                None
            }
            None => None,
        },
        None => {
            reader.error(
                package_value.span(),
                "missing `name` in `[package]`",
                "add `name = \"my-app\"`",
            );
            None
        }
    };
    let version = match get(package, "version") {
        Some(value) => match reader.string(value, "version") {
            Some(version) if is_semver(&version) => Some(version),
            Some(version) => {
                reader.error(
                    value.span(),
                    format!("invalid version `{version}`"),
                    "use MAJOR.MINOR.PATCH, for example `0.1.0`",
                );
                None
            }
            None => None,
        },
        None => {
            reader.error(
                package_value.span(),
                "missing `version` in `[package]`",
                "add `version = \"0.1.0\"`",
            );
            None
        }
    };
    let (kind, kind_span) = match get(package, "kind") {
        Some(value) => {
            let span = reader.span(value.span());
            match reader.string(value, "kind").as_deref() {
                Some("cli") => (Some(Kind::Cli), span),
                Some("lib") => (Some(Kind::Lib), span),
                Some(other) => {
                    reader.error(
                        value.span(),
                        format!("unknown kind `{other}`"),
                        "expected `cli` or `lib`",
                    );
                    (None, span)
                }
                None => (None, span),
            }
        }
        None => {
            reader.error(
                package_value.span(),
                "missing `kind` in `[package]`",
                "add `kind = \"cli\"` or `kind = \"lib\"`",
            );
            (None, reader.span(0..0))
        }
    };

    let (default_target, enabled) = targets(reader, root).unwrap_or_else(|| {
        ok = false;
        (Target::Ts, Target::ALL.to_vec())
    });
    let config = config(reader, root);
    let dependencies = dependencies(reader, root);

    match (name, version, kind) {
        (Some(name), Some(version), Some(kind)) if ok => Some(Manifest {
            file: reader.file,
            name,
            version,
            kind,
            kind_span,
            default_target,
            enabled,
            config,
            dependencies,
        }),
        _ => None,
    }
}

fn targets(reader: &mut Reader, root: &DeTable<'_>) -> Option<(Target, Vec<Target>)> {
    let Some(value) = get(root, "targets") else {
        return Some((Target::Ts, Target::ALL.to_vec()));
    };
    let table = reader.table(value, "targets")?;
    reader.unknown_keys(table, &["default", "enabled"], "`[targets]`");
    let mut enabled = Target::ALL.to_vec();
    let mut valid = true;
    if let Some(list) = get(table, "enabled") {
        match list.get_ref() {
            DeValue::Array(items) => {
                enabled.clear();
                for item in items.iter() {
                    match item.get_ref().as_str().and_then(Target::from_name) {
                        Some(target) if !enabled.contains(&target) => enabled.push(target),
                        Some(_) => {
                            reader.error(
                                item.span(),
                                "target listed twice",
                                "remove the duplicate",
                            );
                            valid = false;
                        }
                        None => {
                            reader.error(
                                item.span(),
                                "unknown target",
                                "targets are `ts`, `go` and `rust`",
                            );
                            valid = false;
                        }
                    }
                }
                if enabled.is_empty() && valid {
                    reader.error(
                        list.span(),
                        "no target enabled",
                        "enable at least one of `ts`, `go`, `rust`",
                    );
                    valid = false;
                }
            }
            other => {
                let found = other.type_str();
                reader.error(
                    list.span(),
                    "`enabled` must be an array of targets",
                    format!("found {found}"),
                );
                valid = false;
            }
        }
    }
    let mut default = Target::Ts;
    match get(table, "default") {
        Some(value) => match reader
            .string(value, "default")
            .as_deref()
            .map(Target::from_name)
        {
            Some(Some(target)) if enabled.contains(&target) => default = target,
            Some(Some(target)) => {
                reader.error(
                    value.span(),
                    format!("default target `{}` is not enabled", target.name()),
                    "add it to `enabled` or choose an enabled target",
                );
                valid = false;
            }
            Some(None) => {
                reader.error(
                    value.span(),
                    "unknown target",
                    "targets are `ts`, `go` and `rust`",
                );
                valid = false;
            }
            None => valid = false,
        },
        None => {
            // `default` is `"ts"` unless written, and must be enabled (chapter 6, rule 6.2).
            if valid && !enabled.contains(&Target::Ts) {
                let span = get(table, "enabled")
                    .map(|v| v.span())
                    .unwrap_or(value.span());
                reader.error(
                    span,
                    "the default target `ts` is not enabled",
                    "add `default = ...` with an enabled target",
                );
                valid = false;
            }
        }
    }
    valid.then_some((default, enabled))
}

/// `[config.users.storage] max_users = 200`: nested tables are module path segments, the
/// leaves are constants (spec chapter 6, section 6.3).
fn config(reader: &mut Reader, root: &DeTable<'_>) -> Vec<ConfigOverride> {
    let mut out = Vec::new();
    let Some(value) = get(root, "config") else {
        return out;
    };
    let Some(table) = reader.table(value, "config") else {
        return out;
    };
    walk_config(reader, table, &mut Vec::new(), &mut out);
    out.sort_by(|a, b| (&a.module, &a.name).cmp(&(&b.module, &b.name)));
    out
}

fn walk_config(
    reader: &mut Reader,
    table: &DeTable<'_>,
    path: &mut Vec<String>,
    out: &mut Vec<ConfigOverride>,
) {
    for (key, value) in table.iter() {
        let name = key.get_ref().to_string();
        let key_span = reader.span(key.span());
        let value_span = reader.span(value.span());
        let constant = match value.get_ref() {
            DeValue::Table(inner) => {
                path.push(name);
                walk_config(reader, inner, path, out);
                path.pop();
                continue;
            }
            DeValue::Integer(integer) => {
                let digits = integer.as_str().replace('_', "");
                let (negative, digits) = match digits.strip_prefix('-') {
                    Some(rest) => (true, rest.to_string()),
                    None => (false, digits.trim_start_matches('+').to_string()),
                };
                let digits = digits
                    .trim_start_matches("0x")
                    .trim_start_matches("0o")
                    .trim_start_matches("0b")
                    .to_string();
                let parsed = i128::from_str_radix(&digits, integer.radix())
                    .ok()
                    .map(|v| if negative { -v } else { v })
                    .and_then(|v| i64::try_from(v).ok());
                match parsed {
                    Some(v) => ConfigValue::Int(v),
                    None => {
                        reader.diagnostics.push(
                            Diagnostic::new(
                                codes::E0613,
                                "config override does not fit into `Int`",
                            )
                            .primary(value_span, "outside the 64-bit range"),
                        );
                        continue;
                    }
                }
            }
            DeValue::Float(float) => match float.as_str().replace('_', "").parse::<f64>() {
                Ok(v) => ConfigValue::Float(v),
                Err(_) => {
                    reader.diagnostics.push(
                        Diagnostic::new(codes::E0613, "invalid float in a config override")
                            .primary(value_span, "not a number"),
                    );
                    continue;
                }
            },
            DeValue::Boolean(v) => ConfigValue::Bool(*v),
            DeValue::String(text) => ConfigValue::Str(text.to_string()),
            other => {
                let found = other.type_str();
                reader.diagnostics.push(
                    Diagnostic::new(
                        codes::E0613,
                        format!("unsupported value for constant `{name}`"),
                    )
                    .primary(value_span, format!("found {found}"))
                    .note("config constants are `Int`, `Float`, `Bool` or `String`"),
                );
                continue;
            }
        };
        if path.is_empty() {
            reader.diagnostics.push(
                Diagnostic::new(
                    codes::E0613,
                    format!("config override `{name}` has no module"),
                )
                .primary(key_span, "write it in a table named after the module")
                .help(format!(
                    "for example `[config.main]` and then `{name} = ...`"
                )),
            );
            continue;
        }
        out.push(ConfigOverride {
            module: path.clone(),
            name,
            value: constant,
            key_span,
            value_span,
        });
    }
}

fn dependencies(
    reader: &mut Reader,
    root: &DeTable<'_>,
) -> BTreeMap<Target, Vec<(String, String)>> {
    let mut out = BTreeMap::new();
    let Some(value) = get(root, "extern") else {
        return out;
    };
    let Some(table) = reader.table(value, "extern") else {
        return out;
    };
    for (key, target_value) in table.iter() {
        let Some(target) = Target::from_name(key.get_ref()) else {
            reader.error(
                key.span(),
                format!("unknown target `{}`", key.get_ref()),
                "targets are `ts`, `go` and `rust`",
            );
            continue;
        };
        let Some(target_table) = reader.table(target_value, key.get_ref()) else {
            continue;
        };
        reader.unknown_keys(
            target_table,
            &["dependencies"],
            &format!("`[extern.{}]`", target.name()),
        );
        let Some(deps_value) = get(target_table, "dependencies") else {
            continue;
        };
        let Some(deps) = reader.table(deps_value, "dependencies") else {
            continue;
        };
        let mut list = Vec::new();
        for (package, version) in deps.iter() {
            if let Some(version) = reader.string(version, package.get_ref()) {
                list.push((package.get_ref().to_string(), version));
            }
        }
        list.sort();
        out.insert(target, list);
    }
    out
}

/// `[a-z][a-z0-9]*(-[a-z0-9]+)*`
pub fn is_kebab_case(name: &str) -> bool {
    let mut parts = name.split('-');
    let first_ok = parts
        .next()
        .is_some_and(|p| p.starts_with(|c: char| c.is_ascii_lowercase()) && simple(p));
    first_ok && parts.all(|p| !p.is_empty() && simple(p))
}

fn simple(part: &str) -> bool {
    part.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Option<Manifest>, Vec<&'static str>) {
        let mut map = wolf_span::SourceMap::new();
        let id = map.add("wolfpack.toml", text.to_string());
        let (manifest, diagnostics) = parse_manifest(id, text);
        (manifest, diagnostics.iter().map(|d| d.code.id()).collect())
    }

    const MINIMAL: &str = "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\nkind = \"cli\"\n";

    #[test]
    fn minimal_manifest_has_defaults() {
        let (manifest, codes) = parse(MINIMAL);
        assert!(codes.is_empty());
        let manifest = manifest.unwrap();
        assert_eq!(manifest.kind, Kind::Cli);
        assert_eq!(manifest.default_target, Target::Ts);
        assert_eq!(manifest.enabled, Target::ALL);
    }

    #[test]
    fn full_manifest() {
        let text = format!(
            "{MINIMAL}\n[targets]\ndefault = \"go\"\nenabled = [\"go\", \"rust\"]\n\n[config.users.storage]\nmax_users = 200\nratio = 0.5\nname = \"x\"\nflag = true\n\n[extern.go.dependencies]\n\"github.com/a/b\" = \"v1.2.0\"\n"
        );
        let (manifest, codes) = parse(&text);
        assert!(codes.is_empty(), "{codes:?}");
        let manifest = manifest.unwrap();
        assert_eq!(manifest.default_target, Target::Go);
        assert_eq!(manifest.enabled, [Target::Go, Target::Rust]);
        let names: Vec<_> = manifest
            .config
            .iter()
            .map(|c| (c.module.join("."), c.name.clone()))
            .collect();
        assert_eq!(names.len(), 4);
        assert!(names.contains(&("users.storage".to_string(), "max_users".to_string())));
        assert_eq!(
            manifest.dependencies[&Target::Go],
            [("github.com/a/b".to_string(), "v1.2.0".to_string())]
        );
    }

    #[test]
    fn errors_point_at_keys() {
        assert_eq!(
            parse("[package]\nname = \"MyApp\"\nversion = \"0.1.0\"\nkind = \"cli\"\n").1,
            ["E0611"]
        );
        assert_eq!(
            parse("[package]\nname = \"a\"\nversion = \"1.0\"\nkind = \"cli\"\n").1,
            ["E0611"]
        );
        assert_eq!(
            parse("[package]\nname = \"a\"\nversion = \"1.0.0\"\nkind = \"app\"\n").1,
            ["E0611"]
        );
        assert_eq!(parse(&format!("{MINIMAL}colour = 1\n")).1, ["E0611"]);
        assert_eq!(
            parse(&format!(
                "{MINIMAL}[targets]\ndefault = \"go\"\nenabled = [\"ts\"]\n"
            ))
            .1,
            ["E0611"]
        );
        assert_eq!(parse("[package\n").1, ["E0611"]);
        assert_eq!(
            parse(&format!(
                "{MINIMAL}[config.main]\nbig = 99999999999999999999\n"
            ))
            .1,
            ["E0613"]
        );
        assert_eq!(
            parse(&format!("{MINIMAL}[config]\nloose = 1\n")).1,
            ["E0613"]
        );
    }

    #[test]
    fn name_and_version_rules() {
        assert!(is_kebab_case("my-app2"));
        assert!(!is_kebab_case("my--app"));
        assert!(!is_kebab_case("2app"));
        assert!(!is_kebab_case("my_app"));
        assert!(is_semver("0.10.1"));
        assert!(!is_semver("01.0.0"));
        assert!(!is_semver("1.0"));
    }
}
