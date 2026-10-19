//! Checks of `extern` declarations (spec chapter 7, section 8): the path strings (E0705), the
//! types at the boundary (E0702), generic functions and `mut` parameters (E0706), names declared
//! twice in one block (E0708) and different signatures in different blocks (E0703).

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{self as ast, ExternDeclKind, Mode, ParamKind};
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind};
use wolf_span::Span;

use crate::env::{Checker, Scope, Sig};
use crate::ty::Ty;

/// The first declaration of an extern function: its target, signature and span.
pub(crate) type FirstDecl = (String, Sig, Span);

impl Checker<'_> {
    /// Checks one target block of the module `m` and records the signatures of its functions.
    pub(crate) fn extern_block(
        &mut self,
        m: usize,
        target: &ast::ExternTarget,
        scope: &Scope,
        first: &mut BTreeMap<DefId, FirstDecl>,
    ) {
        let t = target.target.name.as_str();
        let mut seen = BTreeSet::new();
        for decl in &target.decls {
            let (name, is_type) = match &decl.kind {
                ExternDeclKind::Fn(sig) => (&sig.name, false),
                ExternDeclKind::Type(name) => (name, true),
            };
            if !seen.insert(name.name.clone()) {
                self.report(
                    Diagnostic::new(
                        codes::E0708,
                        format!("`{}` is declared twice in the `{t}` block", name.name),
                    )
                    .primary(name.span, "declared again here")
                    .help("remove one of the declarations"),
                );
                continue;
            }
            if !path_ok(t, &decl.from.value, is_type) {
                self.report(
                    Diagnostic::new(
                        codes::E0705,
                        format!(
                            "`{}` is not a {} path of {}",
                            decl.from.value,
                            what(is_type),
                            target_name(t)
                        ),
                    )
                    .primary(decl.from.span, "malformed path")
                    .help(path_help(t, &decl.from.value, is_type)),
                );
            }
            let ExternDeclKind::Fn(sig) = &decl.kind else {
                continue;
            };
            let Some(def) = self.module_item(m, &sig.name.name) else {
                continue;
            };
            let mut valid = true;
            if let Some(g) = sig.generics.first() {
                valid = false;
                self.report(
                    Diagnostic::new(
                        codes::E0706,
                        format!("the extern function `{}` is generic", sig.name.name),
                    )
                    .primary(g.name.span, "type parameters are not allowed here")
                    .note("extern functions are not generic: declare one function per type"),
                );
            }
            for p in &sig.params {
                if matches!(p.kind, ParamKind::SelfParam) {
                    valid = false;
                    self.report(
                        Diagnostic::new(
                            codes::E0706,
                            format!("the extern function `{}` has a `self` parameter", sig.name.name),
                        )
                        .primary(p.span, "not allowed in an extern function")
                        .help("a method of the target is declared as a function whose first parameter is the receiver"),
                    );
                } else if p.mode == Mode::Mut {
                    valid = false;
                    self.report(
                        Diagnostic::new(
                            codes::E0706,
                            format!("the extern function `{}` has a `mut` parameter", sig.name.name),
                        )
                        .primary(p.span, "`mut` parameter")
                        .note("extern code cannot change wolfpack values: return the new value instead"),
                    );
                }
            }
            let lowered = self.lower_sig(sig, scope);
            let named = sig.params.iter().filter_map(|p| match &p.kind {
                ParamKind::Named { ty, .. } => Some(ty.span),
                ParamKind::SelfParam => None,
            });
            let mut types: Vec<(Ty, Span)> = lowered
                .params
                .iter()
                .map(|(_, t)| t.clone())
                .zip(named)
                .collect();
            if let Some(ret) = &sig.ret {
                types.push((lowered.ret.clone(), ret.span));
            }
            for (ty, span) in types {
                // The types of a generic function are already reported with E0706.
                if !sig.generics.is_empty() {
                    break;
                }
                if !self.crosses(&ty) {
                    valid = false;
                    let shown = self.show(&ty);
                    self.report(
                        Diagnostic::new(
                            codes::E0702,
                            format!("`{shown}` cannot cross the extern boundary"),
                        )
                        .primary(span, "not allowed in an extern signature")
                        .note("extern signatures use `Int`, `Float`, `Bool`, `String`, `Void`, `List`, `Option`, `Result<_, Error>` and extern types"),
                    );
                }
            }
            match first.get(&def) {
                Some((t0, sig0, span0)) => {
                    if valid && !same_sig(sig0, &lowered) {
                        let a = self.show_extern(&sig.name.name, sig0);
                        let b = self.show_extern(&sig.name.name, &lowered);
                        let (t0, span0) = (t0.clone(), *span0);
                        self.report(
                            Diagnostic::new(
                                codes::E0703,
                                format!(
                                    "`{}` has different signatures for `{t0}` and `{t}`",
                                    sig.name.name
                                ),
                            )
                            .primary(sig.span, format!("`{t}`: {b}"))
                            .secondary(span0, format!("`{t0}`: {a}"))
                            .help(
                                "declare the same parameter and result types in every target block",
                            ),
                        );
                    }
                }
                None => {
                    if valid {
                        self.fns.entry(def).or_insert(lowered.clone());
                        first.insert(def, (t.to_string(), lowered, sig.span));
                    }
                }
            }
        }
    }

    /// The declaration `name` of the module `m`.
    fn module_item(&self, m: usize, name: &str) -> Option<DefId> {
        self.resolved
            .modules
            .get(m)
            .and_then(|info| info.items.get(name).copied())
    }

    /// Whether a type may appear in an extern signature (chapter 7, section 8.4).
    fn crosses(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Error => true,
            Ty::Named(d, args) => {
                let d = Some(*d);
                if [
                    self.b.int,
                    self.b.float,
                    self.b.bool,
                    self.b.string,
                    self.b.void,
                ]
                .contains(&d)
                {
                    true
                } else if d == self.b.list || d == self.b.option {
                    args.iter().all(|a| self.crosses(a))
                } else if d == self.b.result {
                    match args.as_slice() {
                        [ok, err] => self.crosses(ok) && self.is_error_iface(err),
                        _ => false,
                    }
                } else {
                    d.and_then(|d| self.def_kind(d)) == Some(DefKind::ExternType)
                }
            }
            _ => false,
        }
    }

    fn is_error_iface(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Error) || self.is(ty, self.b.error)
    }

    fn show_extern(&self, name: &str, sig: &Sig) -> String {
        let params: Vec<String> = sig.params.iter().map(|(_, t)| self.show(t)).collect();
        if self.is(&sig.ret, self.b.void) {
            format!("fn {name}({})", params.join(", "))
        } else {
            format!(
                "fn {name}({}) -> {}",
                params.join(", "),
                self.show(&sig.ret)
            )
        }
    }
}

