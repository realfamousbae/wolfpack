//! Checks of chapter 6 that need resolved names: module lines, entry points, sections of test
//! modules, visibility of types in signatures, and `[config]` overrides.

use std::collections::BTreeSet;

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_project::{ConfigValue, Kind};
use wolf_span::Span;

use crate::collect::{Binding, Resolver, join};
use crate::{DefId, DefKind, ModuleId, ModuleInput, Res, Role};

pub(crate) fn run(r: &mut Resolver<'_>) {
    let inputs = r.inputs;
    for (index, input) in inputs.iter().enumerate() {
        let module = ModuleId(index as u32);
        module_line(r, input);
        intrinsics(r, input);
        test_sections(r, input);
        entry(r, input);
        signatures(r, module, input);
    }
    library_api(r);
    config_overrides(r);
}

/// The `module` line states the path of the file (E0610).
fn module_line(r: &mut Resolver<'_>, input: &ModuleInput<'_>) {
    let Some(path) = &input.ast.module else {
        return;
    };
    let written: Vec<String> = path.segments.iter().map(|s| s.name.clone()).collect();
    if written != input.path {
        r.report(
            Diagnostic::new(
                codes::E0610,
                format!(
                    "module `{}` is declared as `{}`",
                    join(&input.path),
                    join(&written)
                ),
            )
            .primary(
                path.span,
                format!("the file path gives `{}`", join(&input.path)),
            )
            .help(format!("write `module {}`", join(&input.path))),
        );
    }
}

/// `intrinsic` functions, methods and types exist only in std (E0701).
fn intrinsics(r: &mut Resolver<'_>, input: &ModuleInput<'_>) {
    if matches!(input.role, Role::Std | Role::Prelude) {
        return;
    }
    let mut spans: Vec<Span> = Vec::new();
    for section in &input.ast.sections {
        for item in &section.items {
            match &item.kind {
                ItemKind::Fn(decl) if decl.intrinsic => spans.push(decl.sig.name.span),
                ItemKind::IntrinsicType(decl) => spans.push(decl.name.span),
                ItemKind::Impl(block) => spans.extend(
                    block
                        .methods
                        .iter()
                        .filter(|m| m.decl.intrinsic)
                        .map(|m| m.decl.sig.name.span),
                ),
                _ => {}
            }
        }
    }
    for span in spans {
        r.report(
            Diagnostic::new(codes::E0701, "`intrinsic` outside `std`")
                .primary(
                    span,
                    "only the standard library declares intrinsic functions and types",
                )
                .help("give the function a body, or use `extern` for code of the target language"),
        );
    }
}

/// Modules of `tests/` contain only `imports`, `functions` and `tests` (E0619).
fn test_sections(r: &mut Resolver<'_>, input: &ModuleInput<'_>) {
    if input.role != Role::Test {
        return;
    }
    for section in &input.ast.sections {
        if !matches!(
            section.kind,
            SectionKind::Imports | SectionKind::Functions | SectionKind::Tests
        ) {
            r.report(
                Diagnostic::new(
                    codes::E0619,
                    format!("section `{}` in a module of `tests/`", section.kind.name()),
                )
                .primary(section.name_span, "not allowed in integration tests")
                .help("integration tests contain only `imports`, `functions` and `tests`; move declarations into `src/`"),
            );
        }
    }
}

