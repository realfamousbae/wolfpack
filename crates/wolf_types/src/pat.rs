//! Patterns and the exhaustiveness of `match` (chapter 4, section 11).
//!
//! Exhaustiveness uses the usefulness algorithm of Luc Maranget ("Warnings for pattern
//! matching", 2007). A pattern is *useful* after a list of rows if some value matches it and no
//! earlier row. An arm that is not useful is unreachable (E0411); the `match` is exhaustive if
//! the wildcard `_` is not useful after all unguarded arms, and otherwise the algorithm returns a
//! value that no arm covers, for the message of E0410.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{self as ast, PatKind};
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind, LocalId, Res};
use wolf_span::Span;

use crate::body::Body;
use crate::env::{Checker, Shape};
use crate::ty::{ParamId, Ty};

/// A pattern reduced to what exhaustiveness needs.
#[derive(Clone, Debug)]
enum P {
    Wild,
    Ctor(Ctor, Vec<P>),
    Or(Vec<P>),
}

/// A constructor of values.
#[derive(Clone, Debug, PartialEq)]
enum Ctor {
    Variant(DefId),
    Bool(bool),
    Int(i128),
    Str(String),
    /// A constant used as a pattern; its value is not known here.
    Const(DefId),
    /// The only constructor of a struct.
    Struct,
}

