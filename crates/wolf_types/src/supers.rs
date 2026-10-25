//! Super-interfaces (spec chapter 4, section 8.2, rule 6, and section 8.3, rule 7): an interface
//! may name other interfaces after `:`, and a type that implements it implements them too.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{self as ast, ItemKind};
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind};
use wolf_span::Span;

use crate::env::{Checker, Scope, Sig};
use crate::ty::{ParamId, Ty};

impl<'a> Checker<'a> {
    /// Lowers the super-interfaces of every interface and checks them (E0445).
    pub(crate) fn collect_supers(&mut self) {
        let inputs = self.inputs;
        for (m, input) in inputs.iter().enumerate() {
            for section in &input.ast.sections {
                for item in &section.items {
                    let ItemKind::Interface(decl) = &item.kind else {
                        continue;
                    };
                    let Some(def) = self.def_at.get(&decl.name.span).copied() else {
                        continue;
                    };
                    let scope = Scope {
                        file: self.file_of(m),
                        self_ty: None,
                    };
                    let mut supers = Vec::new();
                    for written in &decl.supers {
                        let outer = std::mem::replace(&mut self.in_bound, true);
                        let ty = self.lower(written, &scope);
                        self.in_bound = outer;
                        if let Some(ty) = self.allowed_super(def, &ty, written.span, &supers) {
                            supers.push((ty, written.span));
                        }
                    }
                    self.interface_supers.insert(def, supers);
                }
            }
        }
        self.super_cycles();
    }

    /// The methods of every interface against those of its super-interfaces (E0447), once the
    /// methods of every interface are known.
    pub(crate) fn check_super_methods(&mut self) {
        let inputs = self.inputs;
        for input in inputs {
            for section in &input.ast.sections {
                for item in &section.items {
                    let ItemKind::Interface(decl) = &item.kind else {
                        continue;
                    };
                    if let Some(def) = self.def_at.get(&decl.name.span).copied() {
                        self.methods_of_supers(def, decl);
                    }
                }
            }
        }
    }

    /// A written super-interface, if it may be one (E0445 otherwise).
    fn allowed_super(
        &mut self,
        def: DefId,
        ty: &Ty,
        span: Span,
        before: &[(Ty, Span)],
    ) -> Option<Ty> {
        let name = self.def_name(def);
        let Ty::Named(sup, _) = ty else {
            if !ty.has_error() {
                let shown = self.show(ty);
                self.report(
                    Diagnostic::new(codes::E0445, format!("`{shown}` is not an interface"))
                        .primary(span, "a super-interface must be an interface"),
                );
            }
            return None;
        };
        if self.def_kind(*sup) != Some(DefKind::Interface) {
            let shown = self.show(ty);
            self.report(
                Diagnostic::new(codes::E0445, format!("`{shown}` is not an interface"))
                    .primary(span, "a super-interface must be an interface"),
            );
            return None;
        }
        let shown = self.def_name(*sup);
        if [self.b.equatable, self.b.copyable, self.b.hashable].contains(&Some(*sup)) {
            self.report(
                Diagnostic::new(
                    codes::E0445,
                    format!("`{shown}` cannot be a super-interface"),
                )
                .primary(span, "a built-in interface that no block implements")
                .note("`Equatable` and `Copyable` are derived from the fields of a type, and `Hashable` is a fixed set of types")
                .help(format!("write it as a bound where it is needed: `<T: {name} + {shown}>`")),
            );
            return None;
        }
        if *sup == def {
            self.report(
                Diagnostic::new(codes::E0445, format!("`{name}` is its own super-interface"))
                    .primary(span, "an interface cannot need itself"),
            );
            return None;
        }
        if let Some((_, first)) = before
            .iter()
            .find(|(t, _)| matches!(t, Ty::Named(d, _) if d == sup))
        {
            self.report(
                Diagnostic::new(
                    codes::E0445,
                    format!("`{shown}` is named twice as a super-interface of `{name}`"),
                )
                .primary(span, "named again here")
                .secondary(*first, "first named here"),
            );
            return None;
        }
        Some(ty.clone())
    }