/// The `main` section in programs and examples: at most the parameter `args: List<String>` and
/// the result `Result<Void, Error>` (E0614).
fn entry(r: &mut Resolver<'_>, input: &ModuleInput<'_>) {
    let needs_main = match input.role {
        Role::Entry => r.project.kind == Kind::Cli && input.path == ["main"],
        Role::Example => true,
        _ => false,
    };
    let is_lib = input.role == Role::Entry && input.path == ["lib"];
    let mains: Vec<&FnDecl> = input
        .ast
        .sections
        .iter()
        .filter(|s| s.kind == SectionKind::Main)
        .flat_map(|s| s.items.iter())
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(decl) if decl.sig.name.name == "main" => Some(decl),
            _ => None,
        })
        .collect();
    if is_lib {
        for main in mains {
            r.report(
                Diagnostic::new(codes::E0614, "a library has no `main` section")
                    .primary(main.sig.name.span, "`main` is only for programs")
                    .help("remove the section"),
            );
        }
        return;
    }
    if !needs_main {
        return;
    }
    let Some(main) = mains.first() else {
        let span = input.ast.module.as_ref().map(|p| p.span);
        let mut d = Diagnostic::new(
            codes::E0614,
            format!("missing `main` section in `{}`", join(&input.path)),
        )
        .help("add a `main { }` section above `functions`");
        if let Some(span) = span {
            d = d.primary(span, "this module needs a `main` section");
        }
        r.report(d);
        return;
    };
    let span = main.sig.name.span;
    if main.public {
        r.report(
            Diagnostic::new(codes::E0614, "`main` cannot be `pub`").primary(span, "remove `pub`"),
        );
    }
    let result_ok = match &main.sig.ret {
        None => true,
        Some(ty) => is_result_void_error(ty),
    };
    let params_ok = match main.sig.params.as_slice() {
        [] => true,
        [p] => {
            p.mode == Mode::Read
                && matches!(&p.kind, ParamKind::Named { ty, .. } if is_list_of_string(ty))
        }
        _ => false,
    };
    if !params_ok || !result_ok {
        r.report(
            Diagnostic::new(codes::E0614, "wrong header of `main`")
                .primary(main.sig.span, "`main` takes only `args: List<String>` and returns `Void` or `Result<Void, Error>`")
                .help("write `main`, `main(args)`, `main -> Result<Void, Error>` or `main(args) -> Result<Void, Error>`"),
        );
    }
}

fn is_list_of_string(ty: &Ty) -> bool {
    let TyKind::Path { path, args } = &ty.kind else {
        return false;
    };
    path.segments.len() == 1
        && path.segments.first().is_some_and(|s| s.name == "List")
        && matches!(
            args.as_slice(),
            [Ty { kind: TyKind::Path { path, args }, .. }]
                if args.is_empty()
                    && path.segments.len() == 1
                    && path.segments.first().is_some_and(|s| s.name == "String")
        )
}

fn is_result_void_error(ty: &Ty) -> bool {
    let TyKind::Path { path, args } = &ty.kind else {
        return false;
    };
    let name = |t: &Ty| match &t.kind {
        TyKind::Path { path, args } if args.is_empty() && path.segments.len() == 1 => {
            path.segments.first().map(|s| s.name.clone())
        }
        _ => None,
    };
    path.segments.len() == 1
        && path.segments.first().is_some_and(|s| s.name == "Result")
        && args.len() == 2
        && args.first().and_then(name).as_deref() == Some("Void")
        && args.get(1).and_then(name).as_deref() == Some("Error")
}

/// A `pub` declaration may only mention types visible wherever it is visible (E0608).
fn signatures(r: &mut Resolver<'_>, module: ModuleId, input: &ModuleInput<'_>) {
    if matches!(input.role, Role::Std | Role::Prelude) {
        return;
    }
    let file = input.file;
    for section in &input.ast.sections {
        for item in &section.items {
            let mut types: Vec<&Ty> = Vec::new();
            let what = match &item.kind {
                ItemKind::Fn(decl) if decl.public => {
                    sig_types(&decl.sig, &mut types);
                    "function"
                }
                ItemKind::Struct(decl) if decl.public => {
                    for field in decl.fields.iter().filter(|f| f.public) {
                        types.push(&field.ty);
                    }
                    "field"
                }
                ItemKind::Enum(decl) if decl.public => {
                    for variant in &decl.variants {
                        match &variant.payload {
                            VariantPayload::Tuple(list) => types.extend(list.iter()),
                            VariantPayload::Struct(fields) => {
                                types.extend(fields.iter().map(|f| &f.ty))
                            }
                            VariantPayload::None => {}
                        }
                    }
                    "variant"
                }
                ItemKind::Alias(decl) if decl.public => {
                    types.push(&decl.ty);
                    "type alias"
                }
                ItemKind::Config(entry) if entry.public => {
                    types.push(&entry.ty);
                    "constant"
                }
                ItemKind::Interface(decl) if decl.public => {
                    for method in &decl.methods {
                        sig_types(&method.decl.sig, &mut types);
                    }
                    "interface"
                }
                ItemKind::Impl(block) if block.interface.is_none() => {
                    // Methods of a private type are visible only where the type is.
                    let owner_public = lookup_type(r, module, &block.type_name.name)
                        .and_then(|d| r.def(d))
                        .is_some_and(|d| d.public);
                    if !owner_public {
                        continue;
                    }
                    for method in block.methods.iter().filter(|m| m.decl.public) {
                        sig_types(&method.decl.sig, &mut types);
                    }
                    "method"
                }
                _ => continue,
            };
            let mut all = Vec::new();
            for ty in types {
                collect_types(ty, &mut all);
            }
            for ty in all {
                let Some(Res::Def(def)) = r.res.get(&(file, ty.id)).copied() else {
                    continue;
                };
                let private = r.def(def).is_some_and(|d| d.module == module && !d.public);
                if private {
                    let name = r.def(def).map(|d| d.name.clone()).unwrap_or_default();
                    r.report(
                        Diagnostic::new(
                            codes::E0608,
                            format!("private type `{name}` in a public {what}"),
                        )
                        .primary(ty.span, "this type is private to the module")
                        .help(format!(
                            "mark `{name}` as `pub`, or make the {what} private"
                        )),
                    );
                }
            }
        }
    }
}