impl Checker<'_> {
    pub(crate) fn match_expr(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        scrutinee: &ast::Expr,
        arms: &[ast::Arm],
        expected: Option<&Ty>,
    ) -> Ty {
        let st = self.expr(b, scrutinee, None);
        let bool_ty = self.bool();
        let mut result: Option<Ty> = expected.cloned();
        let mut all_never = true;
        let mut pattern_errors = false;
        for arm in arms {
            let before = self.out.diagnostics.len();
            self.pattern(b, &arm.pattern, &st);
            pattern_errors |= self.out.diagnostics.len() > before;
            if let Some(guard) = &arm.guard {
                self.expr(b, guard, Some(&bool_ty));
            }
            let t = match result.clone() {
                Some(r) => self.value_or_discard(b, &arm.body, &r),
                None => self.expr(b, &arm.body, None),
            };
            if !self.is_never(&t) {
                all_never = false;
                if result.is_none() {
                    result = Some(t);
                }
            }
        }
        // A pattern with an error says nothing reliable about what it covers.
        if !pattern_errors {
            let shapes: Vec<(ast::Pat, bool)> = arms
                .iter()
                .map(|a| (a.pattern.clone(), a.guard.is_some()))
                .collect();
            if self.infer.is_unknown(&st) {
                // The type may be learned later in the body; check at its end.
                b.matches.push((scrutinee.span, st.clone(), shapes));
            } else {
                self.exhaustiveness(scrutinee.span, &st, &shapes);
            }
        }
        let _ = e;
        if all_never {
            Ty::Never
        } else {
            result.unwrap_or_else(|| self.void())
        }
    }

    // ----- checking patterns ------------------------------------------------------------------

    /// Checks a pattern against the type of the scrutinee and gives its bindings their types.
    pub(crate) fn pattern(&mut self, b: &mut Body, pat: &ast::Pat, ty: &Ty) {
        match &pat.kind {
            PatKind::Wild | PatKind::Error => {}
            PatKind::Name(_) => match self.res(b, pat.id) {
                Some(Res::Local(id)) => self.bind(b, id, ty, pat.span),
                Some(Res::Def(def)) => self.path_pattern(b, pat, def, ty),
                _ => {}
            },
            PatKind::Int { negative, value } => {
                if !negative && *value > i64::MAX as u64 {
                    self.report(
                        Diagnostic::new(
                            codes::E0423,
                            format!("integer literal `{value}` is out of range"),
                        )
                        .primary(pat.span, "the largest `Int` is 9223372036854775807"),
                    );
                }
                let int = self.int();
                self.pattern_type(pat.span, ty, &int);
            }
            PatKind::Str(_) => {
                let string = self.string();
                self.pattern_type(pat.span, ty, &string);
            }
            PatKind::Bool(_) => {
                let bool_ty = self.bool();
                self.pattern_type(pat.span, ty, &bool_ty);
            }
            PatKind::Path(_) => {
                if let Some(Res::Def(def)) = self.res(b, pat.id) {
                    self.path_pattern(b, pat, def, ty);
                }
            }
            PatKind::Tuple { path, items } => {
                let Some(Res::Def(def)) = self.res(b, pat.id) else {
                    for item in items {
                        self.pattern(b, item, &Ty::Error);
                    }
                    return;
                };
                let payload = self.variant_pattern_type(pat.span, def, ty);
                match payload {
                    Some((Shape::Tuple(types), map)) => {
                        if types.len() != items.len() {
                            let name = self.variant_name(def);
                            self.report(
                                Diagnostic::new(
                                    codes::E0428,
                                    format!(
                                        "`{name}` has {}, but the pattern has {}",
                                        crate::env::count(types.len(), "value"),
                                        items.len()
                                    ),
                                )
                                .primary(
                                    pat.span,
                                    format!(
                                        "expected {}",
                                        crate::env::count(types.len(), "pattern")
                                    ),
                                ),
                            );
                        }
                        for (i, item) in items.iter().enumerate() {
                            let t = types.get(i).map_or(Ty::Error, |t| t.subst(&map));
                            self.pattern(b, item, &t);
                        }
                    }
                    Some((shape, _)) => {
                        self.wrong_form(path.span, def, &shape);
                        for item in items {
                            self.pattern(b, item, &Ty::Error);
                        }
                    }
                    None => {
                        for item in items {
                            self.pattern(b, item, &Ty::Error);
                        }
                    }
                }
            }
            PatKind::Struct { path, fields, rest } => {
                self.struct_pattern(b, pat, path, fields, *rest, ty);
            }
            PatKind::Or(alternatives) => {
                let mut sets: Vec<BTreeSet<LocalId>> = Vec::new();
                for alt in alternatives {
                    self.pattern(b, alt, ty);
                    let mut set = BTreeSet::new();
                    self.bound_locals(b, alt, &mut set);
                    sets.push(set);
                }
                let all: BTreeSet<LocalId> = sets.iter().flatten().copied().collect();
                for (alt, set) in alternatives.iter().zip(&sets) {
                    if let Some(missing) = all.difference(set).next() {
                        let name = self
                            .resolved
                            .locals
                            .get(missing.0 as usize)
                            .map(|l| l.name.clone())
                            .unwrap_or_default();
                        self.report(
                            Diagnostic::new(
                                codes::E0412,
                                format!("`{name}` is not bound in every alternative"),
                            )
                            .primary(alt.span, format!("this alternative does not bind `{name}`"))
                            .note("every alternative of `|` must bind the same names"),
                        );
                        break;
                    }
                }
            }
        }
    }

    /// Gives a pattern binding its type. A name bound in several alternatives of `|` is one
    /// local; all its types must agree (E0412).
    fn bind(&mut self, b: &mut Body, id: LocalId, ty: &Ty, span: Span) {
        match b.locals.get(&id).cloned() {
            Some(existing) => {
                if !self.infer.unify(&existing, ty) {
                    let name = self
                        .resolved
                        .locals
                        .get(id.0 as usize)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    let first = self.show(&existing);
                    let second = self.show(ty);
                    self.report(
                        Diagnostic::new(
                            codes::E0412,
                            format!("`{name}` has type `{first}` in one alternative and `{second}` in another"),
                        )
                        .primary(span, format!("this `{name}` is `{second}`"))
                        .note("every alternative of `|` must bind each name with the same type"),
                    );
                }
            }
            None => {
                b.locals.insert(id, ty.clone());
            }
        }
    }

    fn bound_locals(&self, b: &Body, pat: &ast::Pat, out: &mut BTreeSet<LocalId>) {
        match &pat.kind {
            PatKind::Name(_) => {
                if let Some(Res::Local(id)) = self.res(b, pat.id) {
                    out.insert(id);
                }
            }
            PatKind::Tuple { items, .. } => {
                for item in items {
                    self.bound_locals(b, item, out);
                }
            }
            PatKind::Struct { fields, .. } => {
                for field in fields {
                    match &field.pat {
                        Some(sub) => self.bound_locals(b, sub, out),
                        None => {
                            if let Some(Res::Local(id)) = self.res(b, field.id) {
                                out.insert(id);
                            }
                        }
                    }
                }
            }
            PatKind::Or(alternatives) => {
                for alt in alternatives {
                    self.bound_locals(b, alt, out);
                }
            }
            _ => {}
        }
    }

    fn pattern_type(&mut self, span: Span, scrutinee: &Ty, pattern: &Ty) {
        if !self.infer.unify(scrutinee, pattern) {
            self.mismatch(span, scrutinee, pattern);
        }
    }

    /// A name or a path in a pattern: a payload-less variant or a constant.
    fn path_pattern(&mut self, b: &mut Body, pat: &ast::Pat, def: DefId, ty: &Ty) {
        let _ = b;
        match self.def_kind(def) {
            Some(DefKind::Config) => {
                let t = self.consts.get(&def).cloned().unwrap_or(Ty::Error);
                self.pattern_type(pat.span, ty, &t);
            }
            Some(DefKind::Variant { .. }) => {
                if let Some((shape, _)) = self.variant_pattern_type(pat.span, def, ty) {
                    if !matches!(shape, Shape::None) {
                        self.wrong_form(pat.span, def, &shape);
                    }
                }
            }
            Some(DefKind::Struct) => {
                let name = self.def_name(def);
                self.report(
                    Diagnostic::new(
                        codes::E0428,
                        format!("a pattern for the struct `{name}` needs braces"),
                    )
                    .primary(pat.span, "write the fields")
                    .help(format!("write `{name} {{ .. }}` to match any `{name}`")),
                );
            }
            _ => {}
        }
    }

    fn wrong_form(&mut self, span: Span, def: DefId, shape: &Shape) {
        let name = self.variant_name(def);
        let written = match shape {
            Shape::None => format!("`{name}`"),
            Shape::Tuple(_) => format!("`{name}(...)`"),
            Shape::Struct(_) => format!("`{name} {{ ... }}`"),
        };
        self.report(
            Diagnostic::new(
                codes::E0428,
                format!("variant `{name}` is written as {written}"),
            )
            .primary(span, "this pattern has a different form"),
        );
    }

    /// Unifies the enum of a variant with the scrutinee and returns the payload with the map of
    /// the enum's type parameters.
    fn variant_pattern_type(
        &mut self,
        span: Span,
        def: DefId,
        ty: &Ty,
    ) -> Option<(Shape, BTreeMap<ParamId, Ty>)> {
        let info = self.variants.get(&def)?.clone();
        let enum_ty = self.fresh_named(info.parent);
        self.pattern_type(span, ty, &enum_ty);
        let map = self.param_map(info.parent, &enum_ty);
        Some((info.shape, map))
    }

    /// The type parameters of `def` mapped to the arguments of `ty`.
    pub(crate) fn param_map(&self, def: DefId, ty: &Ty) -> BTreeMap<ParamId, Ty> {
        let args = match self.infer.shallow(ty) {
            Ty::Named(_, args) => args,
            _ => Vec::new(),
        };
        self.type_generics
            .get(&def)
            .map(|g| g.iter().map(|g| ParamId::Decl(g.span)).zip(args).collect())
            .unwrap_or_default()
    }

    fn struct_pattern(
        &mut self,
        b: &mut Body,
        pat: &ast::Pat,
        path: &ast::Path,
        fields: &[ast::FieldPat],
        rest: bool,
        ty: &Ty,
    ) {
        let check_subpatterns = |this: &mut Self, b: &mut Body| {
            for field in fields {
                if let Some(sub) = &field.pat {
                    this.pattern(b, sub, &Ty::Error);
                } else if let Some(Res::Local(id)) = this.res(b, field.id) {
                    b.locals.entry(id).or_insert(Ty::Error);
                }
            }
        };
        let Some(Res::Def(def)) = self.res(b, pat.id) else {
            check_subpatterns(self, b);
            return;
        };
        let (owner, infos, what, map) = match self.def_kind(def) {
            Some(DefKind::Struct) => {
                let t = self.fresh_named(def);
                self.pattern_type(pat.span, ty, &t);
                let map = self.param_map(def, &t);
                (
                    def,
                    self.fields.get(&def).cloned().unwrap_or_default(),
                    self.def_name(def),
                    map,
                )
            }
            Some(DefKind::Variant { parent, .. }) => match self
                .variant_pattern_type(pat.span, def, ty)
            {
                Some((Shape::Struct(infos), map)) => (parent, infos, self.variant_name(def), map),
                Some((shape, _)) => {
                    self.wrong_form(path.span, def, &shape);
                    check_subpatterns(self, b);
                    return;
                }
                None => {
                    check_subpatterns(self, b);
                    return;
                }
            },
            _ => {
                check_subpatterns(self, b);
                return;
            }
        };
        let foreign = self.resolved.def(owner).map(|d| d.module) != Some(b.module);
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for field in fields {
            if let Some(first) = seen.get(&field.name.name).copied() {
                self.report(
                    Diagnostic::new(
                        codes::E0422,
                        format!("field `{}` appears twice in the pattern", field.name.name),
                    )
                    .primary(field.name.span, "second mention")
                    .secondary(first, "first mention"),
                );
            }
            seen.insert(field.name.name.clone(), field.name.span);
            let info = infos.iter().find(|i| i.name == field.name.name).cloned();
            let field_ty = match &info {
                Some(info) => {
                    if foreign && !info.public {
                        self.report(
                            Diagnostic::new(
                                codes::E0607,
                                format!("field `{}` of `{what}` is private", info.name),
                            )
                            .primary(field.name.span, "private field"),
                        );
                    }
                    info.ty.subst(&map)
                }
                None => {
                    let mut d = Diagnostic::new(
                        codes::E0422,
                        format!("`{what}` has no field `{}`", field.name.name),
                    )
                    .primary(field.name.span, "unknown field");
                    if let Some(close) = wolf_resolve::suggest(
                        &field.name.name,
                        infos.iter().map(|i| i.name.as_str()),
                    ) {
                        d = d.help(format!("did you mean `{close}`?"));
                    }
                    self.report(d);
                    Ty::Error
                }
            };
            match &field.pat {
                Some(sub) => self.pattern(b, sub, &field_ty),
                None => {
                    if let Some(Res::Local(id)) = self.res(b, field.id) {
                        self.bind(b, id, &field_ty, field.name.span);
                    }
                }
            }
        }
        if !rest {
            let missing: Vec<String> = infos
                .iter()
                .filter(|i| !seen.contains_key(&i.name))
                .map(|i| format!("`{}`", i.name))
                .collect();
            if !missing.is_empty() {
                let word = if missing.len() == 1 {
                    "field"
                } else {
                    "fields"
                };
                self.report(
                    Diagnostic::new(
                        codes::E0422,
                        format!("the pattern does not mention {word} {}", missing.join(", ")),
                    )
                    .primary(path.span, format!("`{what}` has more fields"))
                    .help("add `..` to ignore the other fields"),
                );
            }
        }
    }

    /// Whether a checked pattern always matches: a name, `_`, or a struct pattern whose parts
    /// always match (chapter 4, section 6, rule 9b). A pattern with an error counts as matching.
    pub(crate) fn irrefutable(&self, b: &Body, pat: &ast::Pat) -> bool {
        match &pat.kind {
            PatKind::Wild | PatKind::Error => true,
            PatKind::Name(_) => !matches!(self.res(b, pat.id), Some(Res::Def(_))),
            PatKind::Struct { fields, .. } => {
                let is_struct = match self.res(b, pat.id) {
                    Some(Res::Def(def)) => matches!(self.def_kind(def), Some(DefKind::Struct)),
                    Some(Res::SelfType) => true,
                    None => true,
                    _ => false,
                };
                is_struct
                    && fields
                        .iter()
                        .all(|f| f.pat.as_ref().is_none_or(|p| self.irrefutable(b, p)))
            }
            _ => false,
        }
    }

    // ----- exhaustiveness ---------------------------------------------------------------------

    /// Exhaustiveness and unreachable arms; `arms` are the patterns and whether each arm has a
    /// guard.
    pub(crate) fn exhaustiveness(&mut self, span: Span, st: &Ty, arms: &[(ast::Pat, bool)]) {
        let st = self.infer.resolve(st);
        if st.has_error() || self.infer.is_unknown(&st) {
            return;
        }
        let file = span.file;
        let mut rows: Vec<Vec<P>> = Vec::new();
        let mut guarded = false;
        let tys = vec![st.clone()];
        for (pattern, has_guard) in arms {
            let p = self.reduce(file, pattern, &st);
            if self.useful(&rows, std::slice::from_ref(&p), &tys).is_none() {
                self.report(
                    Diagnostic::new(codes::E0411, "unreachable `match` arm")
                        .primary(pattern.span, "the arms above already cover this pattern")
                        .help("remove this arm, or move it before the arm that covers it"),
                );
            }
            if *has_guard {
                guarded = true;
            } else {
                rows.push(vec![p]);
            }
        }
        if let Some(witness) = self.useful(&rows, &[P::Wild], &tys) {
            let missing = witness
                .first()
                .map(|w| self.show_pat(w, &st))
                .unwrap_or_else(|| "_".to_string());
            let mut d = if missing == "_" {
                let shown = self.show(&st);
                Diagnostic::new(
                    codes::E0410,
                    format!("`match` does not cover every value of `{shown}`"),
                )
                .primary(span, "some values are not covered")
            } else {
                Diagnostic::new(
                    codes::E0410,
                    format!("`match` does not cover every value: `{missing}` is missing"),
                )
                .primary(span, format!("`{missing}` is not covered"))
            };
            d = if missing == "_" {
                d.help("add an arm `_ => ...` for the other values")
            } else {
                d.help(format!(
                    "add an arm for `{missing}`, or `_ => ...` for the rest"
                ))
            };
            if guarded {
                d = d.note("an arm with a guard does not count toward exhaustiveness");
            }
            self.report(d);
        }
    }

    /// The reduced form of a checked pattern.
    fn reduce(&self, file: wolf_span::FileId, pat: &ast::Pat, ty: &Ty) -> P {
        let res = self.resolved.res.get(&(file, pat.id)).copied();
        match &pat.kind {
            PatKind::Wild | PatKind::Error => P::Wild,
            PatKind::Name(_) | PatKind::Path(_) => match res {
                Some(Res::Def(def)) => match self.def_kind(def) {
                    Some(DefKind::Variant { .. }) => P::Ctor(Ctor::Variant(def), Vec::new()),
                    Some(DefKind::Config) => P::Ctor(Ctor::Const(def), Vec::new()),
                    _ => P::Wild,
                },
                _ => P::Wild,
            },
            PatKind::Int { negative, value } => {
                let v = i128::from(*value);
                P::Ctor(Ctor::Int(if *negative { -v } else { v }), Vec::new())
            }
            PatKind::Str(s) => P::Ctor(Ctor::Str(s.clone()), Vec::new()),
            PatKind::Bool(v) => P::Ctor(Ctor::Bool(*v), Vec::new()),
            PatKind::Tuple { items, .. } => {
                let Some(Res::Def(def)) = res else {
                    return P::Wild;
                };
                let ctor = Ctor::Variant(def);
                let tys = self.ctor_fields(&ctor, ty);
                if tys.len() != items.len() {
                    return P::Wild;
                }
                let args = items
                    .iter()
                    .zip(&tys)
                    .map(|(item, t)| self.reduce(file, item, t))
                    .collect();
                P::Ctor(ctor, args)
            }
            PatKind::Struct { fields, .. } => {
                let Some(Res::Def(def)) = res else {
                    return P::Wild;
                };
                let (ctor, infos) = match self.def_kind(def) {
                    Some(DefKind::Struct) => (
                        Ctor::Struct,
                        self.fields.get(&def).cloned().unwrap_or_default(),
                    ),
                    Some(DefKind::Variant { .. }) => {
                        match self.variants.get(&def).map(|v| &v.shape) {
                            Some(Shape::Struct(infos)) => (Ctor::Variant(def), infos.clone()),
                            _ => return P::Wild,
                        }
                    }
                    _ => return P::Wild,
                };
                let tys = self.ctor_fields(&ctor, ty);
                let args = infos
                    .iter()
                    .zip(&tys)
                    .map(|(info, t)| {
                        fields
                            .iter()
                            .find(|f| f.name.name == info.name)
                            .and_then(|f| f.pat.as_ref())
                            .map_or(P::Wild, |sub| self.reduce(file, sub, t))
                    })
                    .collect();
                P::Ctor(ctor, args)
            }
            PatKind::Or(alternatives) => P::Or(
                alternatives
                    .iter()
                    .map(|alt| self.reduce(file, alt, ty))
                    .collect(),
            ),
        }
    }

    /// The types of the values inside a constructor of `ty`.
    fn ctor_fields(&self, ctor: &Ctor, ty: &Ty) -> Vec<Ty> {
        let ty = self.infer.resolve(ty);
        match ctor {
            Ctor::Variant(def) => {
                let Some(info) = self.variants.get(def) else {
                    return Vec::new();
                };
                let map = self.param_map(info.parent, &ty);
                match &info.shape {
                    Shape::None => Vec::new(),
                    Shape::Tuple(types) => types.iter().map(|t| t.subst(&map)).collect(),
                    Shape::Struct(fields) => fields.iter().map(|f| f.ty.subst(&map)).collect(),
                }
            }
            Ctor::Struct => match &ty {
                Ty::Named(def, _) => {
                    let map = self.param_map(*def, &ty);
                    self.fields
                        .get(def)
                        .map(|fs| fs.iter().map(|f| f.ty.subst(&map)).collect())
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Every constructor of a type, or `None` if they cannot be listed (numbers, strings).
    fn all_ctors(&self, ty: &Ty) -> Option<Vec<Ctor>> {
        let Ty::Named(def, _) = self.infer.shallow(ty) else {
            return None;
        };
        if Some(def) == self.b.bool {
            return Some(vec![Ctor::Bool(true), Ctor::Bool(false)]);
        }
        if let Some(variants) = self.enum_variants.get(&def) {
            return Some(variants.iter().map(|v| Ctor::Variant(*v)).collect());
        }
        if self.fields.contains_key(&def) {
            return Some(vec![Ctor::Struct]);
        }
        None
    }

    /// A list of values for the columns of `q` that matches `q` and no row, if there is one.
    fn useful(&self, rows: &[Vec<P>], q: &[P], tys: &[Ty]) -> Option<Vec<P>> {
        let Some((head, rest)) = q.split_first() else {
            return if rows.is_empty() {
                Some(Vec::new())
            } else {
                None
            };
        };
        let ty0 = tys.first().cloned().unwrap_or(Ty::Error);
        let rest_tys: Vec<Ty> = tys.iter().skip(1).cloned().collect();
        match head {
            P::Or(alternatives) => {
                for alt in alternatives {
                    let mut q2 = vec![alt.clone()];
                    q2.extend(rest.iter().cloned());
                    if let Some(w) = self.useful(rows, &q2, tys) {
                        return Some(w);
                    }
                }
                None
            }
            P::Ctor(ctor, args) => {
                let field_tys = self.ctor_fields(ctor, &ty0);
                let arity = args.len();
                let specialized = specialize(rows, ctor, arity);
                let mut q2 = args.clone();
                q2.extend(rest.iter().cloned());
                let mut tys2 = field_tys;
                tys2.resize(arity, Ty::Error);
                tys2.extend(rest_tys);
                self.useful(&specialized, &q2, &tys2)
                    .map(|w| rebuild(ctor, arity, w))
            }
            P::Wild => {
                let mut used = Vec::new();
                for row in rows {
                    if let Some(first) = row.first() {
                        head_ctors(first, &mut used);
                    }
                }
                let all = self.all_ctors(&ty0);
                if let Some(all) = &all {
                    if !all.is_empty() && all.iter().all(|c| used.contains(c)) {
                        for ctor in all {
                            let field_tys = self.ctor_fields(ctor, &ty0);
                            let arity = field_tys.len();
                            let specialized = specialize(rows, ctor, arity);
                            let mut q2 = vec![P::Wild; arity];
                            q2.extend(rest.iter().cloned());
                            let mut tys2 = field_tys;
                            tys2.extend(rest_tys.iter().cloned());
                            if let Some(w) = self.useful(&specialized, &q2, &tys2) {
                                return Some(rebuild(ctor, arity, w));
                            }
                        }
                        return None;
                    }
                }
                let defaults = default_rows(rows);
                let w = self.useful(&defaults, rest, &rest_tys)?;
                let head_w = match &all {
                    Some(all) => match all.iter().find(|c| !used.contains(c)) {
                        Some(ctor) => {
                            let arity = self.ctor_fields(ctor, &ty0).len();
                            P::Ctor(ctor.clone(), vec![P::Wild; arity])
                        }
                        None => P::Wild,
                    },
                    None => P::Wild,
                };
                let mut out = vec![head_w];
                out.extend(w);
                Some(out)
            }
        }
    }

    /// A witness as source text.
    fn show_pat(&self, p: &P, ty: &Ty) -> String {
        match p {
            P::Wild | P::Or(_) => "_".to_string(),
            P::Ctor(ctor, args) => {
                let tys = self.ctor_fields(ctor, ty);
                let shown: Vec<String> = args
                    .iter()
                    .zip(tys.iter().chain(std::iter::repeat(&Ty::Error)))
                    .map(|(a, t)| self.show_pat(a, t))
                    .collect();
                match ctor {
                    Ctor::Bool(v) => v.to_string(),
                    Ctor::Int(v) => v.to_string(),
                    Ctor::Str(s) => format!("{s:?}"),
                    Ctor::Const(def) => self.def_name(*def),
                    Ctor::Variant(def) => {
                        let name = self.variant_name(*def);
                        match self.variants.get(def).map(|v| &v.shape) {
                            Some(Shape::Tuple(_)) => format!("{name}({})", shown.join(", ")),
                            Some(Shape::Struct(fields)) => {
                                let names: Vec<String> =
                                    fields.iter().map(|f| f.name.clone()).collect();
                                show_fields(&name, &names, &shown)
                            }
                            _ => name,
                        }
                    }
                    Ctor::Struct => {
                        let Ty::Named(def, _) = self.infer.shallow(ty) else {
                            return "_".to_string();
                        };
                        let names: Vec<String> = self
                            .fields
                            .get(&def)
                            .map(|fs| fs.iter().map(|f| f.name.clone()).collect())
                            .unwrap_or_default();
                        show_fields(&self.def_name(def), &names, &shown)
                    }
                }
            }
        }
    }
}

fn show_fields(name: &str, names: &[String], shown: &[String]) -> String {
    let given: Vec<String> = names
        .iter()
        .zip(shown)
        .filter(|(_, s)| s.as_str() != "_")
        .map(|(n, s)| format!("{n}: {s}"))
        .collect();
    if given.is_empty() {
        format!("{name} {{ .. }}")
    } else {
        format!("{name} {{ {}, .. }}", given.join(", "))
    }
}

fn head_ctors(p: &P, out: &mut Vec<Ctor>) {
    match p {
        P::Ctor(ctor, _) => {
            if !out.contains(ctor) {
                out.push(ctor.clone());
            }
        }
        P::Or(alternatives) => {
            for alt in alternatives {
                head_ctors(alt, out);
            }
        }
        P::Wild => {}
    }
}

/// The rows that match `ctor` in the first column, with its fields in place of that column.
fn specialize(rows: &[Vec<P>], ctor: &Ctor, arity: usize) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    for row in rows {
        let Some((head, rest)) = row.split_first() else {
            continue;
        };
        match head {
            P::Ctor(c, args) => {
                if c == ctor {
                    let mut r: Vec<P> = args.clone();
                    r.resize(arity, P::Wild);
                    r.extend(rest.iter().cloned());
                    out.push(r);
                }
            }
            P::Wild => {
                let mut r = vec![P::Wild; arity];
                r.extend(rest.iter().cloned());
                out.push(r);
            }
            P::Or(alternatives) => {
                let expanded: Vec<Vec<P>> = alternatives
                    .iter()
                    .map(|alt| {
                        let mut r = vec![alt.clone()];
                        r.extend(rest.iter().cloned());
                        r
                    })
                    .collect();
                out.extend(specialize(&expanded, ctor, arity));
            }
        }
    }
    out
}

/// The rows whose first column matches anything, without that column.
fn default_rows(rows: &[Vec<P>]) -> Vec<Vec<P>> {
    let mut out = Vec::new();
    for row in rows {
        let Some((head, rest)) = row.split_first() else {
            continue;
        };
        match head {
            P::Wild => out.push(rest.to_vec()),
            P::Or(alternatives) => {
                let expanded: Vec<Vec<P>> = alternatives
                    .iter()
                    .map(|alt| {
                        let mut r = vec![alt.clone()];
                        r.extend(rest.iter().cloned());
                        r
                    })
                    .collect();
                out.extend(default_rows(&expanded));
            }
            P::Ctor(..) => {}
        }
    }
    out
}

/// Puts the first `arity` values of a witness back under `ctor`.
fn rebuild(ctor: &Ctor, arity: usize, w: Vec<P>) -> Vec<P> {
    let mut inner = w;
    let rest = inner.split_off(arity.min(inner.len()));
    let mut out = vec![P::Ctor(ctor.clone(), inner)];
    out.extend(rest);
    out
}
