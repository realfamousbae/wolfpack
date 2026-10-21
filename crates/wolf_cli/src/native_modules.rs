//! Native modules (spec chapter 6, section 1a; chapter 7, section 8.7): for each `native.<path>`
//! that the project imports, the declarations of its Rust file and Go package are read and written
//! as a module of `extern` declarations, which the rest of the compiler treats like any module.

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
/// available, with the reason (E0713); and the diagnostics: files that could not be read (E0714)
/// and names that the targets declare differently (E0715).
pub(crate) fn load(
    root: &Path,
    parsed: &[Parsed],
    map: &mut SourceMap,
    listing: Option<&str>,
) -> Loaded {
    let reserved = prelude_names(parsed);
    let mut modules = Vec::new();
    let mut names = NativeNames::new();
    let mut diagnostics = Vec::new();
    let mut listings = Vec::new();
    let mut wanted: BTreeMap<Vec<String>, Option<wolf_span::Span>> = imported(parsed)
        .into_iter()
        .map(|(p, s)| (p, Some(s)))
        .collect();
    // `wolf check --native <module>`: that module, imported or not (spec chapter 8, section 8b).
    let only = listing.filter(|m| !m.is_empty()).map(|m| {
        let path: Vec<String> = m.split('.').map(str::to_string).collect();
        wanted.entry(path.clone()).or_insert(None);
        path
    });
    let mut found_only = false;
    for (path, span) in wanted {
        let listed = listing.is_some() && only.as_ref().is_none_or(|o| *o == path);
        if span.is_none() && !listed {
            continue;
        }
        let rest: Vec<&str> = path.iter().skip(1).map(String::as_str).collect();
        let rel = rest.join("/");
        let rust = [format!("{rel}.rs"), format!("{rel}/mod.rs")]
            .into_iter()
            .map(|f| root.join("native/rust").join(f))
            .find(|f| f.is_file());
        let go_dir = root.join("native/go").join(&rel);
        let go_files = go_files(&go_dir);
        let ts = Some(root.join("native/ts").join(format!("{rel}.ts"))).filter(|f| f.is_file());
        let exists = rust.is_some() || !go_files.is_empty() || ts.is_some();
        if !exists {
            // An unknown module: the resolver reports it (E0602).
            continue;
        }
        if only.as_ref() == Some(&path) {
            found_only = true;
        }
        let mut unreadable = false;
        let mut rust_module = None;
        if let Some(file) = &rust {
            let shown = shown(root, file);
            match fs::read_to_string(file) {
                Ok(text) => match wolf_native::read_rust(&text) {
                    Ok(m) => rust_module = Some(m),
                    Err(e) => {
                        unreadable = true;
                        diagnostics.push(unreadable_file(&shown, "Rust", &e));
                    }
                },
                Err(e) => diagnostics.push(Diagnostic::new(
                    codes::E0001,
                    format!("cannot read `{shown}`: {e}"),
                )),
            }
        }
        let mut go_module = None;
        if !go_files.is_empty() {
            match wolf_native::read_go(&go_files) {
                Ok(m) => go_module = Some(m),
                Err((file, e)) => {
                    unreadable = true;
                    let shown = shown(root, &go_dir.join(file));
                    diagnostics.push(unreadable_file(&shown, "Go", &e));
                }
            }
        }
        let mut ts_module = None;
        if let Some(file) = &ts {
            let shown = shown(root, file);
            match fs::read_to_string(file) {
                Ok(text) => match wolf_native::read_ts(&text) {
                    Ok(m) => ts_module = Some(m),
                    Err(e) => {
                        unreadable = true;
                        diagnostics.push(unreadable_file(&shown, "TypeScript", &e));
                    }
                },
                Err(e) => diagnostics.push(Diagnostic::new(
                    codes::E0001,
                    format!("cannot read `{shown}`: {e}"),
                )),
            }
        }
        let module_path = path.join(".");
        // The targets that have code, in a fixed order: label, target, path inside
        // `native/<target>/`, and what was read.
        let mut targets: Vec<(&str, &str, String, wolf_native::Module)> = Vec::new();
        if let Some(m) = rust_module {
            targets.push(("Rust", "rust", rest.join("::"), m));
        }
        if let Some(m) = go_module {
            targets.push(("Go", "go", rel.clone(), m));
        }
        if let Some(m) = ts_module {
            targets.push(("TypeScript", "ts", rel.clone(), m));
        }
        // The `number` rule (spec chapter 7, section 8.7, rule 14).
        if let Some(i) = targets.iter().position(|t| t.1 == "ts") {
            let (ts_part, others) = split_one(&mut targets, i);
            if let Some(ts) = ts_part {
                let others: Vec<&wolf_native::Module> = others.iter().map(|t| &t.3).collect();
                settle_numbers(&mut ts.3, &others);
                float_keys(&mut ts.3);
            }
        }
        // Records first: the functions that use a record the targets disagree on go with it.
        for (i, j) in pairs(targets.len()) {
            let (a, b) = two(&mut targets, i, j);
            if let (Some(a), Some(b)) = (a, b) {
                disagree_records(a, b, &module_path, span, &mut diagnostics);
            }
        }
        for t in &mut targets {
            filter_names(&mut t.3, &reserved);
        }
        // Agreement of the targets (spec chapter 7, section 8.7, rule 10).
        for (i, j) in pairs(targets.len()) {
            let (a, b) = two(&mut targets, i, j);
            if let (Some(a), Some(b)) = (a, b) {
                disagree(a, b, &module_path, span, &mut diagnostics);
            }
        }
        let blocks: Vec<wolf_native::Block<'_>> = targets
            .iter()
            .map(|(_, target, path, m)| wolf_native::Block {
                target,
                path: path.clone(),
                module: m,
            })
            .collect();
        let name_ok = |n: &str| wolf_resolve::is_snake_case(n) && TokenKind::keyword(n).is_none();
        if listed {
            let files: Vec<String> = rust
                .iter()
                .map(|f| shown(root, f))
                .chain((!go_files.is_empty()).then(|| format!("{}/", shown(root, &go_dir))))
                .chain(ts.iter().map(|f| shown(root, f)))
                .map(|f| format!("`{f}`"))
                .collect();
            listings.push(wolf_native::listing(
                &module_path,
                &files,
                &blocks,
                &name_ok,
            ));
        }
        if span.is_none() {
            continue;
        }
        let source = wolf_native::source(&module_path, &blocks, &name_ok);
        let id = map.add(format!("<native module {module_path}>"), source);
        let mut ast = wolf_driver::parse(map, id, &mut diagnostics);
        let mut sigs: BTreeMap<(&str, &str), &wolf_ast::NativeSig> = BTreeMap::new();
        let mut records: BTreeMap<(&str, &str), &wolf_native::Record> = BTreeMap::new();
        let block_refs: Vec<&wolf_native::Block<'_>> = blocks.iter().collect();
        let hidden = wolf_native::method_names(&block_refs);
        for b in &blocks {
            for f in &b.module.fns {
                sigs.insert((b.target, f.name.as_str()), &f.native);
            }
            for m in &b.module.methods {
                if let Some(h) = hidden.get(&(m.owner.clone(), m.func.name.clone())) {
                    sigs.insert((b.target, h.as_str()), &m.func.native);
                }
            }
            for r in &b.module.records {
                records.insert((b.target, r.name.as_str()), r);
            }
        }
        for section in &mut ast.sections {
            for item in &mut section.items {
                let ItemKind::ExternTarget(block) = &mut item.kind else {
                    continue;
                };
                let target = block.target.name.clone();
                for decl in &mut block.decls {
                    match &decl.kind {
                        ExternDeclKind::Fn(sig) => {
                            decl.native = sigs
                                .get(&(target.as_str(), sig.name.name.as_str()))
                                .map(|s| (*s).clone());
                        }
                        ExternDeclKind::Struct(name) => {
                            if let Some(r) = records.get(&(target.as_str(), name.name.as_str())) {
                                decl.native_fields = r
                                    .fields
                                    .iter()
                                    .map(|f| (f.target_name.clone(), f.native.clone()))
                                    .collect();
                            }
                        }
                        ExternDeclKind::Type(_) => {}
                    }
                }
            }
        }
        // A name is not available if no target maps it; the reasons of each target are shown.
        let available = |m: &wolf_native::Module, n: &str| {
            m.fns.iter().any(|f| f.name == n)
                || m.types.iter().any(|t| t == n)
                || m.records.iter().any(|r| r.name == n)
                || m.methods
                    .iter()
                    .any(|x| format!("{}.{}", x.owner, x.func.name) == n)
        };
        let mut reasons: BTreeMap<String, Vec<(&str, &String)>> = BTreeMap::new();
        let present = targets.len();
        for (label, _, _, m) in &targets {
            for (n, reason) in &m.unavailable {
                if targets.iter().any(|t| available(&t.3, n)) {
                    continue;
                }
                reasons.entry(n.clone()).or_default().push((label, reason));
            }
        }
        // One reason that every target gives is shown once.
        let mut unavailable: BTreeMap<String, String> = BTreeMap::new();
        for (n, rs) in reasons {
            let same =
                rs.len() == present && rs.iter().all(|(_, r)| Some(r) == rs.first().map(|x| &x.1));
            let text = match rs.first() {
                Some((_, r)) if same => (*r).clone(),
                _ => rs
                    .iter()
                    .map(|(label, r)| format!("in {label}, {r}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            };
            unavailable.insert(n, text);
        }
        if unreadable {
            unavailable.insert(
                wolf_resolve::UNREADABLE.to_string(),
                "a file cannot be read".to_string(),
            );
        }
        names.insert(module_path, unavailable);
        modules.push((path, Role::Native, id, ast));
    }
    if let (Some(m), false) = (listing.filter(|m| !m.is_empty()), found_only) {
        diagnostics.push(
            Diagnostic::new(codes::E0716, format!("no native module `{m}` in this project"))
                .help("name a module `native.<path>` with code in `native/ts/`, `native/go/` or `native/rust/`"),
        );
    }
    Loaded {
        modules,
        names,
        diagnostics,
        listings,
    }
}

/// What [`load`] produced.
pub(crate) struct Loaded {
    /// The generated modules of the native modules that the project imports.
    pub(crate) modules: Vec<Parsed>,
    /// The names that are not available, with the reason (E0713).
    pub(crate) names: NativeNames,
    /// E0714, E0715 and E0716.
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// What `wolf check --native` prints (spec chapter 8, section 8b).
    pub(crate) listings: Vec<String>,
}

/// A diagnostic with its primary label at the import, when there is one: a module that
/// `wolf check --native` prints without an import has none.
trait AtImport {
    fn with_span_at(self, span: Option<wolf_span::Span>) -> Self;
}

impl AtImport for Diagnostic {
    fn with_span_at(self, span: Option<wolf_span::Span>) -> Self {
        match span {
            Some(span) => self.primary(span, "imported here"),
            None => self,
        }
    }
}

fn shown(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unreadable_file(shown: &str, language: &str, e: &wolf_native::ReadError) -> Diagnostic {
    Diagnostic::new(codes::E0714, format!("cannot read `{shown}`"))
        .note(format!(
            "line {}, column {}: {}",
            e.line, e.column, e.message
        ))
        .help(format!(
            "correct the {language} code: `wolf` reads its declarations"
        ))
}

/// The `.go` files directly in a package directory, by name, in a fixed order.
fn go_files(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, String)> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".go") {
                return None;
            }
            fs::read_to_string(e.path()).ok().map(|text| (name, text))
        })
        .collect();
    files.sort();
    files
}