fn same_sig(a: &Sig, b: &Sig) -> bool {
    a.params == b.params && a.ret == b.ret
}

fn what(is_type: bool) -> &'static str {
    if is_type { "type" } else { "function" }
}

fn target_name(t: &str) -> &'static str {
    match t {
        "go" => "Go",
        "ts" => "TypeScript",
        _ => "Rust",
    }
}

fn path_help(t: &str, path: &str, is_type: bool) -> &'static str {
    if path.starts_with(NATIVE) {
        return match t {
            "go" => "write `native:<package dir>.Name`, such as `native:slug.Slugify`",
            "ts" => {
                "write `native:<file>#<export>` without `.ts`, such as `native:text/slug#slugify`"
            }
            _ => "write `native:<module path>::<name>`, such as `native:slug::slugify`",
        };
    }
    match (t, is_type) {
        ("go", false) => {
            "write `import/path.Name`, or a method expression: `(*bytes.Buffer).WriteString`"
        }
        ("go", true) => "write `import/path.Name`, or `*import/path.Name` for a pointer",
        ("ts", _) => "write `module#export`, `module#default`, or a global path such as `Math.max`",
        _ => "write a Rust path such as `std::cmp::max` or `str::to_uppercase`",
    }
}

/// The prefix of a path to code of the project in `native/<target>/` (chapter 7, section 8.2a).
const NATIVE: &str = "native:";

/// Whether `path` has the form of chapter 7, section 8.2 (or 8.2a) for the target `t`.
fn path_ok(t: &str, path: &str, is_type: bool) -> bool {
    if let Some(rest) = path.strip_prefix(NATIVE) {
        return native_path_ok(t, rest, is_type);
    }
    match t {
        "go" => go_path(path, is_type),
        "ts" => ts_path(path),
        _ => rust_path(path),
    }
}

/// A `native:` path: the form of the target, with a file, package directory or module path
/// inside `native/<target>/`. Every segment of it is an identifier, so that it can neither
/// leave the directory nor name a file that is not valid in the generated project.
fn native_path_ok(t: &str, rest: &str, is_type: bool) -> bool {
    let segments = |s: &str, sep: &str| !s.is_empty() && s.split(sep).all(|p| ident(p, false));
    match t {
        "ts" => rest
            .split_once('#')
            .is_some_and(|(file, export)| segments(file, "/") && ident(export, true)),
        "go" => {
            let rest = if is_type {
                rest.strip_prefix('*').unwrap_or(rest)
            } else {
                rest
            };
            let check = |p: &str| go_qualified(p).is_some_and(|(pkg, _)| segments(pkg, "/"));
            if !is_type {
                if let Some(inner) = rest.strip_prefix('(') {
                    let inner = inner.split_once(").").map_or("", |(i, _)| i);
                    let inner = inner.strip_prefix('*').unwrap_or(inner);
                    return check(inner) && go_path(rest, false);
                }
            }
            check(rest) && go_path(rest, is_type)
        }
        _ => rust_path(rest) && rest.contains("::"),
    }
}