fn lookup_type(r: &Resolver<'_>, module: ModuleId, name: &str) -> Option<DefId> {
    r.module(module).and_then(|m| m.items.get(name).copied())
}

/// The types in the public parts of a declaration: what other modules can see of it.
fn public_types<'a>(item: &'a Item, out: &mut Vec<&'a Ty>) {
    match &item.kind {
        ItemKind::Fn(decl) => sig_types(&decl.sig, out),
        ItemKind::Struct(decl) => {
            out.extend(decl.fields.iter().filter(|f| f.public).map(|f| &f.ty))
        }
        ItemKind::Enum(decl) => {
            for variant in &decl.variants {
                match &variant.payload {
                    VariantPayload::Tuple(list) => out.extend(list.iter()),
                    VariantPayload::Struct(fields) => out.extend(fields.iter().map(|f| &f.ty)),
                    VariantPayload::None => {}
                }
            }
        }
        ItemKind::Alias(decl) => out.push(&decl.ty),
        ItemKind::Config(entry) => out.push(&entry.ty),
        ItemKind::Interface(decl) => {
            for method in &decl.methods {
                sig_types(&method.decl.sig, out);
            }
        }
        _ => {}
    }
}

/// The name span of a declaration, to match it with its definition.
fn name_span(item: &Item) -> Option<Span> {
    Some(match &item.kind {
        ItemKind::Fn(decl) => decl.sig.name.span,
        ItemKind::Struct(decl) => decl.name.span,
        ItemKind::Enum(decl) => decl.name.span,
        ItemKind::Alias(decl) => decl.name.span,
        ItemKind::Config(entry) => entry.name.span,
        ItemKind::Interface(decl) => decl.name.span,
        _ => return None,
    })
}

fn sig_types<'a>(sig: &'a FnSig, out: &mut Vec<&'a Ty>) {
    for param in &sig.params {
        if let ParamKind::Named { ty, .. } = &param.kind {
            out.push(ty);
        }
    }
    if let Some(ret) = &sig.ret {
        out.push(ret);
    }
    for generic in sig.generics.iter().chain(&sig.where_bounds) {
        out.extend(generic.bounds.iter());
    }
}

/// A type and all types nested in it.
fn collect_types<'a>(ty: &'a Ty, out: &mut Vec<&'a Ty>) {
    out.push(ty);
    match &ty.kind {
        TyKind::Path { args, .. } => {
            for arg in args {
                collect_types(arg, out);
            }
        }
        TyKind::Fn { params, ret } => {
            for (_, param) in params {
                collect_types(param, out);
            }
            if let Some(ret) = ret {
                collect_types(ret, out);
            }
        }
        TyKind::SelfType | TyKind::Error => {}
    }
}