/// Names that two targets map to different wolfpack signatures: E0715, and the name is not
/// available in either.
fn disagree(
    (ra, _, _, r): &mut Target,
    (ga, _, _, g): &mut Target,
    module_path: &str,
    span: Option<wolf_span::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let show = |f: &wolf_native::Function| {
        let params: Vec<String> = f.params.iter().map(|(_, w)| w.text()).collect();
        format!("fn {}({}) -> {}", f.name, params.join(", "), f.ret.text())
    };
    let mut bad = Vec::new();
    let mut bad_methods = Vec::new();
    for rf in &r.fns {
        if let Some(gf) = g.fns.iter().find(|gf| gf.name == rf.name) {
            let (a, b) = (show(rf), show(gf));
            if a != b {
                diagnostics.push(
                    Diagnostic::new(
                        codes::E0715,
                        format!(
                            "`{}` of the native module `{module_path}` differs between {ra} and {ga}",
                            rf.name
                        ),
                    )
                    .with_span_at(span)
                    .note(format!("{ra}: {a}"))
                    .note(format!("{ga}: {b}"))
                    .help("give the function the same parameter and result types in both files"),
                );
                bad.push(rf.name.clone());
            }
        }
    }
    for rm in &r.methods {
        let Some(gm) = g
            .methods
            .iter()
            .find(|gm| gm.owner == rm.owner && gm.func.name == rm.func.name)
        else {
            continue;
        };
        let (a, b) = (show(&rm.func), show(&gm.func));
        if a != b || rm.kind != gm.kind {
            let name = format!("{}.{}", rm.owner, rm.func.name);
            diagnostics.push(
                Diagnostic::new(
                    codes::E0715,
                    format!(
                        "`{name}` of the native module `{module_path}` differs between {ra} and {ga}"
                    ),
                )
                .with_span_at(span)
                .note(format!("{ra}: {a}"))
                .note(format!("{ga}: {b}"))
                .help("give the method the same parameter and result types in both files"),
            );
            bad_methods.push((rm.owner.clone(), rm.func.name.clone()));
        }
    }
    for (owner, n) in bad_methods {
        let reason = format!("{ra} and {ga} declare it differently (E0715)");
        r.methods
            .retain(|m| !(m.owner == owner && m.func.name == n));
        g.methods
            .retain(|m| !(m.owner == owner && m.func.name == n));
        r.unavailable.insert(format!("{owner}.{n}"), reason.clone());
        g.unavailable.insert(format!("{owner}.{n}"), reason);
    }
    for n in bad {
        let reason = format!("{ra} and {ga} declare it differently (E0715)");
        r.fns.retain(|f| f.name != n);
        g.fns.retain(|f| f.name != n);
        r.unavailable.insert(n.clone(), reason.clone());
        g.unavailable.insert(n, reason);
    }
}

