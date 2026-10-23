//! What a position of a file names, and what the server says about it: the text of a hover and
//! the place of a declaration.

use wolf_resolve::{DefId, DefKind, LocalId, Res};
use wolf_span::{FileId, Span};
use wolf_types::Shape;

use crate::Project;

/// A thing a position can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    /// A declaration.
    Def(DefId),
    /// A local variable, parameter or binding.
    Local(LocalId),
    /// A module.
    Module(wolf_resolve::ModuleId),
}

/// What the innermost name at `offset` of `file` names: a use of a name, or the name in a
/// declaration of a local or of a top-level item.
pub(crate) fn named_at(p: &Project, file: FileId, offset: u32) -> Option<(Named, Span)> {
    let contains = |s: &Span| s.file == file && s.start <= offset && offset <= s.end;
    let mut best: Option<(Named, Span)> = None;
    let mut consider = |named: Named, span: Span| {
        if contains(&span) && best.is_none_or(|(_, b)| span.len() < b.len()) {
            best = Some((named, span));
        }
    };
    for (key, span) in &p.resolved.spans {
        match p.resolved.res.get(key) {
            Some(Res::Def(d)) => consider(Named::Def(*d), *span),
            Some(Res::Local(l)) => consider(Named::Local(*l), *span),
            Some(Res::Module(m)) => consider(Named::Module(*m), *span),
            _ => {}
        }
    }
    for (span, local) in &p.resolved.local_decls {
        consider(Named::Local(*local), *span);
    }
    for (i, def) in p.resolved.defs.iter().enumerate() {
        consider(Named::Def(DefId(i as u32)), def.span);
    }
    best
}

/// Where the thing is declared.
pub(crate) fn declaration(p: &Project, named: Named) -> Option<Span> {
    match named {
        Named::Def(d) => p.resolved.def(d).map(|d| d.span),
        Named::Local(l) => p.resolved.locals.get(l.0 as usize).map(|l| l.span),
        Named::Module(m) => p
            .resolved
            .modules
            .get(m.0 as usize)
            .map(|m| Span::new(m.file, 0, 0)),
    }
}

/// The text of a hover: the declaration as wolfpack code, then its doc comments.
pub(crate) fn hover(p: &Project, named: Named) -> Option<String> {
    let show = |t: &wolf_types::Ty| wolf_types::show(t, &p.resolved, &p.typed);
    let (code, docs) = match named {
        Named::Local(l) => {
            let name = &p.resolved.locals.get(l.0 as usize)?.name;
            match p.typed.local_types.get(&l) {
                Some(t) => (format!("{name}: {}", show(t)), Vec::new()),
                None => (name.clone(), Vec::new()),
            }
        }
        Named::Module(m) => {
            let path = p.resolved.modules.get(m.0 as usize)?.path.join(".");
            (format!("module {path}"), Vec::new())
        }
        Named::Def(d) => {
            let def = p.resolved.def(d)?;
            let (params, docs) = declared(p, def.span);
            let name = &def.name;
            let code = match def.kind {
                DefKind::Fn | DefKind::ExternFn | DefKind::Member { .. } => {
                    match p.typed.decls.fns.get(&d) {
                        Some(sig) => wolf_types::show_fn(name, sig, &params, &p.resolved, &p.typed),
                        None => format!("fn {name}"),
                    }
                }
                DefKind::Struct => {
                    let fields: Vec<String> = p
                        .typed
                        .decls
                        .fields
                        .get(&d)
                        .map(|fs| {
                            fs.iter()
                                .map(|f| format!("{}: {}", f.name, show(&f.ty)))
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("struct {name} {{ {} }}", fields.join(", "))
                }
                DefKind::Enum => {
                    let variants: Vec<String> = p
                        .typed
                        .decls
                        .enum_variants
                        .get(&d)
                        .map(|vs| {
                            vs.iter()
                                .filter_map(|v| p.resolved.def(*v).map(|v| v.name.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("enum {name} {{ {} }}", variants.join(", "))
                }
                DefKind::Variant { parent, .. } => {
                    let owner = p
                        .resolved
                        .def(parent)
                        .map(|e| e.name.clone())
                        .unwrap_or_default();
                    let payload = match p.typed.decls.variants.get(&d).map(|v| &v.shape) {
                        Some(Shape::Tuple(ts)) => {
                            let ts: Vec<String> = ts.iter().map(show).collect();
                            format!("({})", ts.join(", "))
                        }
                        Some(Shape::Struct(fs)) => {
                            let fs: Vec<String> = fs
                                .iter()
                                .map(|f| format!("{}: {}", f.name, show(&f.ty)))
                                .collect();
                            format!(" {{ {} }}", fs.join(", "))
                        }
                        _ => String::new(),
                    };
                    format!("{owner}.{name}{payload}")
                }
                DefKind::Interface => format!("interface {name}"),
                DefKind::Alias => format!("type {name}"),
                DefKind::Config => match p.typed.decls.consts.get(&d) {
                    Some(t) => format!("{name}: {}", show(t)),
                    None => name.clone(),
                },
                DefKind::IntrinsicType | DefKind::ExternType => format!("type {name}"),
            };
            (code, docs)
        }
    };
    let mut text = format!("```wolf\n{code}\n```");
    if !docs.is_empty() {
        text.push_str("\n\n");
        text.push_str(&docs.join("\n"));
    }
    Some(text)
}

/// The parameter names and the doc comments of the declaration whose name is at `span`.
fn declared(p: &Project, span: Span) -> (Vec<String>, Vec<String>) {
    let names = |sig: &wolf_ast::FnSig| -> Vec<String> {
        sig.params
            .iter()
            .filter_map(|param| match &param.kind {
                wolf_ast::ParamKind::Named { name, .. } => Some(name.name.clone()),
                wolf_ast::ParamKind::SelfParam => None,
            })
            .collect()
    };
    let docs = |docs: &[wolf_ast::Doc]| -> Vec<String> {
        docs.iter().map(|d| d.text.trim().to_string()).collect()
    };
    for (file, ast) in &p.parsed {
        if *file != span.file {
            continue;
        }
        for item in ast.sections.iter().flat_map(|s| &s.items) {
            match &item.kind {
                wolf_ast::ItemKind::Fn(f) if f.sig.name.span == span => {
                    return (names(&f.sig), docs(&item.docs));
                }
                wolf_ast::ItemKind::Impl(block) => {
                    for m in &block.methods {
                        if m.decl.sig.name.span == span {
                            return (names(&m.decl.sig), docs(&m.docs));
                        }
                    }
                }
                _ if item.span.start <= span.start && span.end <= item.span.end => {
                    return (Vec::new(), docs(&item.docs));
                }
                _ => {}
            }
        }
    }
    (Vec::new(), Vec::new())
}
