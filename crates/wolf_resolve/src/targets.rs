//! Target binding of modules (spec chapter 7, section 8.5): a module whose extern names are not
//! declared for a target is not available for it, and neither is a module that imports it.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use wolf_ast::{ExternDeclKind, ItemKind};
use wolf_diag::{Diagnostic, codes};
use wolf_span::Span;

use crate::{ModuleInput, Role};

/// The extern facts of one module: the span of its `extern` header and the names each target
/// block declares.
struct Externs {
    header: Span,
    names: BTreeMap<String, BTreeSet<String>>,
}

impl Externs {
    /// The extern names that the block of `target` misses.
    fn missing(&self, target: &str) -> Vec<String> {
        let all: BTreeSet<&String> = self.names.values().flatten().collect();
        let declared = self.names.get(target);
        all.into_iter()
            .filter(|n| !declared.is_some_and(|d| d.contains(*n)))
            .cloned()
            .collect()
    }
}

/// E0704 for every module of a build for `target` that is not available for it. `built` says
/// which modules the build contains; std modules are always available.
pub fn check_target(
    modules: &[ModuleInput<'_>],
    target: &str,
    built: impl Fn(Role) -> bool,
) -> Vec<Diagnostic> {
    let index: BTreeMap<&[String], usize> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.path.as_slice(), i))
        .collect();
    let mut imports: Vec<Vec<usize>> = vec![Vec::new(); modules.len()];
    // Where each module imports another, for native modules (they have no source of their own).
    let mut import_spans: BTreeMap<(usize, usize), Span> = BTreeMap::new();
    let mut externs: BTreeMap<usize, Externs> = BTreeMap::new();
    for (i, m) in modules.iter().enumerate() {
        if matches!(m.role, Role::Std | Role::Prelude) {
            continue;
        }
        for section in &m.ast.sections {
            for item in &section.items {
                match &item.kind {
                    ItemKind::Import(import) => {
                        let path: Vec<String> = import
                            .path
                            .segments
                            .iter()
                            .map(|s| s.name.clone())
                            .collect();
                        if let (Some(j), Some(list)) =
                            (index.get(path.as_slice()), imports.get_mut(i))
                        {
                            list.push(*j);
                            import_spans.entry((i, *j)).or_insert(import.path.span);
                        }
                    }
                    ItemKind::ExternTarget(block) => {
                        let e = externs.entry(i).or_insert_with(|| Externs {
                            header: section.name_span,
                            names: BTreeMap::new(),
                        });
                        let names = e.names.entry(block.target.name.clone()).or_default();
                        for decl in &block.decls {
                            let name = match &decl.kind {
                                ExternDeclKind::Fn(sig) => &sig.name.name,
                                ExternDeclKind::Type(name) => &name.name,
                            };
                            names.insert(name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // The roots of the build and, for each module, the module it was first reached from.
    let roots: Vec<usize> = modules
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            matches!(m.role, Role::Entry | Role::Example | Role::Test) && built(m.role)
        })
        .map(|(i, _)| i)
        .collect();
    // Modules of `src/` that no root imports are built too; they are reached last.
    let rest: Vec<usize> = modules
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Src)
        .map(|(i, _)| i)
        .collect();
    let mut parent: BTreeMap<usize, Option<usize>> = BTreeMap::new();
    for group in [roots, rest] {
        let mut queue: VecDeque<usize> = VecDeque::new();
        for r in group {
            if let Entry::Vacant(v) = parent.entry(r) {
                v.insert(None);
                queue.push_back(r);
            }
        }
        while let Some(i) = queue.pop_front() {
            for &j in imports.get(i).map_or(&[][..], Vec::as_slice) {
                if let Entry::Vacant(v) = parent.entry(j) {
                    v.insert(Some(i));
                    queue.push_back(j);
                }
            }
        }
    }
    let mut out = Vec::new();
    for (&i, e) in &externs {
        if !parent.contains_key(&i) {
            continue;
        }
        let missing = e.missing(target);
        if missing.is_empty() {
            continue;
        }
        let path_of = |k: usize| modules.get(k).map(|m| m.path.join(".")).unwrap_or_default();
        let name = path_of(i);
        let mut chain = vec![name.clone()];
        let mut cur = i;
        while let Some(Some(p)) = parent.get(&cur) {
            chain.push(path_of(*p));
            cur = *p;
        }
        chain.reverse();
        let bound: Vec<&str> = e
            .names
            .keys()
            .filter(|t| e.missing(t).is_empty())
            .map(String::as_str)
            .collect();
        let list = missing
            .iter()
            .map(|n| format!("`{n}`"))
            .collect::<Vec<_>>()
            .join(", ");
        // A native module (spec chapter 6, section 1a) is reported where it is imported.
        if modules.get(i).is_some_and(|m| m.role == Role::Native) {
            let importer = parent.get(&i).copied().flatten();
            let span = importer.and_then(|p| import_spans.get(&(p, i)).copied());
            let mut d = Diagnostic::new(
                codes::E0704,
                format!("the native module `{name}` is not available for `{target}`"),
            );
            if let Some(span) = span {
                d = d.primary(span, format!("no `{target}` code for {list}"));
            }
            if chain.len() > 2 {
                d = d.note(format!("imported through {}", chain.join(" -> ")));
            }
            out.push(d.help(format!(
                "in this version the compiler reads native modules from Rust code only: build for {}",
                bound
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(" or ")
            )));
            continue;
        }
        let mut d = Diagnostic::new(
            codes::E0704,
            format!("module `{name}` is not available for `{target}`"),
        )
        .primary(e.header, format!("no `{target}` declaration for {list}"));
        if chain.len() > 1 {
            d = d.note(format!("imported through {}", chain.join(" -> ")));
        }
        d = if bound.is_empty() {
            d.help(format!("declare {list} in a `{target}` block"))
        } else {
            d.help(format!(
                "declare {list} in a `{target}` block, or build for {}",
                bound
                    .iter()
                    .map(|t| format!("`{t}`"))
                    .collect::<Vec<_>>()
                    .join(" or ")
            ))
        };
        out.push(d);
    }
    out
}