/// The first type of `removed` that `w` uses.
fn uses_removed(w: &wolf_native::Wolf, removed: &BTreeSet<String>) -> Option<String> {
    use wolf_native::Wolf;
    match w {
        Wolf::Named(n) if removed.contains(n) => Some(n.clone()),
        Wolf::List(t) | Wolf::Option(t) | Wolf::Result(t) => uses_removed(t, removed),
        Wolf::Map(k, v) => uses_removed(k, removed).or_else(|| uses_removed(v, removed)),
        Wolf::Fn(ps, r) => ps
            .iter()
            .find_map(|p| uses_removed(p, removed))
            .or_else(|| uses_removed(r, removed)),
        _ => None,
    }
}

/// Records that two targets read differently: other fields, or a record in one target and an
/// opaque type in the other (E0715). The name is not available in either.
fn disagree_records(
    (ra, _, _, r): &mut Target,
    (ga, _, _, g): &mut Target,
    module_path: &str,
    span: Option<wolf_span::Span>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (r, g) = (&mut *r, &mut *g);
    let show = |m: &wolf_native::Module, name: &str| -> Option<String> {
        if let Some(rec) = m.records.iter().find(|x| x.name == name) {
            let fields: Vec<String> = rec
                .fields
                .iter()
                .map(|f| format!("{}: {}", f.name, f.ty.text()))
                .collect();
            Some(format!("struct {} {{ {} }}", name, fields.join(", ")))
        } else if m.types.iter().any(|t| t == name) {
            Some(format!("type {name} (an opaque type)"))
        } else {
            None
        }
    };
    let names: BTreeSet<String> = r
        .records
        .iter()
        .chain(&g.records)
        .map(|x| x.name.clone())
        .collect();
    for n in names {
        let (Some(a), Some(b)) = (show(r, &n), show(g, &n)) else {
            continue;
        };
        if a == b {
            continue;
        }
        diagnostics.push(
            Diagnostic::new(
                codes::E0715,
                format!("`{n}` of the native module `{module_path}` differs between {ra} and {ga}"),
            )
            .with_span_at(span)
            .note(format!("{ra}: {a}"))
            .note(format!("{ga}: {b}"))
            .help("give the struct the same public fields in both files, in the same order and of the same types"),
        );
        let reason = format!("{ra} and {ga} declare it differently (E0715)");
        for m in [&mut *r, &mut *g] {
            m.records.retain(|x| x.name != n);
            m.types.retain(|x| *x != n);
            m.unavailable.insert(n.clone(), reason.clone());
        }
    }
}