    /// An interface that is its own super-interface through others (E0445); the edge that
    /// closes the cycle is dropped, so that the closure of super-interfaces ends.
    fn super_cycles(&mut self) {
        let defs: Vec<DefId> = self.interface_supers.keys().copied().collect();
        let mut reported = BTreeSet::new();
        for def in defs {
            let Some(path) = self.cycle_from(def) else {
                continue;
            };
            let names: Vec<String> = path.iter().map(|d| self.def_name(*d)).collect();
            if names.len() > 1 && reported.insert(path.iter().copied().collect::<BTreeSet<_>>()) {
                let span = self
                    .interface_supers
                    .get(&def)
                    .and_then(|list| {
                        list.iter()
                            .find(|(t, _)| matches!(t, Ty::Named(d, _) if Some(d) == path.get(1)))
                    })
                    .map(|(_, s)| *s);
                if let Some(span) = span {
                    let name = self.def_name(def);
                    self.report(
                        Diagnostic::new(
                            codes::E0445,
                            format!("`{name}` is its own super-interface"),
                        )
                        .primary(span, "this super-interface leads back to it")
                        .note(format!("the chain is {}", names.join(" -> "))),
                    );
                }
            }
            // Drop the edge into the cycle from this interface.
            if let (Some(list), Some(next)) = (self.interface_supers.get_mut(&def), path.get(1)) {
                list.retain(|(t, _)| !matches!(t, Ty::Named(d, _) if d == next));
            }
        }
    }

    /// A path of super-interfaces from `start` back to `start`, if there is one.
    fn cycle_from(&self, start: DefId) -> Option<Vec<DefId>> {
        let mut stack = vec![(start, vec![start])];
        let mut seen = BTreeSet::new();
        while let Some((def, path)) = stack.pop() {
            for (t, _) in self.interface_supers.get(&def).into_iter().flatten() {
                let Ty::Named(next, _) = t else { continue };
                if *next == start {
                    let mut p = path.clone();
                    p.push(start);
                    return Some(p);
                }
                if seen.insert(*next) {
                    let mut p = path.clone();
                    p.push(*next);
                    stack.push((*next, p));
                }
            }
        }
        None
    }

    /// A method of an interface may not have the name of a method of one of its
    /// super-interfaces (E0447).
    fn methods_of_supers(&mut self, def: DefId, decl: &ast::InterfaceDecl) {
        let params: Vec<Ty> = self
            .type_generics
            .get(&def)
            .map(|g| g.iter().map(|g| Ty::Param(ParamId::Decl(g.span))).collect())
            .unwrap_or_default();
        let supers = self.supers_closure(def, &params);
        let name = self.def_name(def);
        for method in &decl.methods {
            let mname = &method.decl.sig.name;
            let owner = supers.iter().find(|(s, _)| {
                self.interface_methods
                    .get(s)
                    .is_some_and(|m| m.contains_key(&mname.name))
            });
            if let Some((s, _)) = owner {
                let sname = self.def_name(*s);
                self.report(
                    Diagnostic::new(
                        codes::E0447,
                        format!("`{}` is a method of `{sname}` too", mname.name),
                    )
                    .primary(mname.span, format!("`{name}` cannot declare it again"))
                    .note(format!(
                        "a type that implements `{name}` implements `{sname}` too, and a type has at most one method with a name"
                    ))
                    .help(format!("rename the method, or leave it to `{sname}`")),
                );
            }
        }
    }

