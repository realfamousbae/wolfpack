//! Types as the checker sees them, and the inference table.

use std::collections::BTreeMap;

use wolf_ast::Mode;
use wolf_resolve::DefId;
use wolf_span::Span;

/// A type parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParamId {
    /// A declared type parameter, identified by the span of its name in the declaration.
    Decl(Span),
    /// `Self` in an interface: the implementing type.
    SelfType,
}

/// An inference variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarId(pub u32);

/// A type.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    /// A nominal type with its type arguments: a struct, an enum, an interface, a built-in type
    /// such as `Int` or `List<T>`, or an extern type.
    Named(DefId, Vec<Ty>),
    /// `fn(A, mut B) -> R`.
    Fn(Vec<(Mode, Ty)>, Box<Ty>),
    /// A type parameter.
    Param(ParamId),
    /// A type that is not known yet.
    Var(VarId),
    /// The type of expressions that never produce a value.
    Never,
    /// The type of an expression that already has an error. It agrees with every type, so one
    /// error does not cause others.
    Error,
}

impl Ty {
    /// Whether the type contains [`Ty::Error`].
    pub fn has_error(&self) -> bool {
        match self {
            Ty::Error => true,
            Ty::Named(_, args) => args.iter().any(Ty::has_error),
            Ty::Fn(params, ret) => params.iter().any(|(_, t)| t.has_error()) || ret.has_error(),
            Ty::Param(_) | Ty::Var(_) | Ty::Never => false,
        }
    }

    /// Replaces type parameters by the types in `map`.
    pub fn subst(&self, map: &BTreeMap<ParamId, Ty>) -> Ty {
        if map.is_empty() {
            return self.clone();
        }
        match self {
            Ty::Param(p) => map.get(p).cloned().unwrap_or(Ty::Param(*p)),
            Ty::Named(def, args) => Ty::Named(*def, args.iter().map(|a| a.subst(map)).collect()),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|(m, t)| (*m, t.subst(map))).collect(),
                Box::new(ret.subst(map)),
            ),
            Ty::Var(_) | Ty::Never | Ty::Error => self.clone(),
        }
    }
}

/// The inference table: what each variable stands for.
#[derive(Default)]
pub(crate) struct Infer {
    vars: Vec<Option<Ty>>,
}

impl Infer {
    pub(crate) fn fresh(&mut self) -> Ty {
        let id = VarId(u32::try_from(self.vars.len()).unwrap_or(u32::MAX));
        self.vars.push(None);
        Ty::Var(id)
    }

    /// Follows bound variables at the top of the type.
    pub(crate) fn shallow(&self, ty: &Ty) -> Ty {
        let mut current = ty.clone();
        // Chains are short; the bound protects against a broken table.
        for _ in 0..self.vars.len() + 1 {
            match &current {
                Ty::Var(v) => match self.vars.get(v.0 as usize) {
                    Some(Some(bound)) => current = bound.clone(),
                    _ => return current,
                },
                _ => return current,
            }
        }
        current
    }

    /// Replaces every bound variable in the type.
    pub(crate) fn resolve(&self, ty: &Ty) -> Ty {
        match self.shallow(ty) {
            Ty::Named(def, args) => Ty::Named(def, args.iter().map(|a| self.resolve(a)).collect()),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|(m, t)| (*m, self.resolve(t))).collect(),
                Box::new(self.resolve(&ret)),
            ),
            other => other,
        }
    }

    /// Whether the type still contains unbound variables.
    pub(crate) fn is_unknown(&self, ty: &Ty) -> bool {
        match self.shallow(ty) {
            Ty::Var(_) => true,
            Ty::Named(_, args) => args.iter().any(|a| self.is_unknown(a)),
            Ty::Fn(params, ret) => {
                params.iter().any(|(_, t)| self.is_unknown(t)) || self.is_unknown(&ret)
            }
            _ => false,
        }
    }

    /// Binds every unbound variable in the type to [`Ty::Error`].
    pub(crate) fn poison(&mut self, ty: &Ty) {
        match self.shallow(ty) {
            Ty::Var(v) => self.bind(v, Ty::Error),
            Ty::Named(_, args) => args.iter().for_each(|a| self.poison(a)),
            Ty::Fn(params, ret) => {
                params.iter().for_each(|(_, t)| self.poison(t));
                self.poison(&ret);
            }
            _ => {}
        }
    }

    fn bind(&mut self, var: VarId, ty: Ty) {
        if let Some(slot) = self.vars.get_mut(var.0 as usize) {
            *slot = Some(ty);
        }
    }

    fn occurs(&self, var: VarId, ty: &Ty) -> bool {
        match self.shallow(ty) {
            Ty::Var(v) => v == var,
            Ty::Named(_, args) => args.iter().any(|a| self.occurs(var, a)),
            Ty::Fn(params, ret) => {
                params.iter().any(|(_, t)| self.occurs(var, t)) || self.occurs(var, &ret)
            }
            _ => false,
        }
    }

    /// Makes each pair of types equal, like [`Infer::unify`], but binds nothing when one pair
    /// cannot be equal.
    pub(crate) fn try_unify_all(&mut self, pairs: &[(Ty, Ty)]) -> bool {
        let saved = self.vars.clone();
        let mut ok = true;
        for (a, b) in pairs {
            ok &= self.unify(a, b);
        }
        if !ok {
            self.vars = saved;
        }
        ok
    }

    /// Makes two types equal, binding variables. Returns `false` if they cannot be equal.
    pub(crate) fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (&a, &b) {
            (Ty::Error, other) | (other, Ty::Error) => {
                self.poison(other);
                true
            }
            (Ty::Var(x), Ty::Var(y)) if x == y => true,
            (Ty::Var(v), other) | (other, Ty::Var(v)) => {
                if self.occurs(*v, other) {
                    return false;
                }
                self.bind(*v, other.clone());
                true
            }
            (Ty::Never, Ty::Never) => true,
            (Ty::Param(x), Ty::Param(y)) => x == y,
            (Ty::Named(d1, a1), Ty::Named(d2, a2)) => {
                if d1 != d2 || a1.len() != a2.len() {
                    return false;
                }
                let mut ok = true;
                for (x, y) in a1.iter().zip(a2) {
                    ok &= self.unify(x, y);
                }
                ok
            }
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) => {
                if p1.len() != p2.len() || p1.iter().zip(p2).any(|((m1, _), (m2, _))| m1 != m2) {
                    return false;
                }
                let mut ok = true;
                for ((_, x), (_, y)) in p1.iter().zip(p2) {
                    ok &= self.unify(x, y);
                }
                ok & self.unify(r1, r2)
            }
            _ => false,
        }
    }
}