/// What one target of a native module has: label, target, path inside `native/<target>/`, and
/// the declarations read.
type Target<'a> = (&'a str, &'a str, String, wolf_native::Module);

/// The pairs `(i, j)` with `i < j < n`.
fn pairs(n: usize) -> Vec<(usize, usize)> {
    (0..n)
        .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
        .collect()
}

/// The elements `i` and `j` of `v`, with `i < j`, both mutable.
fn two<T>(v: &mut [T], i: usize, j: usize) -> (Option<&mut T>, Option<&mut T>) {
    let (left, right) = v.split_at_mut(j);
    (left.get_mut(i), right.first_mut())
}

/// The element `i` of `v`, mutable, and the others.
fn split_one<T>(v: &mut [T], i: usize) -> (Option<&mut T>, Vec<&T>) {
    let (left, right) = v.split_at_mut(i);
    let (one, after) = match right.split_first_mut() {
        Some((one, after)) => (Some(one), after),
        None => (None, &mut [][..]),
    };
    let others = left.iter().chain(after.iter()).collect();
    (one, others)
}

/// The `number` rule (spec chapter 7, section 8.7, rule 14): a `Float` of TypeScript becomes `Int`
/// where another target gives `Int` at the same place.
fn settle_numbers(ts: &mut wolf_native::Module, others: &[&wolf_native::Module]) {
    for f in &mut ts.fns {
        let Some(other) = others
            .iter()
            .find_map(|m| m.fns.iter().find(|g| g.name == f.name))
        else {
            continue;
        };
        if other.params.len() == f.params.len() {
            for ((_, w), (_, o)) in f.params.iter_mut().zip(&other.params) {
                settle(w, o);
            }
        }
        settle(&mut f.ret, &other.ret);
    }
    for m in &mut ts.methods {
        let Some(other) = others.iter().find_map(|o| {
            o.methods
                .iter()
                .find(|x| x.owner == m.owner && x.func.name == m.func.name)
        }) else {
            continue;
        };
        if other.func.params.len() == m.func.params.len() {
            for ((_, w), (_, o)) in m.func.params.iter_mut().zip(&other.func.params) {
                settle(w, o);
            }
        }
        settle(&mut m.func.ret, &other.func.ret);
    }
    for r in &mut ts.records {
        let Some(other) = others
            .iter()
            .find_map(|m| m.records.iter().find(|o| o.name == r.name))
        else {
            continue;
        };
        for (f, o) in r.fields.iter_mut().zip(&other.fields) {
            if f.name == o.name {
                settle(&mut f.ty, &o.ty);
            }
        }
    }
}

