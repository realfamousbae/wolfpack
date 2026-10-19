//! Native modules (spec chapter 6, section 1a; chapter 7, section 8.7): for each `native.<path>`
//! that the project imports, the declarations of its Rust file are read and written as a module
//! of `extern` declarations, which the rest of the compiler treats like any module.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use wolf_ast::{ExternDeclKind, ItemKind};
use wolf_diag::{Diagnostic, codes};
use wolf_lexer::TokenKind;
use wolf_resolve::{NativeNames, Role};
use wolf_span::SourceMap;

/// A parsed module: its path, its role, its file and its syntax tree.
type Parsed = (Vec<String>, Role, wolf_span::FileId, wolf_ast::File);

/// The native modules imported by `parsed`, read from `root/native/`; the names that are not
/// available, with the reason (E0713); and the files that could not be read (E0714).
pub(crate) fn load(
    root: &Path,
    parsed: &[Parsed],
    map: &mut SourceMap,
) -> (Vec<Parsed>, NativeNames, Vec<Diagnostic>) {
    let reserved = prelude_names(parsed);
    let mut modules = Vec::new();
    let mut names = NativeNames::new();
    let mut diagnostics = Vec::new();
    for path in imported(parsed) {
        let rest: Vec<&str> = path.iter().skip(1).map(String::as_str).collect();
        let rel = rest.join("/");
        let rust = [format!("{rel}.rs"), format!("{rel}/mod.rs")]
            .into_iter()
            .map(|f| root.join("native/rust").join(f))
            .find(|f| f.is_file());
        let exists = rust.is_some()
            || root.join("native/ts").join(format!("{rel}.ts")).is_file()
            || root.join("native/go").join(&rel).is_dir();
        if !exists {
            // An unknown module: the resolver reports it (E0602).
            continue;
        }
        let mut module = wolf_native::Module::default();
        if let Some(file) = &rust {
            let shown = file
                .strip_prefix(root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            match fs::read_to_string(file) {
                Ok(text) => match wolf_native::read_rust(&text) {
                    Ok(m) => module = m,
                    Err(e) => {
                        module.unavailable.insert(
                            wolf_resolve::UNREADABLE.to_string(),
                            "the file cannot be read".to_string(),
                        );
                        diagnostics.push(
                            Diagnostic::new(codes::E0714, format!("cannot read `{shown}`"))
                                .note(format!(
                                    "line {}, column {}: {}",
                                    e.line, e.column, e.message
                                ))
                                .help("correct the Rust code: `wolf` reads its declarations"),
                        );
                    }
                },
                Err(e) => diagnostics.push(Diagnostic::new(
                    codes::E0001,
                    format!("cannot read `{shown}`: {e}"),
                )),
            }
        }
        filter_names(&mut module, &reserved);
        let module_path = path.join(".");
        let source = wolf_native::rust_source(&module_path, &rest.join("::"), &module, &|n| {
            wolf_resolve::is_snake_case(n) && TokenKind::keyword(n).is_none()
        });
        let id = map.add(format!("<native module {module_path}>"), source);
        let mut ast = wolf_driver::parse(map, id, &mut diagnostics);
        let sigs: BTreeMap<&str, &wolf_ast::NativeSig> = module
            .fns
            .iter()
            .map(|f| (f.name.as_str(), &f.native))
            .collect();
        for section in &mut ast.sections {
            for item in &mut section.items {
                let ItemKind::ExternTarget(block) = &mut item.kind else {
                    continue;
                };
                for decl in &mut block.decls {
                    if let ExternDeclKind::Fn(sig) = &decl.kind {
                        decl.native = sigs.get(sig.name.name.as_str()).map(|s| (*s).clone());
                    }
                }
            }
        }
        names.insert(module_path, module.unavailable);
        modules.push((path, Role::Native, id, ast));
    }
    (modules, names, diagnostics)
}

/// The module paths `native.<path>` imported by the modules of the project, in a fixed order.
fn imported(parsed: &[Parsed]) -> BTreeSet<Vec<String>> {
    let mut out = BTreeSet::new();
    for (_, role, _, file) in parsed {
        if matches!(role, Role::Prelude | Role::Std) {
            continue;
        }
        for item in file.sections.iter().flat_map(|s| &s.items) {
            if let ItemKind::Import(import) = &item.kind {
                let path: Vec<String> = import
                    .path
                    .segments
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                if path.len() > 1 && path.first().map(String::as_str) == Some("native") {
                    out.insert(path);
                }
            }
        }
    }
    out
}

/// The names declared by the prelude, which a native module may not declare again (E0617).
fn prelude_names(parsed: &[Parsed]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, role, _, file) in parsed {
        if *role != Role::Prelude {
            continue;
        }
        for item in file.sections.iter().flat_map(|s| &s.items) {
            let name = match &item.kind {
                ItemKind::Struct(d) => &d.name,
                ItemKind::Enum(d) => &d.name,
                ItemKind::Alias(d) => &d.name,
                ItemKind::IntrinsicType(d) => &d.name,
                ItemKind::Interface(d) => &d.name,
                ItemKind::Fn(d) => &d.sig.name,
                _ => continue,
            };
            out.insert(name.name.clone());
        }
    }
    out
}

/// Moves the items whose names wolfpack cannot declare into `unavailable`: names that break the
/// naming style, keywords and prelude names; then the functions that use a removed type.
fn filter_names(module: &mut wolf_native::Module, reserved: &BTreeSet<String>) {
    let reason = |name: &str, pascal: bool| -> Option<String> {
        if TokenKind::keyword(name).is_some() {
            Some(format!("`{name}` is a keyword of wolfpack"))
        } else if reserved.contains(name) {
            Some(format!("`{name}` is a name of the prelude"))
        } else if pascal && !wolf_resolve::is_pascal_case(name) {
            Some(format!(
                "a type name of wolfpack is `PascalCase`, not `{name}`"
            ))
        } else if !pascal && !wolf_resolve::is_snake_case(name) {
            Some(format!(
                "a function name of wolfpack is `snake_case`, not `{name}`"
            ))
        } else {
            None
        }
    };
    let mut removed = BTreeSet::new();
    module.types.retain(|t| match reason(t, true) {
        Some(r) => {
            module.unavailable.insert(t.clone(), r);
            removed.insert(t.clone());
            false
        }
        None => true,
    });
    let uses = |w: &wolf_native::Wolf| -> Option<String> {
        fn find(w: &wolf_native::Wolf, removed: &BTreeSet<String>) -> Option<String> {
            use wolf_native::Wolf;
            match w {
                Wolf::Named(n) if removed.contains(n) => Some(n.clone()),
                Wolf::List(t) | Wolf::Option(t) | Wolf::Result(t) => find(t, removed),
                Wolf::Map(k, v) => find(k, removed).or_else(|| find(v, removed)),
                Wolf::Fn(ps, r) => ps
                    .iter()
                    .find_map(|p| find(p, removed))
                    .or_else(|| find(r, removed)),
                _ => None,
            }
        }
        find(w, &removed)
    };
    let mut kept = Vec::new();
    for f in std::mem::take(&mut module.fns) {
        let why = reason(&f.name, false).or_else(|| {
            f.params
                .iter()
                .map(|(_, w)| w)
                .chain(std::iter::once(&f.ret))
                .find_map(&uses)
                .map(|t| format!("it uses `{t}`, which is not available"))
        });
        match why {
            Some(r) => {
                module.unavailable.insert(f.name.clone(), r);
            }
            None => kept.push(f),
        }
    }
    module.fns = kept;
}
