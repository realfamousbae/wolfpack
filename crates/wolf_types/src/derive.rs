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
            Ty::Named(def, _) => {
                def == interface
                    || self
                        .impls
                        .get(&def)
                        .is_some_and(|list| list.iter().any(|i| i.interface == interface))
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
        if self.opaque(ty) {
            self.report(
                Diagnostic::new(
                    codes::E0707,
                    format!("`{shown}` is an extern type and is not `{iface}`"),
                )
                .primary(span, format!("{why} needs `{iface}`"))
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
                    .primary(span, format!("{why} needs `{iface}`"));
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
            if self.is(ty, self.b.float) && Some(interface) == self.b.comparable {
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