/// `w` with `Float` made `Int` where `other` has `Int`.
fn settle(w: &mut wolf_native::Wolf, other: &wolf_native::Wolf) {
    use wolf_native::Wolf;
    match (w, other) {
        (w @ Wolf::Float, Wolf::Int) => *w = Wolf::Int,
        (Wolf::List(a), Wolf::List(b))
        | (Wolf::Option(a), Wolf::Option(b))
        | (Wolf::Result(a), Wolf::Result(b)) => settle(a, b),
        (Wolf::Map(ak, av), Wolf::Map(bk, bv)) => {
            settle(ak, bk);
            settle(av, bv);
        }
        (Wolf::Fn(ap, ar), Wolf::Fn(bp, br)) if ap.len() == bp.len() => {
            for (a, b) in ap.iter_mut().zip(bp) {
                settle(a, b);
            }
            settle(ar, br);
        }
        _ => {}
    }
}

/// After the `number` rule, a map whose keys are still `Float` (TypeScript `number` keys that no
/// other target makes `Int`) does not map: the item is not available.
fn float_keys(ts: &mut wolf_native::Module) {
    fn has(w: &wolf_native::Wolf) -> bool {
        use wolf_native::Wolf;
        match w {
            Wolf::Map(k, v) => matches!(**k, Wolf::Float) || has(v),
            Wolf::List(t) | Wolf::Option(t) | Wolf::Result(t) => has(t),
            Wolf::Fn(ps, r) => ps.iter().any(has) || has(r),
            _ => false,
        }
    }
    let reason = "a map with `number` keys: write `bigint` for `Int` keys".to_string();
    let bad: Vec<String> = ts
        .records
        .iter()
        .filter(|r| r.fields.iter().any(|f| has(&f.ty)))
        .map(|r| r.name.clone())
        .collect();
    for n in bad {
        ts.records.retain(|r| r.name != n);
        ts.unavailable.insert(n, reason.clone());
    }
    let bad: Vec<String> = ts
        .fns
        .iter()
        .filter(|f| f.params.iter().any(|(_, w)| has(w)) || has(&f.ret))
        .map(|f| f.name.clone())
        .collect();
    for n in bad {
        ts.fns.retain(|f| f.name != n);
        ts.unavailable.insert(n, reason.clone());
    }
}