/// Every project type in a signature of the library API must be part of the API (E0608).
fn library_api(r: &mut Resolver<'_>) {
    if r.project.kind != Kind::Lib {
        return;
    }
    let Some(lib) = r.by_path.get("lib").copied() else {
        return;
    };
    let exported: BTreeSet<DefId> = r
        .module(lib)
        .map(|m| {
            m.exports
                .values()
                .filter_map(|b| match b {
                    Binding::Def(d) => Some(*d),
                    Binding::Module(_) => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let inputs = r.inputs;
    let mut problems: Vec<(Span, String, String)> = Vec::new();
    for &def in &exported {
        let Some(d) = r.def(def).cloned() else {
            continue;
        };
        if r.is_std(d.module) {
            continue;
        }
        let Some(input) = inputs.get(d.module.0 as usize) else {
            continue;
        };
        let mut types: Vec<&Ty> = Vec::new();
        for section in &input.ast.sections {
            for item in &section.items {
                if name_span(item) == Some(d.span) {
                    public_types(item, &mut types);
                }
                // The `pub` methods of an exported type are part of the API too.
                if let ItemKind::Impl(block) = &item.kind {
                    if block.interface.is_none()
                        && block.type_name.name == d.name
                        && d.kind.is_type()
                    {
                        for method in block.methods.iter().filter(|m| m.decl.public) {
                            sig_types(&method.decl.sig, &mut types);
                        }
                    }
                }
            }
        }
        let mut all = Vec::new();
        for ty in types {
            collect_types(ty, &mut all);
        }
        for ty in all {
            let Some(Res::Def(used)) = r.res.get(&(input.file, ty.id)).copied() else {
                continue;
            };
            let Some(used_def) = r.def(used) else {
                continue;
            };
            if r.is_std(used_def.module) || exported.contains(&used) {
                continue;
            }
            problems.push((ty.span, used_def.name.clone(), d.name.clone()));
        }
    }
    for (span, name, user) in problems {
        r.report(
            Diagnostic::new(
                codes::E0608,
                format!("type `{name}` is used by the library API but not exported"),
            )
            .primary(span, format!("used by `{user}`, which `lib` exports"))
            .help(format!(
                "re-export `{name}` in `src/lib.wolf` with a `pub` import"
            )),
        );
    }
}

/// `[config.<module>]` overrides name existing constants of the right type (E0613).
fn config_overrides(r: &mut Resolver<'_>) {
    let overrides = r.project.config;
    for o in overrides {
        let path = o.module.join(".");
        let module = r.by_path.get(&path).copied().filter(|m| {
            r.module(*m)
                .is_some_and(|s| matches!(s.role, Role::Src | Role::Entry))
        });
        let Some(module) = module else {
            r.report(
                Diagnostic::new(
                    codes::E0613,
                    format!("config override for unknown module `{path}`"),
                )
                .primary(o.key_span, "no module with this path in `src/`"),
            );
            continue;
        };
        let def = r
            .module(module)
            .and_then(|m| m.items.get(&o.name).copied())
            .filter(|d| r.def(*d).is_some_and(|def| def.kind == DefKind::Config));
        let Some(def) = def else {
            r.report(
                Diagnostic::new(
                    codes::E0613,
                    format!("`{path}` has no constant `{}`", o.name),
                )
                .primary(o.key_span, "unknown constant"),
            );
            continue;
        };
        let declared = declared_type(r, module, def);
        let expected = o.value.type_name();
        // A constant whose type is an alias (`type Port = Int`) is checked with types (M4).
        let builtin = declared
            .as_deref()
            .is_some_and(|t| matches!(t, "Int" | "Float" | "Bool" | "String"));
        if let Some(declared) = declared.filter(|_| builtin) {
            if declared != expected {
                let hint = match (&o.value, declared.as_str()) {
                    (ConfigValue::Int(_), "Float") => " (write a float such as `1.0`)",
                    _ => "",
                };
                r.report(
                    Diagnostic::new(
                        codes::E0613,
                        format!("constant `{path}.{}` is `{declared}`, but the override is `{expected}`", o.name),
                    )
                    .primary(o.value_span, format!("expected a value of type `{declared}`{hint}")),
                );
            }
        }
    }
}

/// The written type name of a constant, if it is a plain name.
fn declared_type(r: &Resolver<'_>, module: ModuleId, def: DefId) -> Option<String> {
    let input = r.inputs.get(module.0 as usize)?;
    let span = r.def(def)?.span;
    for section in &input.ast.sections {
        for item in &section.items {
            if let ItemKind::Config(entry) = &item.kind {
                if entry.name.span == span {
                    if let TyKind::Path { path, args } = &entry.ty.kind {
                        if args.is_empty() && path.segments.len() == 1 {
                            return path.segments.first().map(|s| s.name.clone());
                        }
                    }
                }
            }
        }
    }
    None
}
