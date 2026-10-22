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

/// The imports and the `extern` sections of the modules of a program.
struct Graph {
    imports: Vec<Vec<usize>>,
    /// Where each module imports another, for native modules (they have no source of their own).
    import_spans: BTreeMap<(usize, usize), Span>,
    externs: BTreeMap<usize, Externs>,
}

fn graph(modules: &[ModuleInput<'_>]) -> Graph {
    let index: BTreeMap<&[String], usize> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.path.as_slice(), i))
        .collect();
    let mut imports: Vec<Vec<usize>> = vec![Vec::new(); modules.len()];
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
                                ExternDeclKind::Struct(_) => continue,
                            };
                            names.insert(name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Graph {
        imports,
        import_spans,
        externs,
    }
}

impl Graph {
    /// The modules reached from each group of roots in turn, each with the module it was first
    /// reached from.
    fn reach(&self, groups: Vec<Vec<usize>>) -> BTreeMap<usize, Option<usize>> {
        let mut parent: BTreeMap<usize, Option<usize>> = BTreeMap::new();
        for group in groups {
            let mut queue: VecDeque<usize> = VecDeque::new();
            for r in group {
                if let Entry::Vacant(v) = parent.entry(r) {
                    v.insert(None);
                    queue.push_back(r);
                }
            }
            while let Some(i) = queue.pop_front() {
                for &j in self.imports.get(i).map_or(&[][..], Vec::as_slice) {
                    if let Entry::Vacant(v) = parent.entry(j) {
                        v.insert(Some(i));
                        queue.push_back(j);
                    }
                }
            }
        }
        parent
    }
}

/// The chain of imports from a root to module `i`, as module paths.
fn chain(
    modules: &[ModuleInput<'_>],
    parent: &BTreeMap<usize, Option<usize>>,
    i: usize,
) -> Vec<String> {
    let path_of = |k: usize| modules.get(k).map(|m| m.path.join(".")).unwrap_or_default();
    let mut chain = vec![path_of(i)];
    let mut cur = i;
    while let Some(Some(p)) = parent.get(&cur) {
        chain.push(path_of(*p));
        cur = *p;
    }
    chain.reverse();
    chain
}

/// E0807 for every module reachable from the modules that `root` selects that has an `extern`
/// section or is a native module: the VM runs no code of the targets (spec chapter 10,
/// section 3). `help` says how to run the program on a target instead.
pub fn check_vm(
    modules: &[ModuleInput<'_>],
    root: impl Fn(&ModuleInput<'_>) -> bool,
    help: &str,
) -> Vec<Diagnostic> {
    let g = graph(modules);
    let roots: Vec<usize> = modules
        .iter()
        .enumerate()
        .filter(|(_, m)| !matches!(m.role, Role::Std | Role::Prelude) && root(m))
        .map(|(i, _)| i)
        .collect();
    let parent = g.reach(vec![roots]);
    let mut out = Vec::new();
    for (&i, e) in &g.externs {
        if !parent.contains_key(&i) {
            continue;
        }
        let chain = chain(modules, &parent, i);
        let name = modules.get(i).map(|m| m.path.join(".")).unwrap_or_default();
        let native = modules.get(i).is_some_and(|m| m.role == Role::Native);
        let mut d = if native {
            let importer = parent.get(&i).copied().flatten();
            let mut d = Diagnostic::new(
                codes::E0807,
                format!("the native module `{name}` cannot run in the VM"),
            );
            if let Some(span) = importer.and_then(|p| g.import_spans.get(&(p, i)).copied()) {
                d = d.primary(span, "code of the targets is imported here");
            }
            d
        } else {
            Diagnostic::new(
                codes::E0807,
                format!("module `{name}` cannot run in the VM"),
            )
            .primary(e.header, "code of the targets is called here")
        };
        if chain.len() > 1 {
            d = d.note(format!("imported through {}", chain.join(" -> ")));
        }
        out.push(d.help(help.to_string()));
    }
    out
}

/// E0704 for every module of a build for `target` that is not available for it. `built` says
/// which modules the build contains; std modules are always available.
pub fn check_target(
    modules: &[ModuleInput<'_>],
    target: &str,
    built: impl Fn(Role) -> bool,
) -> Vec<Diagnostic> {
    let g = graph(modules);
    let (import_spans, externs) = (&g.import_spans, &g.externs);
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
    let parent = g.reach(vec![roots, rest]);
    let mut out = Vec::new();
    for (&i, e) in externs {
        if !parent.contains_key(&i) {
            continue;
        }
        let missing = e.missing(target);
        if missing.is_empty() {
            continue;
        }
        let name = modules.get(i).map(|m| m.path.join(".")).unwrap_or_default();
        let chain = chain(modules, &parent, i);
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
            let others = bound
                .iter()
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(" or ");
            out.push(d.help(format!(
                "add the `{target}` code of the module, or build for {others}"
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