/// The module paths `native.<path>` imported by the modules of the project, in a fixed order,
/// each with the place of its first import.
fn imported(parsed: &[Parsed]) -> BTreeMap<Vec<String>, wolf_span::Span> {
    let mut out = BTreeMap::new();
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
                    out.entry(path).or_insert(import.path.span);
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
    // Types that are already not available (records the targets disagree on).
    let mut removed: BTreeSet<String> = module
        .unavailable
        .keys()
        .filter(|k| k.starts_with(|c: char| c.is_uppercase()))
        .cloned()
        .collect();
    module.types.retain(|t| match reason(t, true) {
        Some(r) => {
            module.unavailable.insert(t.clone(), r);
            removed.insert(t.clone());
            false
        }
        None => true,
    });
    // A record whose name or field names wolfpack cannot declare.
    module.records.retain(|rec| {
        let why = reason(&rec.name, true).or_else(|| {
            rec.fields.iter().find_map(|f| {
                if TokenKind::keyword(&f.name).is_some() {
                    Some(format!("its field `{}` is a keyword of wolfpack", f.name))
                } else if !wolf_resolve::is_snake_case(&f.name) {
                    Some(format!(
                        "a field name of wolfpack is `snake_case`, not `{}`",
                        f.name
                    ))
                } else {
                    None
                }
            })
        });
        match why {
            Some(r) => {
                module.unavailable.insert(rec.name.clone(), r);
                removed.insert(rec.name.clone());
                false
            }
            None => true,
        }
    });
    // A record that holds a removed type goes too, until nothing changes.
    loop {
        let gone: Vec<(String, String)> = module
            .records
            .iter()
            .filter_map(|rec| {
                rec.fields
                    .iter()
                    .find_map(|f| uses_removed(&f.ty, &removed))
                    .map(|t| (rec.name.clone(), t))
            })
            .collect();
        if gone.is_empty() {
            break;
        }
        for (name, t) in gone {
            module.records.retain(|r| r.name != name);
            module.unavailable.insert(
                name.clone(),
                format!("it holds `{t}`, which is not available"),
            );
            removed.insert(name);
        }
    }
    let uses = |w: &wolf_native::Wolf| uses_removed(w, &removed);
    // Methods: a name wolfpack cannot declare, or a type that is not available (rule 17).
    let mut kept = Vec::new();
    for m in std::mem::take(&mut module.methods) {
        let why = if removed.contains(&m.owner) {
            Some(format!("`{}` is not available", m.owner))
        } else {
            reason(&m.func.name, false).or_else(|| {
                m.func
                    .params
                    .iter()
                    .map(|(_, w)| w)
                    .chain(std::iter::once(&m.func.ret))
                    .find_map(&uses)
                    .map(|t| format!("it uses `{t}`, which is not available"))
            })
        };
        match why {
            Some(r) => {
                module
                    .unavailable
                    .insert(format!("{}.{}", m.owner, m.func.name), r);
            }
            None => kept.push(m),
        }
    }
    module.methods = kept;
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
