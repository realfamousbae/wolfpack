//! Built-in interfaces (chapter 4, section 9): `Equatable` and `Copyable` are derived from the
//! fields of a type, `Hashable` is a closed set, other interfaces are implemented by `impl`.

use std::collections::BTreeSet;

use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind};
use wolf_span::Span;

use crate::env::{Checker, Shape};
use crate::ty::{ParamId, Ty};

/// Which derived interface is being checked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Derived {
    Equatable,
    Copyable,
}

impl Checker<'_> {
    /// Whether a type implements an interface through an `impl` block (or is the interface).
    pub(crate) fn implements(&self, ty: &Ty, interface: DefId) -> bool {
        match self.infer.resolve(ty) {
            Ty::Named(def, args) => {
                if def == interface {
                    return true;
                }
                let implemented = self.impls.get(&def).is_some_and(|list| {
                    list.iter()
                        .any(|i| i.interface == interface && self.head_holds(&i.bounds, &args))
                });
                implemented && (!self.result_display(def, interface) || self.err_shown(&args))
            }
            Ty::Param(ParamId::Decl(span)) => self.has_bound(span, interface),
            Ty::Error | Ty::Never => true,
            _ => false,
        }
    }

    /// Unifies the type arguments of an interface type with those of the implementation of a
    /// concrete type: coercing `Stack<Int>` to `Container<T>` fixes `T` to `Int`.
    pub(crate) fn unify_impl_args(&mut self, concrete: &Ty, interface: &Ty) -> bool {
        let (Ty::Named(def, args), Ty::Named(iface, iargs)) =
            (self.infer.resolve(concrete), self.infer.resolve(interface))
        else {
            return true;
        };
        let Some(imp) = self
            .impls
            .get(&def)
            .and_then(|list| list.iter().find(|i| i.interface == iface))
            .cloned()
        else {
            return true;
        };
        let map: std::collections::BTreeMap<ParamId, Ty> = imp
            .generics
            .iter()
            .map(|s| ParamId::Decl(*s))
            .zip(args)
            .collect();
        let mut ok = true;
        for (a, b) in imp.args.iter().zip(&iargs) {
            ok &= self.infer.unify(&a.subst(&map), b);
        }
        ok
    }

    /// The implementation of `Display` for `Result`, whose error may also be only an `Error`.
    fn result_display(&self, def: DefId, interface: DefId) -> bool {
        Some(def) == self.b.result && Some(interface) == self.b.display
    }

    /// Whether the error type of a `Result` can be written: it is `Display` or implements
    /// `Error` (chapter 4, section 9, rule 7).
    fn err_shown(&self, args: &[Ty]) -> bool {
        let Some(e) = args.get(1) else {
            return true;
        };
        matches!(self.infer.resolve(e), Ty::Var(_))
            || self.b.display.is_some_and(|d| self.satisfies(e, d))
            || self.b.error.is_some_and(|x| self.satisfies(e, x))
    }

    /// Whether the type arguments of an implementation have the bounds that its head adds;
    /// arguments that are not known yet are not held against it.
    fn head_holds(&self, bounds: &[Vec<Ty>], args: &[Ty]) -> bool {
        bounds.iter().zip(args).all(|(bounds, arg)| {
            matches!(self.infer.resolve(arg), Ty::Var(_))
                || bounds.iter().all(|b| match b {
                    Ty::Named(iface, _) => self.satisfies(arg, *iface),
                    _ => true,
                })
        })
    }

    /// For a type whose implementation of `interface` needs bounds that its arguments do not
    /// have: "the implementation of `Display` for `Box` needs `T: Display`".
    fn head_needs(&self, ty: &Ty, interface: DefId) -> Option<String> {
        let Ty::Named(def, _) = self.infer.resolve(ty) else {
            return None;
        };
        let imp = self
            .impls
            .get(&def)?
            .iter()
            .find(|i| i.interface == interface)?;
        if self.result_display(def, interface) {
            return Some(
                "a `Result` is `Display` when its value is `Display` and its error is `Display` or implements `Error`"
                    .to_string(),
            );
        }
        let needs: Vec<String> = imp
            .generics
            .iter()
            .zip(&imp.bounds)
            .filter(|(_, bounds)| !bounds.is_empty())
            .map(|(span, bounds)| {
                let name = self.param_names.get(span).cloned().unwrap_or_default();
                let list: Vec<String> = bounds.iter().map(|b| self.show(b)).collect();
                format!("`{name}: {}`", list.join(" + "))
            })
            .collect();
        Some(format!(
            "the implementation of `{}` for `{}` needs {}",
            self.def_name(interface),
            self.def_name(def),
            needs.join(" and ")
        ))
    }

    /// Whether `ty` has the interface `interface`, derived, closed or implemented.
    pub(crate) fn satisfies(&self, ty: &Ty, interface: DefId) -> bool {
        if Some(interface) == self.b.equatable {
            self.derived(ty, Derived::Equatable, &mut BTreeSet::new())
                .is_ok()
        } else if Some(interface) == self.b.copyable {
            self.derived(ty, Derived::Copyable, &mut BTreeSet::new())
                .is_ok()
        } else if Some(interface) == self.b.hashable {
            self.hashable(ty)
        } else {
            self.implements(ty, interface)
        }
    }

    /// Whether the type parameter declared at `span` has the bound `interface`.
    pub(crate) fn has_bound(&self, span: Span, interface: DefId) -> bool {
        self.param_bounds.get(&span).is_some_and(|bounds| {
            bounds
                .iter()
                .any(|b| matches!(b, Ty::Named(d, _) if *d == interface))
        })
    }

    /// Reports an error if `ty` does not satisfy the interface `interface`; `why` says what
    /// needs it.
    pub(crate) fn require(&mut self, ty: &Ty, interface: DefId, span: Span, why: &str) {
        let shown = self.show(ty);
        let iface = self.def_name(interface);
        // "required by `f`" already says why; "`==`" needs the interface after it.
        let needs = if why.starts_with("required by") {
            why.to_string()
        } else {
            format!("{why} needs `{iface}`")
        };
        if self.opaque(ty) {
            self.report(
                Diagnostic::new(
                    codes::E0707,
                    format!("`{shown}` is an extern type and is not `{iface}`"),
                )
                .primary(span, needs.clone())
                .note(
                    "an extern type is used only through extern functions (chapter 7, section 8.3)",
                ),
            );
            return;
        }
        if Some(interface) == self.b.equatable || Some(interface) == self.b.copyable {
            let which = if Some(interface) == self.b.equatable {
                Derived::Equatable
            } else {
                Derived::Copyable
            };
            if let Err(reason) = self.derived(ty, which, &mut BTreeSet::new()) {
                let d = Diagnostic::new(codes::E0406, format!("`{shown}` is not `{iface}`"))
                    .primary(span, needs.clone());
                let d = if matches!(self.infer.resolve(ty), Ty::Param(_)) {
                    d.help(format!(
                        "add the bound where `{shown}` is declared: `{shown}: {iface}`"
                    ))
                } else {
                    d.note(format!("`{shown}` is not `{iface}` because {reason}"))
                };
                self.report(d);
            }
            return;
        }
        if Some(interface) == self.b.hashable {
            if !self.hashable(ty) {
                self.report(
                    Diagnostic::new(codes::E0407, format!("`{shown}` cannot be a map key"))
                        .primary(span, format!("{why} must be `Hashable`"))
                        .note("map keys are `Int`, `String`, `Bool` and enums without payloads"),
                );
            }
            return;
        }
        if !self.implements(ty, interface) {
            let mut d = Diagnostic::new(
                codes::E0404,
                format!("`{shown}` does not implement `{iface}`"),
            )
            .primary(span, why.to_string());
            if let Some(needs) = self.head_needs(ty, interface) {
                d = d.note(needs);
            } else if self.is(ty, self.b.float) && Some(interface) == self.b.comparable {
                d = d.note("`Float` has no total order because of NaN; sort floats with `list.sort_by` and a comparison function");
            } else if self.user_type(ty) {
                d = d.help(format!(
                    "implement `{iface}` for `{shown}` in an `impl` block"
                ));
            }
            self.report(d);
        }
    }

    /// Whether the type is an opaque extern type.
    /// E0713 for a member of an opaque type of a native module that the compiler could not map,
    /// with the reason (spec chapter 7, section 8.7, rule 17); `None` for other members.
    pub(crate) fn native_member(
        &self,
        owner: DefId,
        name: &str,
        span: wolf_span::Span,
    ) -> Option<Diagnostic> {
        let def = self.resolved.def(owner)?;
        let module = self.resolved.modules.get(def.module.0 as usize)?;
        let path = module.path.join(".");
        let key = format!("{}.{name}", def.name);
        let reason = self.project.native.get(&path)?.get(&key)?;
        Some(
            Diagnostic::new(
                codes::E0713,
                format!("`{key}` of the native module `{path}` is not available"),
            )
            .primary(span, "cannot be used from wolfpack")
            .note(format!("`{key}`: {reason}")),
        )
    }

    pub(crate) fn opaque(&self, ty: &Ty) -> bool {
        matches!(self.infer.resolve(ty), Ty::Named(d, _) if self.def_kind(d) == Some(DefKind::ExternType))
    }

    /// `Equatable` or `Copyable`: the reason it fails, if it does.
    fn derived(&self, ty: &Ty, which: Derived, seen: &mut BTreeSet<DefId>) -> Result<(), String> {
        match self.infer.resolve(ty) {
            Ty::Error | Ty::Never | Ty::Var(_) => Ok(()),
            Ty::Fn(..) => Err(match which {
                Derived::Equatable => "functions cannot be compared".to_string(),
                Derived::Copyable => "functions cannot be copied".to_string(),
            }),
            Ty::Param(ParamId::Decl(span)) => {
                let bound = match which {
                    Derived::Equatable => self.b.equatable,
                    Derived::Copyable => self.b.copyable,
                };
                if bound.is_some_and(|b| self.has_bound(span, b)) {
                    return Ok(());
                }
                let name = self.param_names.get(&span).cloned().unwrap_or_default();
                let iface = bound.map(|b| self.def_name(b)).unwrap_or_default();
                Err(format!(
                    "the type parameter `{name}` has no bound `{iface}`; write `{name}: {iface}`"
                ))
            }
            Ty::Param(ParamId::SelfType) => Err("`Self` has no such bound".to_string()),
            Ty::Named(def, args) => {
                let kind = self.def_kind(def);
                match kind {
                    Some(DefKind::Interface) => {
                        if which == Derived::Equatable {
                            Err(format!(
                                "`{}` is an interface, and interface values have no `==`",
                                self.def_name(def)
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    Some(DefKind::ExternType) => {
                        Err(format!("`{}` is an extern type", self.def_name(def)))
                    }
                    Some(DefKind::IntrinsicType) => {
                        if Some(def) == self.b.list {
                            args.first()
                                .map_or(Ok(()), |a| self.derived(a, which, seen))
                        } else if Some(def) == self.b.map {
                            let from = if which == Derived::Equatable { 1 } else { 0 };
                            for a in args.iter().skip(from) {
                                self.derived(a, which, seen)?;
                            }
                            Ok(())
                        } else {
                            Ok(())
                        }
                    }
                    Some(DefKind::Struct) => {
                        if !seen.insert(def) {
                            return Ok(());
                        }
                        let map = self.param_map(def, &Ty::Named(def, args));
                        for field in self.fields.get(&def).into_iter().flatten() {
                            let t = field.ty.subst(&map);
                            if let Err(inner) = self.derived(&t, which, seen) {
                                return Err(self.because_field(&field.name, &t, inner));
                            }
                        }
                        Ok(())
                    }
                    Some(DefKind::Enum) => {
                        if !seen.insert(def) {
                            return Ok(());
                        }
                        let map = self.param_map(def, &Ty::Named(def, args));
                        for variant in self.enum_variants.get(&def).into_iter().flatten() {
                            let name = self.def_name(*variant);
                            let payload: Vec<(String, Ty)> =
                                match self.variants.get(variant).map(|v| &v.shape) {
                                    Some(Shape::Tuple(types)) => types
                                        .iter()
                                        .enumerate()
                                        .map(|(i, t)| (format!("{name}.{i}"), t.subst(&map)))
                                        .collect(),
                                    Some(Shape::Struct(fields)) => fields
                                        .iter()
                                        .map(|f| (format!("{name}.{}", f.name), f.ty.subst(&map)))
                                        .collect(),
                                    _ => Vec::new(),
                                };
                            for (label, t) in payload {
                                if let Err(inner) = self.derived(&t, which, seen) {
                                    return Err(self.because_payload(&name, &label, &t, inner));
                                }
                            }
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    /// Whether the type is declared in the project, so that it can get an `impl` block.
    fn user_type(&self, ty: &Ty) -> bool {
        let Ty::Named(def, _) = self.infer.resolve(ty) else {
            return false;
        };
        self.resolved.def(def).is_some_and(|d| {
            self.resolved
                .modules
                .get(d.module.0 as usize)
                .is_some_and(|m| {
                    !matches!(
                        m.role,
                        wolf_resolve::Role::Std | wolf_resolve::Role::Prelude
                    )
                })
        })
    }

    fn because_field(&self, field: &str, ty: &Ty, inner: String) -> String {
        if matches!(self.infer.resolve(ty), Ty::Fn(..)) {
            format!("field `{field}` has a function type")
        } else {
            format!(
                "field `{field}` has the type `{}`, and {inner}",
                self.show(ty)
            )
        }
    }

    fn because_payload(&self, variant: &str, _label: &str, ty: &Ty, inner: String) -> String {
        if matches!(self.infer.resolve(ty), Ty::Fn(..)) {
            format!("variant `{variant}` holds a function")
        } else {
            format!("variant `{variant}` holds `{}`, and {inner}", self.show(ty))
        }
    }

    /// `Int`, `String`, `Bool` and enums without payloads (G3).
    fn hashable(&self, ty: &Ty) -> bool {
        match self.infer.resolve(ty) {
            Ty::Error | Ty::Never | Ty::Var(_) => true,
            Ty::Param(ParamId::Decl(span)) => {
                self.b.hashable.is_some_and(|h| self.has_bound(span, h))
            }
            Ty::Named(def, _) => {
                if [self.b.int, self.b.string, self.b.bool].contains(&Some(def)) {
                    return true;
                }
                match self.enum_variants.get(&def) {
                    Some(variants) => variants.iter().all(|v| {
                        matches!(self.variants.get(v).map(|i| &i.shape), Some(Shape::None))
                    }),
                    None => false,
                }
            }
            _ => false,
        }
    }
}
