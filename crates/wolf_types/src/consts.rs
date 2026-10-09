//! `config` constants (chapter 2, section 3.3, and chapter 4, section 12): their types, constant
//! expressions, cycles, evaluation with the rules of chapter 4, and the overrides of
//! `wolfpack.toml`.

use std::collections::BTreeSet;

use wolf_ast::{self as ast, BinaryOp, ExprKind, TyKind, UnaryOp};
use wolf_diag::{Diagnostic, codes};
use wolf_project::ConfigValue;
use wolf_resolve::{DefId, DefKind, Res, Role};
use wolf_span::{FileId, Span};

use crate::ConstValue;
use crate::env::Checker;
use crate::ty::Ty;

/// A constant initializer waiting for evaluation.
pub(crate) struct ConstInit<'a> {
    pub(crate) file: FileId,
    pub(crate) expr: &'a ast::Expr,
    pub(crate) name_span: Span,
}

/// How far the evaluation of a constant is.
pub(crate) enum ConstState {
    InProgress,
    Done(Option<ConstValue>),
}

impl<'a> Checker<'a> {
    pub(crate) fn config_body(&mut self, module: usize, def: DefId, entry: &'a ast::ConfigEntry) {
        let ty = self.consts.get(&def).cloned().unwrap_or(Ty::Error);
        let allowed = matches!(ty, Ty::Error)
            || [self.b.int, self.b.float, self.b.bool, self.b.string]
                .iter()
                .any(|d| self.is(&ty, *d));
        if !allowed {
            let shown = self.show(&ty);
            self.report(
                Diagnostic::new(codes::E0427, format!("a constant cannot have the type `{shown}`"))
                    .primary(entry.ty.span, "not a type for constants")
                    .note("constants are `Int`, `Float`, `Bool` or `String`, the types that `[config]` in `wolfpack.toml` can override"),
            );
            self.consts.insert(def, Ty::Error);
            return;
        }
        let file = self.file_of(module);
        if let Some(bad) = self.non_constant(file, &entry.value) {
            self.report(
                Diagnostic::new(codes::E0426, "a constant must be computed by the compiler")
                    .primary(bad.span, "this is not allowed in a constant")
                    .note("a constant may use literals, other constants and operators, but no function calls"),
            );
            self.const_failed.insert(def);
            return;
        }
        let mut b = self.new_body(module, None, Ty::Error);
        self.expr(&mut b, &entry.value, Some(&ty));
        self.finish_body(b);
        self.const_inits.insert(
            def,
            ConstInit {
                file,
                expr: &entry.value,
                name_span: entry.name.span,
            },
        );
        self.const_written.insert(def, (entry.ty.clone(), file));
    }