fn ident(s: &str, dollar: bool) -> bool {
    let mut chars = s.chars();
    let ok = |c: char| c == '_' || (dollar && c == '$');
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || ok(c))
        && chars.all(|c| c.is_ascii_alphanumeric() || ok(c))
}

/// `import/path.Name`: the package is everything before the first `.` after the last `/`.
fn go_qualified(path: &str) -> Option<(&str, &str)> {
    let start = path.rfind('/').map_or(0, |i| i + 1);
    let dot = path[start..].find('.')? + start;
    let (pkg, rest) = (&path[..dot], &path[dot + 1..]);
    let pkg_ok = !pkg.is_empty()
        && pkg.split('/').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~'))
        });
    pkg_ok.then_some((pkg, rest))
}

fn go_path(path: &str, is_type: bool) -> bool {
    if is_type {
        let path = path.strip_prefix('*').unwrap_or(path);
        return go_qualified(path).is_some_and(|(_, name)| ident(name, false));
    }
    // A method expression: `(*pkg.Type).Method` or `(pkg.Type).Method`.
    if let Some(rest) = path.strip_prefix('(') {
        let Some((inner, method)) = rest.split_once(").") else {
            return false;
        };
        let inner = inner.strip_prefix('*').unwrap_or(inner);
        return ident(method, false)
            && go_qualified(inner).is_some_and(|(_, name)| ident(name, false));
    }
    // `pkg.Func` or `pkg.Type.Method`.
    go_qualified(path).is_some_and(|(_, rest)| {
        let parts: Vec<&str> = rest.split('.').collect();
        parts.len() <= 2 && parts.iter().all(|p| ident(p, false))
    })
}

fn ts_path(path: &str) -> bool {
    if let Some((module, export)) = path.split_once('#') {
        return !module.is_empty() && !module.contains(char::is_whitespace) && ident(export, true);
    }
    !path.is_empty() && path.split('.').all(|p| ident(p, true))
}

fn rust_path(path: &str) -> bool {
    !path.is_empty() && path.split("::").all(|p| ident(p, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_forms() {
        assert!(go_path("strings.ToUpper", false));
        assert!(go_path(
            "github.com/agnivade/levenshtein.ComputeDistance",
            false
        ));
        assert!(go_path("(*bytes.Buffer).WriteString", false));
        assert!(go_path("time.Duration.String", false));
        assert!(go_path("*regexp.Regexp", true));
        assert!(!go_path("ToUpper", false));
        assert!(!go_path("strings.", false));
        assert!(!go_path("**regexp.Regexp", true));
        assert!(ts_path("fastest-levenshtein#distance"));
        assert!(ts_path("some-lib#default"));
        assert!(ts_path("String.prototype.toUpperCase"));
        assert!(!ts_path("#parse"));
        assert!(!ts_path("Math..max"));
        assert!(rust_path("strsim::levenshtein"));
        assert!(rust_path("str::to_uppercase"));
        assert!(!rust_path("strsim:levenshtein"));
        assert!(!rust_path("a::"));
    }

    #[test]
    fn native_path_forms() {
        assert!(path_ok("ts", "native:slug#slugify", false));
        assert!(path_ok("ts", "native:text/slug#slugify", false));
        assert!(path_ok("ts", "native:search#Index", true));
        assert!(!path_ok("ts", "native:slug", false));
        assert!(!path_ok("ts", "native:slug.ts#slugify", false));
        assert!(!path_ok("ts", "native:../slug#slugify", false));
        assert!(!path_ok("ts", "native:/slug#slugify", false));
        assert!(!path_ok("ts", "native:#slugify", false));
        assert!(path_ok("go", "native:slug.Slugify", false));
        assert!(path_ok("go", "native:text/slug.Slugify", false));
        assert!(path_ok("go", "native:search.Index.Add", false));
        assert!(path_ok("go", "native:(*search.Index).Add", false));
        assert!(!path_ok("go", "*native:search.Index", true));
        assert!(path_ok("go", "native:*search.Index", true));
        assert!(!path_ok("go", "native:my-slug.Slugify", false));
        assert!(!path_ok("go", "native:../slug.Slugify", false));
        assert!(!path_ok("go", "native:Slugify", false));
        assert!(path_ok("rust", "native:slug::slugify", false));
        assert!(path_ok("rust", "native:text::slug::slugify", false));
        assert!(!path_ok("rust", "native:slugify", false));
        assert!(!path_ok("rust", "native:slug:slugify", false));
        assert!(!path_ok("rust", "native:", false));
    }
}