    /// The super-interfaces of an interface with the type arguments `args`, and theirs in turn,
    /// each once, with their type arguments.
    pub(crate) fn supers_closure(&self, iface: DefId, args: &[Ty]) -> Vec<(DefId, Vec<Ty>)> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([iface]);
        let mut stack = vec![(iface, args.to_vec())];
        while let Some((def, args)) = stack.pop() {
            let map = self.generics_map(def, &args);
            for (t, _) in self.interface_supers.get(&def).into_iter().flatten() {
                let Ty::Named(sup, sargs) = t else { continue };
                if seen.insert(*sup) {
                    let sargs: Vec<Ty> = sargs.iter().map(|a| a.subst(&map)).collect();
                    out.push((*sup, sargs.clone()));
                    stack.push((*sup, sargs));
                }
            }
        }
        out
    }

    /// Whether `sup` is `iface` or one of its super-interfaces.
    pub(crate) fn is_super_or_self(&self, iface: DefId, sup: DefId) -> bool {
        iface == sup
            || self
                .supers_closure(iface, &[])
                .iter()
                .any(|(d, _)| *d == sup)
    }

    /// The map from the type parameters of a declaration to `args`.
    fn generics_map(&self, def: DefId, args: &[Ty]) -> BTreeMap<ParamId, Ty> {
        self.type_generics
            .get(&def)
            .map(|g| {
                g.iter()
                    .zip(args)
                    .map(|(g, a)| (ParamId::Decl(g.span), a.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A method of an interface with the type arguments `args`, or of one of its
    /// super-interfaces: the interface that declares it, and its signature with the type
    /// parameters of that interface replaced (`Self` is left as it is).
    pub(crate) fn iface_method(
        &self,
        iface: DefId,
        args: &[Ty],
        name: &str,
    ) -> Option<(DefId, BTreeMap<ParamId, Ty>, Sig)> {
        let mut candidates = vec![(iface, args.to_vec())];
        candidates.extend(self.supers_closure(iface, args));
        candidates.into_iter().find_map(|(def, args)| {
            let sig = self.interface_methods.get(&def)?.get(name)?.clone();
            Some((def, self.generics_map(def, &args), sig))
        })
    }

    /// Every implementation of an interface with super-interfaces needs an implementation of
    /// each of them by the same type, under the bounds of its head (chapter 4, section 8.3,
    /// rule 7; E0446).
    pub(crate) fn check_super_impls(&mut self) {
        for (owner, iface, span) in std::mem::take(&mut self.super_checks) {
            let Some(imp) = self
                .impls
                .get(&owner)
                .and_then(|list| list.iter().find(|i| i.interface == iface))
                .cloned()
            else {
                continue;
            };
            let map = self.generics_map(iface, &imp.args);
            let direct: Vec<Ty> = self
                .interface_supers
                .get(&iface)
                .map(|list| list.iter().map(|(t, _)| t.subst(&map)).collect())
                .unwrap_or_default();
            for sup in direct {
                let Ty::Named(sdef, sargs) = &sup else {
                    continue;
                };
                if let Err(needs) = self.implemented_under(owner, *sdef, sargs, &imp) {
                    let tname = self.def_name(owner);
                    let iname = self.def_name(iface);
                    let shown = self.show(&sup);
                    let d = Diagnostic::new(
                        codes::E0446,
                        format!(
                            "`{tname}` implements `{iname}` but not its super-interface `{shown}`"
                        ),
                    )
                    .primary(span, format!("`{iname}` needs `{shown}`"));
                    let d = match needs {
                        Some(bound) => d.note(format!(
                            "the implementation of `{shown}` needs `{bound}`, which the head of this block does not give"
                        ))
                        .help(format!("add the bound to the head of this block: `{bound}`")),
                        None => d.help(format!(
                            "add a block `{tname}: {shown} {{ ... }}` in the module of `{tname}`"
                        )),
                    };
                    self.report(d);
                }
            }
        }
    }

    /// Whether `owner` implements `sup` with the arguments `sargs` (in terms of the parameters
    /// of the block `imp`) for every type argument that the head of `imp` allows. `Err(Some)`
    /// names a bound of the implementation of `sup` that the head of `imp` lacks.
    fn implemented_under(
        &self,
        owner: DefId,
        sup: DefId,
        sargs: &[Ty],
        imp: &crate::env::ImplOf,
    ) -> Result<(), Option<String>> {
        let Some(other) = self
            .impls
            .get(&owner)
            .and_then(|list| list.iter().find(|i| i.interface == sup))
        else {
            return Err(None);
        };
        // The other block, in terms of the parameters of `imp`.
        let to_imp: BTreeMap<ParamId, Ty> = other
            .generics
            .iter()
            .zip(&imp.generics)
            .map(|(o, i)| (ParamId::Decl(*o), Ty::Param(ParamId::Decl(*i))))
            .collect();
        let same = other
            .args
            .iter()
            .map(|a| a.subst(&to_imp))
            .zip(sargs)
            .all(|(a, b)| a == *b);
        if !same {
            return Err(None);
        }
        for (i, needed) in other.bounds.iter().enumerate() {
            let given = imp.bounds.get(i).cloned().unwrap_or_default();
            for bound in needed {
                let Ty::Named(b, _) = bound else { continue };
                let holds = given
                    .iter()
                    .any(|g| matches!(g, Ty::Named(d, _) if self.is_super_or_self(*d, *b)));
                if !holds {
                    let param = imp
                        .generics
                        .get(i)
                        .and_then(|s| self.param_names.get(s))
                        .cloned()
                        .unwrap_or_else(|| "T".to_string());
                    return Err(Some(format!("{param}: {}", self.def_name(*b))));
                }
            }
        }
        Ok(())
    }
}