    /// The first part of an initializer that is not a constant expression.
    fn non_constant<'e>(&self, file: FileId, e: &'e ast::Expr) -> Option<&'e ast::Expr> {
        match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Error => None,
            ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => {
                self.non_constant(file, inner)
            }
            ExprKind::Binary { lhs, rhs, .. } => self
                .non_constant(file, lhs)
                .or_else(|| self.non_constant(file, rhs)),
            ExprKind::Name(_) | ExprKind::Field { .. } => {
                match self.resolved.res.get(&(file, e.id)).copied() {
                    Some(Res::Def(def)) if self.def_kind(def) == Some(DefKind::Config) => None,
                    None => None,
                    _ => Some(e),
                }
            }
            _ => Some(e),
        }
    }

    /// Finds cycles, applies the overrides and evaluates every constant.
    pub(crate) fn eval_consts(&mut self) {
        self.const_cycles();
        self.apply_overrides();
        let defs: Vec<DefId> = self.const_inits.keys().copied().collect();
        for def in defs {
            let _ = self.eval_init(def);
            if let Some(value) = self.const_value(def) {
                self.out.consts.insert(def, value);
            }
        }
    }

    fn deps(&self, def: DefId) -> Vec<DefId> {
        let mut out = Vec::new();
        if let Some(init) = self.const_inits.get(&def) {
            self.collect_deps(init.file, init.expr, &mut out);
        }
        out
    }

    fn collect_deps(&self, file: FileId, e: &ast::Expr, out: &mut Vec<DefId>) {
        match &e.kind {
            ExprKind::Paren(inner) | ExprKind::Unary { operand: inner, .. } => {
                self.collect_deps(file, inner, out);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.collect_deps(file, lhs, out);
                self.collect_deps(file, rhs, out);
            }
            ExprKind::Name(_) | ExprKind::Field { .. } => {
                if let Some(Res::Def(def)) = self.resolved.res.get(&(file, e.id)).copied() {
                    if self.def_kind(def) == Some(DefKind::Config) {
                        out.push(def);
                    }
                }
            }
            _ => {}
        }
    }

    /// Cycles between constants (E0419), each reported once.
    fn const_cycles(&mut self) {
        let defs: Vec<DefId> = self.const_inits.keys().copied().collect();
        let mut done: BTreeSet<DefId> = BTreeSet::new();
        for start in defs {
            let mut stack = Vec::new();
            self.cycle_dfs(start, &mut stack, &mut done);
        }
    }

    fn cycle_dfs(&mut self, def: DefId, stack: &mut Vec<DefId>, done: &mut BTreeSet<DefId>) {
        if done.contains(&def) {
            return;
        }
        if let Some(pos) = stack.iter().position(|d| *d == def) {
            let cycle: Vec<DefId> = stack.iter().skip(pos).copied().collect();
            let mut names: Vec<String> = cycle
                .iter()
                .map(|d| format!("`{}`", self.def_name(*d)))
                .collect();
            names.push(format!("`{}`", self.def_name(def)));
            let span = self.const_inits.get(&def).map(|i| i.name_span);
            let mut d = Diagnostic::new(
                codes::E0419,
                format!("cycle in constants: {}", names.join(" -> ")),
            )
            .note("a constant cannot depend on itself");
            if let Some(span) = span {
                d = d.primary(span, "this constant depends on itself");
            }
            self.report(d);
            for member in cycle {
                self.const_failed.insert(member);
                done.insert(member);
            }
            return;
        }
        stack.push(def);
        for dep in self.deps(def) {
            self.cycle_dfs(dep, stack, done);
        }
        stack.pop();
        done.insert(def);
    }

    /// Overrides from `[config.<module>]` replace the values of constants. Their types were
    /// checked by name resolution when the constant is written with a built-in type name; a
    /// constant written with an alias is checked here (E0613).
    fn apply_overrides(&mut self) {
        let overrides = self.project.config;
        for o in overrides {
            let path = o.module.join(".");
            let Some(module) =
                self.resolved.modules.iter().find(|m| {
                    m.path.join(".") == path && matches!(m.role, Role::Src | Role::Entry)
                })
            else {
                continue;
            };
            let Some(def) = module.items.get(&o.name).copied() else {
                continue;
            };
            if self.def_kind(def) != Some(DefKind::Config) {
                continue;
            }
            let ty = self.consts.get(&def).cloned().unwrap_or(Ty::Error);
            let written_builtin = self.const_written.get(&def).is_some_and(|(t, _)| {
                matches!(&t.kind, TyKind::Path { path, args }
                    if args.is_empty()
                        && path.segments.len() == 1
                        && path.segments.first().is_some_and(|s| matches!(s.name.as_str(), "Int" | "Float" | "Bool" | "String")))
            });
            let matches = match &o.value {
                ConfigValue::Int(_) => self.is(&ty, self.b.int),
                ConfigValue::Float(_) => self.is(&ty, self.b.float),
                ConfigValue::Bool(_) => self.is(&ty, self.b.bool),
                ConfigValue::Str(_) => self.is(&ty, self.b.string),
            };
            if matches {
                self.overrides
                    .insert(def, ConstValue::from(o.value.clone()));
            } else if !written_builtin && !matches!(ty, Ty::Error) {
                let declared = self.show(&ty);
                self.report(
                    Diagnostic::new(
                        codes::E0613,
                        format!(
                            "constant `{path}.{}` is `{declared}`, but the override is `{}`",
                            o.name,
                            o.value.type_name()
                        ),
                    )
                    .primary(
                        o.value_span,
                        format!("expected a value of type `{declared}`"),
                    ),
                );
            }
        }
    }

    /// The value a constant has in the program: the override, or its initializer.
    fn const_value(&mut self, def: DefId) -> Option<ConstValue> {
        if let Some(v) = self.overrides.get(&def) {
            return Some(v.clone());
        }
        self.eval_init(def)
    }

    fn eval_init(&mut self, def: DefId) -> Option<ConstValue> {
        if self.const_failed.contains(&def) {
            return None;
        }
        match self.const_state.get(&def) {
            Some(ConstState::Done(v)) => return v.clone(),
            Some(ConstState::InProgress) => return None,
            None => {}
        }
        let (file, expr) = {
            let init = self.const_inits.get(&def)?;
            (init.file, init.expr)
        };
        self.const_state.insert(def, ConstState::InProgress);
        let value = self.eval(file, expr);
        self.const_state
            .insert(def, ConstState::Done(value.clone()));
        value
    }

    fn eval(&mut self, file: FileId, e: &ast::Expr) -> Option<ConstValue> {
        match &e.kind {
            ExprKind::Int(v) => i64::try_from(*v).ok().map(ConstValue::Int),
            ExprKind::Float(v) => Some(ConstValue::Float(*v)),
            ExprKind::Str(s) => Some(ConstValue::Str(s.clone())),
            ExprKind::Bool(v) => Some(ConstValue::Bool(*v)),
            ExprKind::Paren(inner) => self.eval(file, inner),
            ExprKind::Name(_) | ExprKind::Field { .. } => {
                match self.resolved.res.get(&(file, e.id)).copied() {
                    Some(Res::Def(def)) => self.const_value(def),
                    _ => None,
                }
            }
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Neg {
                    if let ExprKind::Int(v) = operand.kind {
                        if v == i64::MAX as u64 + 1 {
                            return Some(ConstValue::Int(i64::MIN));
                        }
                    }
                }
                let v = self.eval(file, operand)?;
                match (op, v) {
                    (UnaryOp::Neg, ConstValue::Int(x)) => match x.checked_neg() {
                        Some(r) => Some(ConstValue::Int(r)),
                        None => self.const_panic(e.span, "integer overflow"),
                    },
                    (UnaryOp::Neg, ConstValue::Float(x)) => Some(ConstValue::Float(-x)),
                    (UnaryOp::Not, ConstValue::Bool(x)) => Some(ConstValue::Bool(!x)),
                    (UnaryOp::BitNot, ConstValue::Int(x)) => Some(ConstValue::Int(!x)),
                    _ => None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.eval(file, lhs)?;
                if let (BinaryOp::And, ConstValue::Bool(false)) = (op, &l) {
                    return Some(ConstValue::Bool(false));
                }
                if let (BinaryOp::Or, ConstValue::Bool(true)) = (op, &l) {
                    return Some(ConstValue::Bool(true));
                }
                let r = self.eval(file, rhs)?;
                self.binary_value(e.span, *op, l, r)
            }
            _ => None,
        }
    }

    fn const_panic(&mut self, span: Span, what: &str) -> Option<ConstValue> {
        self.report(
            Diagnostic::new(
                codes::E0423,
                format!("evaluating this constant panics: {what}"),
            )
            .primary(span, what.to_string())
            .note("constants are computed by the compiler, so a panic is a compile error"),
        );
        None
    }

    fn binary_value(
        &mut self,
        span: Span,
        op: BinaryOp,
        l: ConstValue,
        r: ConstValue,
    ) -> Option<ConstValue> {
        use ConstValue::{Bool, Float, Int, Str};
        let cmp = |o: std::cmp::Ordering| -> Option<bool> {
            Some(match op {
                BinaryOp::Eq => o.is_eq(),
                BinaryOp::NotEq => o.is_ne(),
                BinaryOp::Lt => o.is_lt(),
                BinaryOp::Le => o.is_le(),
                BinaryOp::Gt => o.is_gt(),
                BinaryOp::Ge => o.is_ge(),
                _ => return None,
            })
        };
        match (l, r) {
            (Int(a), Int(b)) => {
                let checked = match op {
                    BinaryOp::Add => a.checked_add(b),
                    BinaryOp::Sub => a.checked_sub(b),
                    BinaryOp::Mul => a.checked_mul(b),
                    BinaryOp::Div | BinaryOp::Rem => {
                        if b == 0 {
                            return self.const_panic(span, "division by zero");
                        }
                        if op == BinaryOp::Div {
                            a.checked_div(b)
                        } else {
                            a.checked_rem(b)
                        }
                    }
                    BinaryOp::BitAnd => Some(a & b),
                    BinaryOp::BitOr => Some(a | b),
                    BinaryOp::BitXor => Some(a ^ b),
                    BinaryOp::Shl | BinaryOp::Shr => {
                        let Ok(n) = u32::try_from(b) else {
                            return self.const_panic(span, "shift amount out of range");
                        };
                        if n > 63 {
                            return self.const_panic(span, "shift amount out of range");
                        }
                        // `<<` discards the bits shifted out; `>>` keeps the sign.
                        if op == BinaryOp::Shl {
                            Some(a.wrapping_shl(n))
                        } else {
                            Some(a >> n)
                        }
                    }
                    _ => return cmp(a.cmp(&b)).map(Bool),
                };
                match checked {
                    Some(v) => Some(Int(v)),
                    None => self.const_panic(span, "integer overflow"),
                }
            }
            (Float(a), Float(b)) => match op {
                BinaryOp::Add => Some(Float(a + b)),
                BinaryOp::Sub => Some(Float(a - b)),
                BinaryOp::Mul => Some(Float(a * b)),
                BinaryOp::Div => Some(Float(a / b)),
                BinaryOp::Rem => Some(Float(a % b)),
                BinaryOp::Eq => Some(Bool(a == b)),
                BinaryOp::NotEq => Some(Bool(a != b)),
                BinaryOp::Lt => Some(Bool(a < b)),
                BinaryOp::Le => Some(Bool(a <= b)),
                BinaryOp::Gt => Some(Bool(a > b)),
                BinaryOp::Ge => Some(Bool(a >= b)),
                _ => None,
            },
            (Str(a), Str(b)) => match op {
                BinaryOp::Add => Some(Str(a + &b)),
                // Byte order of UTF-8 is the order of Unicode scalar values (chapter 4, 3.4).
                _ => cmp(a.as_str().cmp(b.as_str())).map(Bool),
            },
            (Bool(a), Bool(b)) => match op {
                BinaryOp::And => Some(Bool(a && b)),
                BinaryOp::Or => Some(Bool(a || b)),
                _ => cmp(a.cmp(&b)).map(Bool),
            },
            _ => None,
        }
    }
}
