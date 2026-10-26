//! Bodies: expressions, statements, calls and methods (chapter 4, sections 3 to 8 and 10).
//!
//! Checking is bidirectional: an expression is checked with the type its place expects when that
//! is known (an annotated `let`, an argument, a field), and the expected type flows into
//! literals such as `[]` and `None` and into closure parameters. Unknown types are inference
//! variables; checks that need a known type (operators, bounds) wait until the end of the body.

use std::collections::BTreeMap;

use wolf_ast::{self as ast, AssignOp, BinaryOp, ExprKind, ForIter, Mode, StmtKind, UnaryOp};
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::{DefId, DefKind, LocalId, ModuleId, Res};
use wolf_span::{FileId, Span};

use crate::MethodCall;
use crate::env::{BodyRef, Checker, Method, Scope, Shape, Sig, count, given};
use crate::ty::{ParamId, Ty};

/// Where the code being checked is.
pub(crate) struct Body {
    pub(crate) module: ModuleId,
    pub(crate) file: FileId,
    pub(crate) self_ty: Option<Ty>,
    /// The result types of the enclosing function and closures, innermost last.
    pub(crate) returns: Vec<Ty>,
    /// For the enclosing function and each closure: the loops around the current point,
    /// innermost last.
    pub(crate) loops: Vec<Vec<LoopFrame>>,
    pub(crate) locals: BTreeMap<LocalId, Ty>,
    /// Checks that wait until the types are known.
    pub(crate) deferred: Vec<Deferred>,
    /// Expressions whose types were recorded, with their spans.
    pub(crate) recorded: Vec<(ast::NodeId, Span)>,
    /// Fields and methods of values whose type was not known yet.
    pub(crate) members: Vec<Member>,
    /// `match` expressions whose scrutinee type was not known yet: the scrutinee span, its type,
    /// and the patterns with whether each arm has a guard.
    pub(crate) matches: Vec<PendingMatch>,
}

/// A loop around the current point (chapter 4, section 6, rules 9 and 9a).
pub(crate) struct LoopFrame {
    /// Its label.
    pub(crate) label: Option<String>,
    /// Whether it is a `loop`, which may end with `break value`.
    pub(crate) is_loop: bool,
    /// Whether it has a `break` of its own.
    pub(crate) has_break: bool,
    /// For a `loop`: the type of its `break` values, once one is seen or expected.
    pub(crate) value: Option<Ty>,
}

/// See [`Body::matches`].
pub(crate) type PendingMatch = (Span, Ty, Vec<(ast::Pat, bool)>);

/// The arguments (types and spans), the span and the node of a deferred method call.
type DeferredCall = (Vec<(Ty, Span, ast::NodeId)>, Span, ast::NodeId);

/// `x.f` or `x.m(...)` on a value whose type is learned later in the body.
pub(crate) struct Member {
    recv: Ty,
    name: ast::Ident,
    /// The receiver expression.
    span: Span,
    /// For a method call: the argument types with their spans, the call span and node.
    call: Option<DeferredCall>,
    /// The type of the whole expression.
    ty: Ty,
}

/// A check that runs at the end of a body.
pub(crate) enum Deferred {
    /// The type must implement the interface.
    Bound {
        ty: Ty,
        interface: DefId,
        span: Span,
        why: String,
    },
    /// An operator was applied to a type that was not known yet.
    Operator { op: Op, ty: Ty, span: Span },
    /// A format specification of an interpolation (chapter 7, section 4.4): a precision needs
    /// a `Float`, `0` a number.
    Format {
        ty: Ty,
        precision: bool,
        zero: bool,
        span: Span,
    },
}

/// Which operand types an operator accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    /// `+`: `Int`, `Float`, `String`.
    Add,
    /// `-`, `*`, `/`, `%` and unary `-`: `Int`, `Float`.
    Arith(&'static str),
    /// Bitwise operators and shifts: `Int`.
    Bits(&'static str),
    /// `<`, `<=`, `>`, `>=`: `Int`, `Float`, `String`.
    Order(&'static str),
}

/// A function found for a method call or a path.
struct Found {
    params: Vec<(Mode, Ty)>,
    ret: Ty,
    has_self: bool,
    public: bool,
    module: Option<ModuleId>,
    method: Option<MethodCall>,
    /// What the head of the method's `impl` block requires of the type arguments of the
    /// receiver: each argument with an interface it must implement (chapter 3, section 2,
    /// rule 6).
    bounds: Vec<(Ty, DefId)>,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Arith(s) | Op::Bits(s) | Op::Order(s) => s,
        }
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_bodies(&mut self, bodies: Vec<BodyRef<'a>>) {
        for body in bodies {
            match body {
                BodyRef::Fn {
                    module,
                    decl,
                    self_ty,
                    method,
                } => self.fn_body(module, decl, self_ty, method),
                BodyRef::Test { module, test } => {
                    let mut b = self.new_body(module, None, self.void());
                    let void = self.void();
                    let ty = self.block(&mut b, &test.body, Some(&void), None);
                    let _ = ty;
                    self.finish_body(b);
                }
                BodyRef::Config { module, def, entry } => self.config_body(module, def, entry),
                BodyRef::Default {
                    module,
                    interface,
                    decl,
                } => self.default_body(module, interface, decl),
            }
        }
    }

    pub(crate) fn new_body(&self, module: usize, self_ty: Option<Ty>, ret: Ty) -> Body {
        Body {
            module: ModuleId(u32::try_from(module).unwrap_or(u32::MAX)),
            file: self.file_of(module),
            self_ty,
            returns: vec![ret],
            loops: vec![Vec::new()],
            locals: BTreeMap::new(),
            deferred: Vec::new(),
            recorded: Vec::new(),
            members: Vec::new(),
            matches: Vec::new(),
        }
    }

    fn fn_body(
        &mut self,
        module: usize,
        decl: &ast::FnDecl,
        self_ty: Option<Ty>,
        method: Option<usize>,
    ) {
        let Some(body) = &decl.body else {
            return;
        };
        let sig = match method {
            Some(index) => self
                .methods
                .get(index)
                .map(|m| (m.sig.clone(), !m.impl_generics.is_empty())),
            None => self
                .def_at
                .get(&decl.sig.name.span)
                .and_then(|def| self.fns.get(def))
                .map(|s| (s.clone(), false)),
        };
        let Some((sig, _)) = sig else {
            return;
        };
        // The bounds of the method's `where` hold in its body only (chapter 4, section 8.6).
        let added: Vec<(Span, Vec<Ty>)> = method
            .and_then(|i| self.methods.get(i))
            .map(|m| {
                m.impl_generics
                    .iter()
                    .copied()
                    .zip(m.where_bounds.iter().cloned())
                    .filter(|(_, w)| !w.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let saved: Vec<(Span, Option<Vec<Ty>>)> = added
            .iter()
            .map(|(span, extra)| {
                let before = self.param_bounds.get(span).cloned();
                let mut all = before.clone().unwrap_or_default();
                all.extend(extra.iter().cloned());
                self.param_bounds.insert(*span, all);
                (*span, before)
            })
            .collect();
        self.fn_body_checked(module, decl, self_ty, &sig, body);
        for (span, before) in saved {
            match before {
                Some(b) => self.param_bounds.insert(span, b),
                None => self.param_bounds.remove(&span),
            };
        }
    }

    /// The default of an interface method, checked once over `Self` that implements the
    /// interface (chapter 4, section 8.2, rule 4).
    fn default_body(&mut self, module: usize, interface: DefId, decl: &ast::FnDecl) {
        let Some(body) = &decl.body else {
            return;
        };
        let Some(sig) = self
            .interface_methods
            .get(&interface)
            .and_then(|m| m.get(&decl.sig.name.name))
            .cloned()
        else {
            return;
        };
        let args: Vec<Ty> = self
            .type_generics
            .get(&interface)
            .map(|g| g.iter().map(|g| Ty::Param(ParamId::Decl(g.span))).collect())
            .unwrap_or_default();
        let outer = self.current_interface.replace((interface, args));
        self.fn_body_checked(module, decl, Some(Ty::Param(ParamId::SelfType)), &sig, body);
        self.current_interface = outer;
    }

    fn fn_body_checked(
        &mut self,
        module: usize,
        decl: &ast::FnDecl,
        self_ty: Option<Ty>,
        sig: &Sig,
        body: &ast::Block,
    ) {
        let mut b = self.new_body(module, self_ty, sig.ret.clone());
        let mut param_types = sig.params.iter();
        for param in &decl.sig.params {
            if let ast::ParamKind::Named { name, .. } = &param.kind {
                let ty = param_types
                    .next()
                    .map(|(_, t)| t.clone())
                    .unwrap_or(Ty::Error);
                if let Some(id) = self.resolved.local_decls.get(&name.span) {
                    b.locals.insert(*id, ty);
                }
            }
        }
        let ret = sig.ret.clone();
        self.block(&mut b, body, Some(&ret), Some(decl.sig.name.span));
        self.finish_body(b);
    }

    /// The end of a body: unknown types (E0420), then the deferred checks.
    pub(crate) fn finish_body(&mut self, mut b: Body) {
        self.resolve_members(&mut b);
        for (span, st, arms) in std::mem::take(&mut b.matches) {
            self.exhaustiveness(span, &st, &arms);
        }
        let mut reported = false;
        let locals: Vec<(LocalId, Ty)> = b.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        for (id, ty) in locals {
            if self.infer.is_unknown(&ty) {
                let local = self.resolved.locals.get(id.0 as usize);
                if let Some(local) = local {
                    let shown = self.show(&ty);
                    let mut d = Diagnostic::new(
                        codes::E0420,
                        format!("cannot infer the type of `{}`", local.name),
                    )
                    .primary(local.span, format!("the type is `{shown}` here"));
                    let written = if shown == "_" {
                        "Type".to_string()
                    } else {
                        shown.replace('_', "...")
                    };
                    d = d.help(format!(
                        "write the type after the name: `{}: {written}`",
                        local.name
                    ));
                    self.report(d);
                    reported = true;
                }
                self.infer.poison(&ty);
            }
        }
        let recorded = std::mem::take(&mut b.recorded);
        for (id, span) in &recorded {
            let key = (b.file, *id);
            let Some(ty) = self.out.expr_types.get(&key).cloned() else {
                continue;
            };
            if self.infer.is_unknown(&ty) {
                if !reported {
                    self.report(
                        Diagnostic::new(codes::E0420, "cannot infer the type of this expression")
                            .primary(*span, format!("the type is `{}` here", self.show(&ty)))
                            .help("give the variable it is stored in a type"),
                    );
                    reported = true;
                }
                self.infer.poison(&ty);
            }
        }
        self.check_pending_bounds();
        for check in std::mem::take(&mut b.deferred) {
            self.run_deferred(check);
        }
        for (id, _) in recorded {
            let key = (b.file, id);
            if let Some(ty) = self.out.expr_types.get(&key).cloned() {
                let resolved = self.infer.resolve(&ty);
                self.out.expr_types.insert(key, resolved);
            }
        }
        for (id, ty) in b.locals {
            let resolved = self.infer.resolve(&ty);
            self.out.local_types.insert(id, resolved);
        }
    }

    /// Resolves fields and methods whose receiver type became known, until nothing changes.
    fn resolve_members(&mut self, b: &mut Body) {
        loop {
            let pending = std::mem::take(&mut b.members);
            let before = pending.len();
            for m in pending {
                if matches!(self.infer.shallow(&m.recv), Ty::Var(_)) {
                    b.members.push(m);
                } else {
                    self.resolve_member(b, m);
                }
            }
            if b.members.is_empty() || b.members.len() == before {
                break;
            }
        }
        for m in std::mem::take(&mut b.members) {
            let what = if m.call.is_some() {
                format!("`.{}()`", m.name.name)
            } else {
                format!("`.{}`", m.name.name)
            };
            self.unknown_receiver(m.span, &what);
            self.infer.poison(&m.recv);
            self.infer.poison(&m.ty);
        }
    }

    fn resolve_member(&mut self, b: &mut Body, m: Member) {
        let recv = self.infer.shallow(&m.recv);
        let Some((arg_tys, call_span, call_id)) = m.call else {
            let ty = self.field_of(b, &recv, &m.name);
            let _ = self.infer.unify(&ty, &m.ty);
            return;
        };
        if matches!(recv, Ty::Error | Ty::Never) {
            self.infer.poison(&m.ty);
            return;
        }
        let Some((found, sig)) = self.lookup_method(&recv, &m.name.name) else {
            if m.name.name == "copy" && arg_tys.is_empty() {
                if let Some(copyable) = self.b.copyable {
                    b.deferred.push(Deferred::Bound {
                        ty: recv.clone(),
                        interface: copyable,
                        span: m.name.span,
                        why: "`.copy()`".to_string(),
                    });
                }
                let _ = self.infer.unify(&recv, &m.ty);
                return;
            }
            let shown = self.show(&recv);
            let d = Diagnostic::new(
                codes::E0424,
                format!("`{shown}` has no method `{}`", m.name.name),
            )
            .primary(m.name.span, "unknown method");
            let d = self.note_other_instance(d, &recv, &m.name.name, "methods");
            self.report(d);
            self.infer.poison(&m.ty);
            return;
        };
        self.require_block_bounds(b, &found, &m.name);
        if !found.public && found.module.is_some_and(|md| md != b.module) {
            self.report(
                Diagnostic::new(
                    codes::E0607,
                    format!(
                        "method `{}` of `{}` is private",
                        m.name.name,
                        self.show(&recv)
                    ),
                )
                .primary(m.name.span, "private method"),
            );
        }
        let (params, ret) =
            self.instantiate(b, &sig, BTreeMap::new(), &m.name.name, m.name.span, None);
        if params.len() != arg_tys.len() {
            self.report(
                Diagnostic::new(
                    codes::E0428,
                    format!(
                        "`{}` takes {}, but {} given",
                        m.name.name,
                        count(params.len(), "argument"),
                        given(arg_tys.len())
                    ),
                )
                .primary(
                    call_span,
                    format!("expected {}", count(params.len(), "argument")),
                ),
            );
        }
        for ((arg, span, node), (_, param)) in arg_tys.iter().zip(&params) {
            self.coerce(b, Some(*node), *span, arg, param);
        }
        if let Some(call) = found.method {
            self.out.method_calls.insert((b.file, call_id), call);
        }
        if !self.infer.unify(&ret, &m.ty) {
            self.mismatch(call_span, &m.ty, &ret);
        }
    }

    fn run_deferred(&mut self, check: Deferred) {
        match check {
            Deferred::Bound {
                ty,
                interface,
                span,
                why,
            } => {
                let ty = self.infer.resolve(&ty);
                if ty.has_error() || self.infer.is_unknown(&ty) {
                    return;
                }
                self.require(&ty, interface, span, &why);
            }
            Deferred::Operator { op, ty, span } => {
                let ty = self.infer.resolve(&ty);
                if ty.has_error() || self.infer.is_unknown(&ty) {
                    return;
                }
                if !self.op_accepts(op, &ty) {
                    self.op_error(op, &ty, span);
                }
            }
            Deferred::Format {
                ty,
                precision,
                zero,
                span,
            } => {
                let ty = self.infer.resolve(&ty);
                if ty.has_error() || self.infer.is_unknown(&ty) {
                    return;
                }
                let shown = self.show(&ty);
                let float = self.is(&ty, self.b.float);
                let number = float || self.is(&ty, self.b.int);
                if precision && !float {
                    let help = if self.is(&ty, self.b.int) {
                        "convert the value first: `{n.to_float():.2}`"
                    } else {
                        "remove the precision"
                    };
                    self.report(
                        Diagnostic::new(codes::E0441, format!("`{shown}` takes no precision"))
                            .primary(span, "a precision is only for `Float`")
                            .help(help),
                    );
                } else if zero && !number {
                    self.report(
                        Diagnostic::new(
                            codes::E0441,
                            format!("`{shown}` cannot be padded with `0`"),
                        )
                        .primary(span, "`0` is only for `Int` and `Float`")
                        .help("pad with the fill character `0` instead: `0>5`"),
                    );
                }
            }
        }
    }

    /// Checks a format specification against the type of its value, when the type is known.
    fn format_spec(&mut self, b: &mut Body, ty: &Ty, spec: &ast::FormatSpec) {
        if spec.precision.is_some() || spec.zero {
            b.deferred.push(Deferred::Format {
                ty: ty.clone(),
                precision: spec.precision.is_some(),
                zero: spec.zero,
                span: spec.span,
            });
        }
    }

    // ----- helpers ----------------------------------------------------------------------------

    pub(crate) fn res(&self, b: &Body, id: ast::NodeId) -> Option<Res> {
        self.resolved.res.get(&(b.file, id)).copied()
    }

    pub(crate) fn scope(&self, b: &Body) -> Scope {
        Scope {
            file: b.file,
            self_ty: b.self_ty.clone(),
        }
    }

    pub(crate) fn is_void(&self, ty: &Ty) -> bool {
        self.is(ty, self.b.void)
    }

    pub(crate) fn is_never(&self, ty: &Ty) -> bool {
        matches!(self.infer.shallow(ty), Ty::Never)
    }

    fn is_error(&self, ty: &Ty) -> bool {
        matches!(self.infer.shallow(ty), Ty::Error)
    }

    fn record(&mut self, b: &mut Body, e: &ast::Expr, ty: &Ty) {
        self.out.expr_types.insert((b.file, e.id), ty.clone());
        b.recorded.push((e.id, e.span));
    }

    pub(crate) fn mismatch(&mut self, span: Span, expected: &Ty, found: &Ty) {
        let expected_s = self.show(expected);
        let found_s = self.show(found);
        let mut d = Diagnostic::new(
            codes::E0401,
            format!("expected `{expected_s}`, found `{found_s}`"),
        )
        .primary(span, format!("this is `{found_s}`"));
        let int_float = (self.is(expected, self.b.float) && self.is(found, self.b.int))
            || (self.is(expected, self.b.int) && self.is(found, self.b.float));
        if int_float {
            d = d.note("there are no implicit conversions between `Int` and `Float`; use `to_float()` or `to_int()`");
        }
        self.report(d);
    }

    /// Checks that a value of type `found` can be used where `expected` is expected, and returns
    /// the type of the value there. Interface coercion (chapter 4, section 8.5) happens here.
    pub(crate) fn coerce(
        &mut self,
        b: &mut Body,
        node: Option<ast::NodeId>,
        span: Span,
        found: &Ty,
        expected: &Ty,
    ) -> Ty {
        let f = self.infer.shallow(found);
        let x = self.infer.shallow(expected);
        if matches!(f, Ty::Never) {
            return Ty::Never;
        }
        if let (Ty::Named(iface, _), Ty::Named(def, _)) = (&x, &f) {
            let to_interface = self.def_kind(*iface) == Some(DefKind::Interface);
            let from_concrete = iface != def && self.def_kind(*def) != Some(DefKind::Interface);
            if to_interface && from_concrete {
                if self.implements(&f, *iface) && self.unify_impl_args(&f, &x) {
                    if let Some(node) = node {
                        self.out.coercions.insert((b.file, node), x.clone());
                    }
                } else {
                    let expected_s = self.show(&x);
                    let found_s = self.show(&f);
                    self.report(
                        Diagnostic::new(
                            codes::E0401,
                            format!("expected `{expected_s}`, found `{found_s}`"),
                        )
                        .primary(span, format!("this is `{found_s}`"))
                        .note(format!("`{found_s}` does not implement `{expected_s}`")),
                    );
                }
                return x;
            }
        }
        if !self.infer.unify(&f, &x) {
            self.mismatch(span, &x, &f);
        }
        x
    }

    /// Checks an expression against an expected type.
    pub(crate) fn expr(&mut self, b: &mut Body, e: &ast::Expr, expected: Option<&Ty>) -> Ty {
        let found = self.expr_found(b, e, expected);
        match expected {
            Some(x) => self.coerce(b, Some(e.id), e.span, &found, x),
            None => found,
        }
    }

    /// The type of an expression, using the expected type only as a hint.
    fn expr_found(&mut self, b: &mut Body, e: &ast::Expr, expected: Option<&Ty>) -> Ty {
        let ty = self.expr_inner(b, e, expected);
        self.record(b, e, &ty);
        ty
    }

    /// Checks an expression whose value may be discarded: when `expected` is `Void`, it is an
    /// expression statement (chapter 4, rule 6.4).
    pub(crate) fn value_or_discard(&mut self, b: &mut Body, e: &ast::Expr, expected: &Ty) -> Ty {
        if self.is_void(expected) {
            self.discard(b, e)
        } else {
            self.expr(b, e, Some(expected))
        }
    }

    /// The items of an `if let` or a `while let`: each `let` matches its pattern against its
    /// value, and the other items are `Bool` (chapter 4, rules 6.1 and 6.9b).
    fn cond_items(&mut self, b: &mut Body, items: &[ast::CondItem]) {
        for item in items {
            match item {
                ast::CondItem::Let { pattern, value, .. } => {
                    let t = self.expr(b, value, None);
                    self.pattern(b, pattern, &t);
                }
                ast::CondItem::Bool(e) => {
                    let bool_ty = self.bool();
                    self.expr(b, e, Some(&bool_ty));
                }
            }
        }
    }

    /// An expression statement (chapter 4, rules 6.7 and 6.8).
    pub(crate) fn discard(&mut self, b: &mut Body, e: &ast::Expr) -> Ty {
        let void = self.void();
        let ty = match &e.kind {
            ExprKind::If { .. }
            | ExprKind::IfLet { .. }
            | ExprKind::Match { .. }
            | ExprKind::Block(_) => self.expr(b, e, Some(&void)),
            _ => self.expr(b, e, None),
        };
        let allowed = match &e.kind {
            ExprKind::Call { .. }
            | ExprKind::If { .. }
            | ExprKind::IfLet { .. }
            | ExprKind::Loop { .. }
            | ExprKind::Match { .. }
            | ExprKind::Block(_)
            | ExprKind::Unit
            | ExprKind::Return(_)
            | ExprKind::Break { .. }
            | ExprKind::Continue { .. }
            | ExprKind::Error => true,
            ExprKind::Try(inner) => matches!(inner.kind, ExprKind::Call { .. }),
            _ => false,
        };
        if !allowed {
            let mut d = Diagnostic::new(codes::E0425, "the value of this expression is not used")
                .primary(e.span, "this computes a value that nobody uses");
            if matches!(
                e.kind,
                ExprKind::Unary {
                    op: UnaryOp::Neg,
                    ..
                }
            ) {
                d = d.help(
                    "if it continues the line above, put the operator at the end of that line",
                );
            } else {
                d = d.help("remove it, or store the value with `let`");
            }
            self.report(d);
        } else if self.is(&ty, self.b.result) {
            self.report(
                Diagnostic::new(codes::E0409, "this `Result` is not handled")
                    .primary(e.span, format!("this is `{}`", self.show(&ty)))
                    .help("handle the error with `match` or `?`, or discard it explicitly with `let _ = ...`"),
            );
        }
        if self.is_never(&ty) { Ty::Never } else { void }
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr_inner(&mut self, b: &mut Body, e: &ast::Expr, expected: Option<&Ty>) -> Ty {
        match &e.kind {
            ExprKind::Int(value) => {
                if *value > i64::MAX as u64 {
                    self.report(
                        Diagnostic::new(
                            codes::E0423,
                            format!("integer literal `{value}` is out of range"),
                        )
                        .primary(e.span, "the largest `Int` is 9223372036854775807")
                        .note("`9223372036854775808` can only be written after a minus sign"),
                    );
                }
                self.int()
            }
            ExprKind::Float(_) => self.float(),
            ExprKind::Str(_) => self.string(),
            ExprKind::FStr(parts) => {
                for part in parts {
                    if let ast::FStrPart::Expr(value, spec) = part {
                        let t = self.expr(b, value, None);
                        self.require_display(b, &t, value.span, "an interpolated value");
                        if let Some(spec) = spec {
                            self.format_spec(b, &t, spec);
                        }
                    }
                }
                self.string()
            }
            ExprKind::Bool(_) => self.bool(),
            ExprKind::Unit => self.void(),
            ExprKind::Name(_) => match self.res(b, e.id) {
                Some(res) => self.path_value(b, res, e.span, None, None),
                None => Ty::Error,
            },
            ExprKind::SelfValue => b.self_ty.clone().unwrap_or(Ty::Error),
            ExprKind::SelfType => {
                self.report(
                    Diagnostic::new(codes::E0429, "`Self` is a type, not a value")
                        .primary(e.span, "expected a value"),
                );
                Ty::Error
            }
            ExprKind::Paren(inner) => self.expr(b, inner, expected),
            ExprKind::List(items) => {
                let elem = expected
                    .and_then(|x| self.args_of(x, self.b.list))
                    .and_then(|a| a.first().cloned())
                    .unwrap_or_else(|| self.infer.fresh());
                for item in items {
                    self.expr(b, item, Some(&elem));
                }
                self.list_of(elem)
            }
            ExprKind::Map(entries) => {
                let (k, v) = match expected.and_then(|x| self.args_of(x, self.b.map)) {
                    Some(args) if args.len() == 2 => (
                        args.first().cloned().unwrap_or(Ty::Error),
                        args.get(1).cloned().unwrap_or(Ty::Error),
                    ),
                    _ => (self.infer.fresh(), self.infer.fresh()),
                };
                for (key, value) in entries {
                    self.expr(b, key, Some(&k));
                    self.expr(b, value, Some(&v));
                }
                // With an expected type, the written type was checked already.
                if let (Some(hashable), None) = (self.b.hashable, expected) {
                    b.deferred.push(Deferred::Bound {
                        ty: k.clone(),
                        interface: hashable,
                        span: e.span,
                        why: "a map key".to_string(),
                    });
                }
                self.b.map.map_or(Ty::Error, |m| Ty::Named(m, vec![k, v]))
            }
            ExprKind::StructLit { .. } => self.struct_lit(b, e, expected),
            ExprKind::Field { base, name } => match self.res(b, e.id) {
                Some(res) => self.path_value(b, res, name.span, None, Some(base)),
                None => self.field_access(b, base, name),
            },
            ExprKind::TypeArgs { base, args } => match self.res(b, base.id) {
                Some(res) => {
                    let head = match &base.kind {
                        ExprKind::Field { base: head, .. } => Some(head.as_ref()),
                        _ => None,
                    };
                    let ty = self.path_value(b, res, base.span, Some((args, e.span)), head);
                    self.record(b, base, &ty);
                    ty
                }
                None => Ty::Error,
            },
            ExprKind::Call { callee, args } => self.call(b, e, callee, args, expected),
            ExprKind::Index { base, index } => self.index(b, base, index),
            ExprKind::Slice {
                base, start, end, ..
            } => self.slice(b, e, base, start.as_deref(), end.as_deref()),
            ExprKind::Try(inner) => self.try_op(b, e, inner),
            ExprKind::Unary { op, operand } => self.unary(b, e, *op, operand),
            ExprKind::Binary { op, lhs, rhs } => self.binary(b, e, *op, lhs, rhs),
            ExprKind::Closure { params, ret, body } => {
                self.closure(b, params, ret.as_ref(), body, expected)
            }
            ExprKind::Block(block) => self.block(b, block, expected, None),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let bool_ty = self.bool();
                self.expr(b, cond, Some(&bool_ty));
                self.if_branches(b, then, otherwise.as_deref(), expected)
            }
            ExprKind::IfLet {
                items,
                then,
                otherwise,
            } => {
                self.cond_items(b, items);
                self.if_branches(b, then, otherwise.as_deref(), expected)
            }
            ExprKind::Loop { label, body } => {
                let value = expected
                    .filter(|x| !self.is_void(x) && !self.is_error(x))
                    .cloned();
                if let Some(loops) = b.loops.last_mut() {
                    loops.push(LoopFrame {
                        label: label.as_ref().map(|l| l.name.clone()),
                        is_loop: true,
                        has_break: false,
                        value,
                    });
                }
                let void = self.void();
                self.block(b, body, Some(&void), None);
                let frame = b.loops.last_mut().and_then(|l| l.pop());
                match frame {
                    // Without a `break` of its own a `loop` never ends (rule 6.9).
                    Some(f) if !f.has_break => Ty::Never,
                    Some(f) => f.value.unwrap_or_else(|| self.void()),
                    None => Ty::Error,
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => self.match_expr(b, e, scrutinee, arms, expected),
            ExprKind::Return(value) => {
                let ret = b.returns.last().cloned().unwrap_or(Ty::Error);
                match value {
                    Some(value) => {
                        self.expr(b, value, Some(&ret));
                    }
                    None => {
                        if !self.is_void(&ret) && !self.is_error(&ret) {
                            let void = self.void();
                            self.mismatch(e.span, &ret, &void);
                        }
                    }
                }
                Ty::Never
            }
            ExprKind::Break { label, value } => {
                self.break_expr(b, e, label.as_ref(), value.as_deref());
                Ty::Never
            }
            ExprKind::Continue { label } => {
                let in_closure = b.loops.len() > 1;
                let inside = b.loops.last().is_some_and(|l| !l.is_empty());
                if !inside {
                    self.outside_loop(e.span, "continue", in_closure);
                }
                let _ = label;
                Ty::Never
            }
            ExprKind::Error => Ty::Error,
        }
    }

    /// `if` and `if let` after the condition: the branches and the type.
    fn if_branches(
        &mut self,
        b: &mut Body,
        then: &ast::Block,
        otherwise: Option<&ast::Expr>,
        expected: Option<&Ty>,
    ) -> Ty {
        match otherwise {
            None => {
                let void = self.void();
                self.block(b, then, Some(&void), None);
                void
            }
            Some(other) => match expected {
                Some(x) => {
                    let t1 = self.block(b, then, Some(x), None);
                    let t2 = self.expr(b, other, Some(x));
                    if self.is_never(&t1) && self.is_never(&t2) {
                        Ty::Never
                    } else {
                        x.clone()
                    }
                }
                None => {
                    let t1 = self.block(b, then, None, None);
                    if self.is_never(&t1) {
                        self.expr(b, other, None)
                    } else {
                        let t2 = self.expr(b, other, Some(&t1));
                        if self.is_never(&t2) { t1 } else { t2 }
                    }
                }
            },
        }
    }

    /// Reports `break` or `continue` outside a loop (E0430).
    fn outside_loop(&mut self, span: Span, word: &str, in_closure: bool) {
        let mut d = Diagnostic::new(codes::E0430, format!("`{word}` outside a loop"))
            .primary(span, "not inside `while`, `for` or `loop`");
        if in_closure {
            d = d.note("the body of a closure is not part of the loop around it");
        }
        self.report(d);
    }

    /// `break`, `break label` and `break value` (chapter 4, section 6, rules 9a and 10).
    fn break_expr(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        label: Option<&ast::Ident>,
        value: Option<&ast::Expr>,
    ) {
        let in_closure = b.loops.len() > 1;
        let target = b.loops.last().and_then(|loops| match label {
            Some(l) => loops
                .iter()
                .rposition(|f| f.label.as_deref() == Some(l.name.as_str())),
            None => loops.len().checked_sub(1),
        });
        let Some(index) = target else {
            if label.is_none() {
                self.outside_loop(e.span, "break", in_closure);
            }
            // An unknown label was reported by name resolution.
            if let Some(v) = value {
                self.expr(b, v, None);
            }
            return;
        };
        let (is_loop, expected) = match b.loops.last_mut().and_then(|l| l.get_mut(index)) {
            Some(frame) => {
                frame.has_break = true;
                (frame.is_loop, frame.value.clone())
            }
            None => return,
        };
        if !is_loop {
            if let Some(v) = value {
                self.expr(b, v, None);
                // `break name` with an unknown name was reported as an unknown label.
                if matches!(v.kind, ExprKind::Name(_)) && self.res(b, v.id).is_none() {
                    return;
                }
                self.report(
                    Diagnostic::new(codes::E0435, "`break` with a value in a `while` or a `for`")
                        .primary(v.span, "only a `loop` ends with a value")
                        .help(
                            "use `loop { ... }`, or store the value in a variable before `break`",
                        ),
                );
            }
            return;
        }
        let found = match value {
            Some(v) => self.expr(b, v, expected.as_ref()),
            None => {
                let void = self.void();
                match &expected {
                    Some(x) => self.coerce(b, None, e.span, &void, x),
                    None => void,
                }
            }
        };
        if expected.is_none() && !self.is_never(&found) {
            if let Some(frame) = b.loops.last_mut().and_then(|l| l.get_mut(index)) {
                frame.value = Some(found);
            }
        }
    }

    /// A name that refers to a declaration, a local or a module, used as a value. `head` is the
    /// expression before the last dot, which may write the type's arguments: `List.<Int>.new`.
    fn path_value(
        &mut self,
        b: &mut Body,
        res: Res,
        span: Span,
        explicit: Option<(&[ast::Ty], Span)>,
        head: Option<&ast::Expr>,
    ) -> Ty {
        match res {
            Res::Local(id) => b.locals.get(&id).cloned().unwrap_or(Ty::Error),
            Res::Module(_) => {
                self.report(
                    Diagnostic::new(codes::E0429, "a module is not a value")
                        .primary(span, "this names a module")
                        .help("use one of its declarations: `module.name`"),
                );
                Ty::Error
            }
            Res::TypeParam(_) | Res::SelfType => {
                self.report(
                    Diagnostic::new(codes::E0429, "a type is not a value")
                        .primary(span, "this names a type"),
                );
                Ty::Error
            }
            Res::Def(def) => {
                let Some(d) = self.resolved.def(def).cloned() else {
                    return Ty::Error;
                };
                match d.kind {
                    DefKind::Fn | DefKind::ExternFn => {
                        let Some(sig) = self.fns.get(&def).cloned() else {
                            return Ty::Error;
                        };
                        let (params, ret) =
                            self.instantiate(b, &sig, BTreeMap::new(), &d.name, span, explicit);
                        Ty::Fn(params, Box::new(ret))
                    }
                    DefKind::Member { method, owner } => {
                        let Some(m) = self
                            .member_method
                            .get(&def)
                            .and_then(|i| self.methods.get(*i))
                            .cloned()
                        else {
                            return Ty::Error;
                        };
                        if method {
                            self.report(
                                Diagnostic::new(
                                    codes::E0424,
                                    format!(
                                        "`{}.{}` is a method, not an associated function",
                                        self.def_name(owner),
                                        d.name
                                    ),
                                )
                                .primary(span, "methods are called on a value")
                                .help(format!("call it on a value: `x.{}(...)`", d.name)),
                            );
                            return Ty::Error;
                        }
                        // The impl block's parameters are those of the type, in its order.
                        let written = self.written_args(b, head, owner).unwrap_or_default();
                        let args: Vec<Ty> = (0..m.impl_generics.len())
                            .map(|i| {
                                written
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| self.infer.fresh())
                            })
                            .collect();
                        // A function of a block for one instantiation (chapter 4, section 8.3,
                        // rule 8): `Stack.zero()` is a `Stack<Int>`.
                        if !self.instance_matches(&m, &args) {
                            let recv = Ty::Named(owner, args);
                            let shown = self.show(&recv);
                            let diag = Diagnostic::new(
                                codes::E0424,
                                format!("`{shown}` has no associated function `{}`", d.name),
                            )
                            .primary(span, "not found");
                            let diag = self.note_other_instance(diag, &recv, &d.name, "functions");
                            self.report(diag);
                            return Ty::Error;
                        }
                        let map: BTreeMap<ParamId, Ty> = m
                            .impl_generics
                            .iter()
                            .map(|s| ParamId::Decl(*s))
                            .zip(args)
                            .collect();
                        // `Map.new()`: the bounds of the impl block apply to the new arguments.
                        for (s, bounds) in m.impl_generics.iter().zip(&m.impl_bounds) {
                            for bound in bounds {
                                if let (Ty::Named(interface, _), Some(ty)) =
                                    (bound, map.get(&ParamId::Decl(*s)))
                                {
                                    let why = if Some(*interface) == self.b.hashable {
                                        "a map key".to_string()
                                    } else {
                                        format!("required by `{}`", d.name)
                                    };
                                    b.deferred.push(Deferred::Bound {
                                        ty: ty.clone(),
                                        interface: *interface,
                                        span,
                                        why,
                                    });
                                }
                            }
                        }
                        let (params, ret) =
                            self.instantiate(b, &m.sig, map, &d.name, span, explicit);
                        Ty::Fn(params, Box::new(ret))
                    }
                    DefKind::Config => self.consts.get(&def).cloned().unwrap_or(Ty::Error),
                    DefKind::Variant { parent, .. } => {
                        let shape = self.variants.get(&def).map(|v| v.shape.clone());
                        match shape {
                            Some(Shape::None) => match self.written_args(b, head, parent) {
                                Some(args) => Ty::Named(parent, args),
                                None => self.fresh_named(parent),
                            },
                            Some(shape) => {
                                let name = self.variant_name(def);
                                let help = match shape {
                                    Shape::Struct(_) => {
                                        format!("write it with its fields: `{name} {{ ... }}`")
                                    }
                                    _ => format!(
                                        "write it with its payload: `{name}(...)`; a variant is not a function value, so to pass it on write a closure such as `|x| {name}(x)`"
                                    ),
                                };
                                self.report(
                                    Diagnostic::new(
                                        codes::E0428,
                                        format!("variant `{name}` needs a payload"),
                                    )
                                    .primary(span, "the payload is missing")
                                    .help(help),
                                );
                                Ty::Error
                            }
                            None => Ty::Error,
                        }
                    }
                    kind => {
                        let what = match kind {
                            DefKind::Interface => "an interface",
                            _ => "a type",
                        };
                        let mut diag = Diagnostic::new(
                            codes::E0429,
                            format!("`{}` is {what}, not a value", d.name),
                        )
                        .primary(span, "expected a value");
                        if kind == DefKind::Enum {
                            diag = diag
                                .help(format!("write one of its variants: `{}.Variant`", d.name));
                        } else if kind == DefKind::Struct {
                            diag = diag.help(format!("build a value: `{} {{ ... }}`", d.name));
                        }
                        self.report(diag);
                        Ty::Error
                    }
                }
            }
        }
    }

    /// The type arguments written after the type in `Type.<Args>.name` (chapter 3, section 5,
    /// rule 2), when `head` is `Type.<Args>` and `Type` is `owner`. A wrong number of them is
    /// reported, and then they are not used.
    fn written_args(
        &mut self,
        b: &Body,
        head: Option<&ast::Expr>,
        owner: DefId,
    ) -> Option<Vec<Ty>> {
        let head = head?;
        let ExprKind::TypeArgs { base, args } = &head.kind else {
            return None;
        };
        if self.res(b, base.id) != Some(Res::Def(owner)) {
            return None;
        }
        let scope = self.scope(b);
        let lowered: Vec<Ty> = args.iter().map(|a| self.lower(a, &scope)).collect();
        let n = self.type_generics.get(&owner).map_or(0, Vec::len);
        if lowered.len() == n {
            return Some(lowered);
        }
        let name = self.def_name(owner);
        let d = if n == 0 {
            Diagnostic::new(
                codes::E0403,
                format!(
                    "`{name}` takes no type arguments, but {} given",
                    given(lowered.len())
                ),
            )
            .primary(head.span, "remove the type arguments")
        } else {
            Diagnostic::new(
                codes::E0403,
                format!(
                    "`{name}` takes {}, but {} given",
                    count(n, "type argument"),
                    given(lowered.len())
                ),
            )
            .primary(head.span, format!("expected {}", count(n, "type argument")))
        };
        self.report(d);
        None
    }

    /// A named type with fresh type arguments.
    pub(crate) fn fresh_named(&mut self, def: DefId) -> Ty {
        let n = self.type_generics.get(&def).map_or(0, Vec::len);
        let args = (0..n).map(|_| self.infer.fresh()).collect();
        Ty::Named(def, args)
    }

    /// `Shape.Rect`, or `Some` for the variants of the prelude enums.
    pub(crate) fn variant_name(&self, variant: DefId) -> String {
        let name = self.def_name(variant);
        match self.variants.get(&variant) {
            Some(info)
                if Some(info.parent) != self.b.option && Some(info.parent) != self.b.result =>
            {
                format!("{}.{name}", self.def_name(info.parent))
            }
            _ => name,
        }
    }

    /// The parameter and result types of a signature with fresh type arguments, recording the
    /// bounds of its type parameters.
    fn instantiate(
        &mut self,
        b: &mut Body,
        sig: &Sig,
        mut map: BTreeMap<ParamId, Ty>,
        name: &str,
        span: Span,
        explicit: Option<(&[ast::Ty], Span)>,
    ) -> (Vec<(Mode, Ty)>, Ty) {
        let mut given_args: Option<Vec<Ty>> = None;
        if let Some((args, args_span)) = explicit {
            let scope = self.scope(b);
            let lowered: Vec<Ty> = args.iter().map(|a| self.lower(a, &scope)).collect();
            if lowered.len() == sig.generics.len() {
                given_args = Some(lowered);
            } else {
                let d = if sig.generics.is_empty() {
                    Diagnostic::new(
                        codes::E0403,
                        format!(
                            "`{name}` takes no type arguments, but {} given",
                            given(lowered.len())
                        ),
                    )
                    .primary(args_span, "remove the type arguments")
                } else {
                    Diagnostic::new(
                        codes::E0403,
                        format!(
                            "`{name}` takes {}, but {} given",
                            count(sig.generics.len(), "type argument"),
                            given(lowered.len())
                        ),
                    )
                    .primary(
                        args_span,
                        format!("expected {}", count(sig.generics.len(), "type argument")),
                    )
                };
                self.report(d);
            }
        }
        let mut vars = Vec::new();
        for (i, g) in sig.generics.iter().enumerate() {
            let ty = given_args
                .as_ref()
                .and_then(|a| a.get(i).cloned())
                .unwrap_or_else(|| self.infer.fresh());
            map.insert(ParamId::Decl(g.span), ty.clone());
            vars.push(ty);
        }
        for (g, ty) in sig.generics.iter().zip(&vars) {
            for bound in &g.bounds {
                if let Ty::Named(interface, _) = bound {
                    b.deferred.push(Deferred::Bound {
                        ty: ty.clone(),
                        interface: *interface,
                        span,
                        why: format!("required by `{name}`"),
                    });
                }
            }
        }
        let params = sig
            .params
            .iter()
            .map(|(m, t)| (*m, t.subst(&map)))
            .collect();
        (params, sig.ret.subst(&map))
    }

    fn struct_lit(&mut self, b: &mut Body, e: &ast::Expr, expected: Option<&Ty>) -> Ty {
        let ExprKind::StructLit {
            path,
            type_args,
            fields,
            base,
        } = &e.kind
        else {
            return Ty::Error;
        };
        let base = base.as_deref();
        if let (Some(base), Some(Res::Def(def))) = (base, self.res(b, e.id)) {
            if matches!(self.def_kind(def), Some(DefKind::Variant { .. })) {
                self.report(
                    Diagnostic::new(
                        codes::E0434,
                        "the update syntax `..base` is for structs only",
                    )
                    .primary(base.span, "a variant cannot be built from another value")
                    .help("write every field of the variant"),
                );
            }
        }
        let res = self.res(b, e.id);
        let Some(Res::Def(mut def)) = res else {
            self.not_a_struct(res, path, false);
            for f in fields {
                if let Some(v) = &f.value {
                    self.expr(b, v, None);
                }
            }
            return Ty::Error;
        };
        // `type P = Point`: the literal builds the struct the alias stands for (section 7.3).
        let mut alias_args = None;
        if self.def_kind(def) == Some(DefKind::Alias) {
            let written = if type_args.is_empty() {
                None
            } else {
                let scope = self.scope(b);
                Some(type_args.iter().map(|a| self.lower(a, &scope)).collect())
            };
            match self.struct_of_alias(def, written) {
                Some(Ty::Named(target, args)) => {
                    self.out.alias_structs.insert((b.file, e.id), target);
                    def = target;
                    alias_args = Some(args);
                }
                _ => {
                    self.not_a_struct(res, path, false);
                    for f in fields {
                        if let Some(v) = &f.value {
                            self.expr(b, v, None);
                        }
                    }
                    return Ty::Error;
                }
            }
        }
        let (owner, infos, what) = match self.def_kind(def) {
            Some(DefKind::Struct) => (
                def,
                self.fields.get(&def).cloned().unwrap_or_default(),
                self.def_name(def),
            ),
            Some(DefKind::Variant { parent, .. }) => {
                match self.variants.get(&def).map(|v| v.shape.clone()) {
                    Some(Shape::Struct(infos)) => (parent, infos, self.variant_name(def)),
                    _ => {
                        let name = self.variant_name(def);
                        self.report(
                            Diagnostic::new(
                                codes::E0428,
                                format!("variant `{name}` is not written with fields"),
                            )
                            .primary(path.span, "this variant has no named fields")
                            .help(format!("write `{name}` or `{name}(...)` as declared")),
                        );
                        for f in fields {
                            if let Some(v) = &f.value {
                                self.expr(b, v, None);
                            }
                        }
                        return Ty::Error;
                    }
                }
            }
            _ => {
                self.not_a_struct(res, path, false);
                for f in fields {
                    if let Some(v) = &f.value {
                        self.expr(b, v, None);
                    }
                }
                return Ty::Error;
            }
        };
        let generics: Vec<Span> = self
            .type_generics
            .get(&owner)
            .map(|g| g.iter().map(|g| g.span).collect())
            .unwrap_or_default();
        let mut args: Vec<Ty> = alias_args.unwrap_or_default();
        if args.is_empty() && !type_args.is_empty() {
            let scope = self.scope(b);
            let lowered: Vec<Ty> = type_args.iter().map(|a| self.lower(a, &scope)).collect();
            if lowered.len() == generics.len() {
                args = lowered;
            } else {
                self.report(
                    Diagnostic::new(
                        codes::E0403,
                        format!(
                            "`{}` takes {}, but {} given",
                            self.def_name(owner),
                            count(generics.len(), "type argument"),
                            given(lowered.len())
                        ),
                    )
                    .primary(
                        path.span,
                        format!("expected {}", count(generics.len(), "type argument")),
                    ),
                );
            }
        }
        if args.len() != generics.len() {
            args = generics.iter().map(|_| self.infer.fresh()).collect();
        }
        let ty = Ty::Named(owner, args.clone());
        if let Some(x) = expected {
            let x = self.infer.shallow(x);
            if matches!(&x, Ty::Named(d, _) if *d == owner) {
                let _ = self.infer.unify(&ty, &x);
            }
        }
        let map: BTreeMap<ParamId, Ty> = generics
            .iter()
            .map(|s| ParamId::Decl(*s))
            .zip(args)
            .collect();
        let foreign = self.resolved.def(owner).map(|d| d.module) != Some(b.module);
        if foreign {
            if let Some(private) = infos.iter().find(|f| !f.public) {
                self.report(
                    Diagnostic::new(
                        codes::E0607,
                        format!(
                            "cannot build `{what}` here: field `{}` is private",
                            private.name
                        ),
                    )
                    .primary(
                        path.span,
                        "a struct with private fields is built only in its own module",
                    )
                    .help("use a function of its module that returns the value"),
                );
            }
        }
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for f in fields {
            if let Some(first) = seen.get(&f.name.name) {
                let first = *first;
                self.report(
                    Diagnostic::new(
                        codes::E0422,
                        format!("field `{}` is given twice", f.name.name),
                    )
                    .primary(f.name.span, "second value")
                    .secondary(first, "first value"),
                );
            }
            seen.insert(f.name.name.clone(), f.name.span);
            let info = infos.iter().find(|i| i.name == f.name.name).cloned();
            match info {
                Some(info) => {
                    let field_ty = info.ty.subst(&map);
                    match &f.value {
                        Some(v) => {
                            self.expr(b, v, Some(&field_ty));
                        }
                        None => {
                            let local_ty = match self.res(b, f.id) {
                                Some(res @ Res::Local(_)) | Some(res @ Res::Def(_)) => {
                                    self.path_value(b, res, f.name.span, None, None)
                                }
                                _ => Ty::Error,
                            };
                            self.coerce(b, Some(f.id), f.name.span, &local_ty, &field_ty);
                        }
                    }
                }
                None => {
                    let mut d = Diagnostic::new(
                        codes::E0422,
                        format!("`{what}` has no field `{}`", f.name.name),
                    )
                    .primary(f.name.span, "unknown field");
                    if let Some(close) =
                        wolf_resolve::suggest(&f.name.name, infos.iter().map(|i| i.name.as_str()))
                    {
                        d = d.help(format!("did you mean `{close}`?"));
                    }
                    self.report(d);
                    if let Some(v) = &f.value {
                        self.expr(b, v, None);
                    }
                }
            }
        }
        if let Some(base) = base {
            // The fields that are not listed come from the base (chapter 3, section 5, rule 6).
            self.expr(b, base, Some(&ty));
            return ty;
        }
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
                    format!("missing {word} {} in `{what}`", missing.join(", ")),
                )
                .primary(path.span, "every field must be given"),
            );
        }
        ty
    }

    fn field_access(&mut self, b: &mut Body, base: &ast::Expr, name: &ast::Ident) -> Ty {
        if self.is_path_head(b, base) {
            return Ty::Error;
        }
        let base_ty = self.expr(b, base, None);
        let t = self.infer.shallow(&base_ty);
        match &t {
            Ty::Error | Ty::Never => return Ty::Error,
            Ty::Var(_) => {
                // The type may be learned later in the body (chapter 4, rule 10.3).
                let ty = self.infer.fresh();
                b.members.push(Member {
                    recv: t.clone(),
                    name: name.clone(),
                    span: base.span,
                    call: None,
                    ty: ty.clone(),
                });
                return ty;
            }
            _ => {}
        }
        self.field_of(b, &t, name)
    }

    /// The type of the field `name` of a value of the known type `t`.
    fn field_of(&mut self, b: &mut Body, t: &Ty, name: &ast::Ident) -> Ty {
        let t = &self.infer.shallow(t);
        if let Ty::Named(def, args) = t {
            if let Some(infos) = self.fields.get(def).cloned() {
                if let Some(info) = infos.iter().find(|i| i.name == name.name) {
                    let module = self.resolved.def(*def).map(|d| d.module);
                    if !info.public && module != Some(b.module) {
                        self.report(
                            Diagnostic::new(
                                codes::E0607,
                                format!(
                                    "field `{}` of `{}` is private",
                                    name.name,
                                    self.def_name(*def)
                                ),
                            )
                            .primary(name.span, "private field"),
                        );
                    }
                    let map: BTreeMap<ParamId, Ty> = self
                        .type_generics
                        .get(def)
                        .map(|g| {
                            g.iter()
                                .map(|g| ParamId::Decl(g.span))
                                .zip(args.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    return info.ty.subst(&map);
                }
            }
        }
        let shown = self.show(t);
        if self.opaque(t) {
            self.report(
                Diagnostic::new(
                    codes::E0707,
                    format!("`{shown}` is an extern type and has no fields"),
                )
                .primary(name.span, "no field")
                .note(
                    "an extern type is used only through extern functions (chapter 7, section 8.3)",
                ),
            );
            return Ty::Error;
        }
        let mut d = Diagnostic::new(
            codes::E0424,
            format!("`{shown}` has no field `{}`", name.name),
        )
        .primary(name.span, "unknown field");
        if self.lookup_method(t, &name.name).is_some() {
            d = d.help(format!(
                "`{}` is a method; call it: `.{}()`",
                name.name, name.name
            ));
        } else if let Ty::Named(def, _) = &t {
            if let Some(infos) = self.fields.get(def) {
                if let Some(close) =
                    wolf_resolve::suggest(&name.name, infos.iter().map(|i| i.name.as_str()))
                {
                    d = d.help(format!("did you mean `{close}`?"));
                }
            }
        }
        self.report(d);
        Ty::Error
    }

    /// Whether an expression names a module or a type, so that `.name` after it is a path.
    fn is_path_head(&self, b: &Body, e: &ast::Expr) -> bool {
        let e = without_type_args(e);
        // Only names and dotted names are paths; a struct literal also has a resolution.
        if !matches!(
            e.kind,
            ExprKind::Name(_)
                | ExprKind::Field { .. }
                | ExprKind::TypeArgs { .. }
                | ExprKind::SelfType
        ) {
            return false;
        }
        match self.res(b, e.id) {
            Some(Res::Module(_)) | Some(Res::SelfType) => true,
            Some(Res::Def(def)) => self.def_kind(def).is_some_and(|k| k.is_type()),
            _ => matches!(e.kind, ExprKind::SelfType),
        }
    }

    fn unknown_receiver(&mut self, span: Span, what: &str) {
        self.report(
            Diagnostic::new(
                codes::E0420,
                format!("the type of this value must be known to use {what}"),
            )
            .primary(span, "type not known yet")
            .help("give the variable a type, for example `let names: List<String> = []`"),
        );
    }

    fn call(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        callee: &ast::Expr,
        args: &[ast::Arg],
        expected: Option<&Ty>,
    ) -> Ty {
        if let ExprKind::Field { base, name } = &callee.kind {
            if self.res(b, callee.id).is_none() {
                if let Some(Res::Def(owner)) = self.res(b, without_type_args(base).id) {
                    if matches!(
                        self.def_kind(owner),
                        Some(DefKind::Alias | DefKind::Interface | DefKind::ExternType)
                    ) {
                        return self.assoc_through(b, e, owner, base, name, args);
                    }
                }
                return self.method_call(b, e, callee, base, name, args);
            }
        }
        if let Some(Res::Def(def)) = self.res(b, callee.id) {
            if matches!(self.def_kind(def), Some(DefKind::Variant { .. })) {
                return self.variant_call(b, e, callee, def, args, expected);
            }
        }
        let callee_ty = self.expr(b, callee, None);
        let what = callee_name(callee);
        match self.infer.shallow(&callee_ty) {
            Ty::Fn(params, ret) => {
                self.args(b, e.span, &params, args, &what);
                *ret
            }
            Ty::Var(v) => {
                let params: Vec<(Mode, Ty)> =
                    args.iter().map(|a| (a.mode, self.infer.fresh())).collect();
                let ret = self.infer.fresh();
                let _ = self
                    .infer
                    .unify(&Ty::Var(v), &Ty::Fn(params.clone(), Box::new(ret.clone())));
                self.args(b, e.span, &params, args, &what);
                ret
            }
            Ty::Error | Ty::Never => {
                for a in args {
                    self.expr(b, &a.value, None);
                }
                Ty::Error
            }
            other => {
                let shown = self.show(&other);
                self.report(
                    Diagnostic::new(
                        codes::E0401,
                        format!("expected a function, found `{shown}`"),
                    )
                    .primary(
                        callee.span,
                        format!("this is `{shown}`, it cannot be called"),
                    ),
                );
                for a in args {
                    self.expr(b, &a.value, None);
                }
                Ty::Error
            }
        }
    }

    /// Checks call arguments against parameter types. Closures are checked last, so that the
    /// other arguments can fix the types of their parameters.
    fn args(
        &mut self,
        b: &mut Body,
        span: Span,
        params: &[(Mode, Ty)],
        args: &[ast::Arg],
        what: &str,
    ) {
        if params.len() != args.len() {
            self.report(
                Diagnostic::new(
                    codes::E0428,
                    format!(
                        "{what} takes {}, but {} given",
                        count(params.len(), "argument"),
                        given(args.len())
                    ),
                )
                .primary(
                    span,
                    format!("expected {}", count(params.len(), "argument")),
                ),
            );
        }
        let is_closure = |a: &ast::Arg| matches!(a.value.kind, ExprKind::Closure { .. });
        let order: Vec<usize> = (0..args.len())
            .filter(|i| args.get(*i).is_some_and(|a| !is_closure(a)))
            .chain((0..args.len()).filter(|i| args.get(*i).is_some_and(is_closure)))
            .collect();
        for i in order {
            let Some(arg) = args.get(i) else {
                continue;
            };
            let expected = params.get(i).map(|(_, t)| t.clone());
            self.expr(b, &arg.value, expected.as_ref());
        }
    }

    /// `Alias.f(...)`, and `Interface.f(...)` or `Extern.f(...)`, which name resolution leaves
    /// to the type checker. `head` is the expression before the dot: `Alias` or `Alias.<Args>`.
    fn assoc_through(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        owner: DefId,
        head: &ast::Expr,
        name: &ast::Ident,
        args: &[ast::Arg],
    ) -> Ty {
        let written = matches!(head.kind, ExprKind::TypeArgs { .. });
        if self.def_kind(owner) == Some(DefKind::Interface) {
            let is_method = self
                .interface_methods
                .get(&owner)
                .is_some_and(|m| m.contains_key(&name.name));
            if is_method {
                // `Holder.<Int>.first_or_same(x, 9)`: the written arguments of the interface.
                let iargs = if written {
                    let Some(type_args) = self.written_args(b, Some(head), owner) else {
                        for a in args {
                            self.expr(b, &a.value, None);
                        }
                        return Ty::Error;
                    };
                    Some(type_args)
                } else {
                    None
                };
                return self.default_call(b, e, owner, name, args, iargs);
            }
        }
        let target = if self.def_kind(owner) != Some(DefKind::Alias) {
            None
        } else if written {
            let Some(type_args) = self.written_args(b, Some(head), owner) else {
                for a in args {
                    self.expr(b, &a.value, None);
                }
                return Ty::Error;
            };
            Some(self.expand_alias(owner, type_args))
        } else {
            self.alias_type(owner)
        };
        if let Some(target @ Ty::Named(..)) = target {
            if let Some((found, sig)) = self.lookup_method(&target, &name.name) {
                if !found.has_self {
                    if !found.public && found.module.is_some_and(|m| m != b.module) {
                        self.report(
                            Diagnostic::new(
                                codes::E0607,
                                format!("`{}.{}` is private", self.show(&target), name.name),
                            )
                            .primary(name.span, "private associated function"),
                        );
                    }
                    let (params, ret) =
                        self.instantiate(b, &sig, BTreeMap::new(), &name.name, name.span, None);
                    self.args(b, e.span, &params, args, &format!("`{}`", name.name));
                    return ret;
                }
            }
        }
        let d = self
            .native_member(owner, &name.name, name.span)
            .unwrap_or_else(|| {
                Diagnostic::new(
                    codes::E0424,
                    format!(
                        "`{}` has no associated function `{}`",
                        self.def_name(owner),
                        name.name
                    ),
                )
                .primary(name.span, "not found")
            });
        self.report(d);
        for a in args {
            self.expr(b, &a.value, None);
        }
        Ty::Error
    }

    /// `Named.greeting(x)`: the default of an interface method, called with the receiver as
    /// the first argument (chapter 4, section 8.2, rule 5).
    fn default_call(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        interface: DefId,
        name: &ast::Ident,
        args: &[ast::Arg],
        written: Option<Vec<Ty>>,
    ) -> Ty {
        let Some(sig) = self
            .interface_methods
            .get(&interface)
            .and_then(|m| m.get(&name.name))
            .cloned()
        else {
            return Ty::Error;
        };
        let iname = self.def_name(interface);
        let has_default = self
            .interface_defaults
            .get(&interface)
            .is_some_and(|d| d.contains(&name.name));
        if !has_default {
            self.report(
                Diagnostic::new(
                    codes::E0444,
                    format!("`{iname}.{}` has no default", name.name),
                )
                .primary(
                    name.span,
                    "only a method with a default can be called this way",
                )
                .help(format!("call it on a value: `x.{}(...)`", name.name)),
            );
        }
        let recv = self.infer.fresh();
        let mut map: BTreeMap<ParamId, Ty> = BTreeMap::new();
        map.insert(ParamId::SelfType, recv.clone());
        let iargs: Vec<Ty> = self
            .type_generics
            .get(&interface)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|g| {
                let v = self.infer.fresh();
                map.insert(ParamId::Decl(g.span), v.clone());
                v
            })
            .collect();
        for (v, w) in iargs.iter().zip(written.iter().flatten()) {
            let _ = self.infer.unify(v, w);
        }
        let mut params = vec![(sig.self_mode.unwrap_or(Mode::Read), recv.clone())];
        params.extend(sig.params.iter().map(|(m, t)| (*m, t.subst(&map))));
        let ret = sig.ret.subst(&map);
        let what = format!("`{iname}.{}`", name.name);
        self.args(b, e.span, &params, args, &what);
        let span = args.first().map_or(e.span, |a| a.value.span);
        // The receiver must implement the interface; its implementation fixes the interface's
        // type arguments.
        let wanted = Ty::Named(interface, iargs);
        match self.infer.resolve(&recv) {
            Ty::Named(d, _) if d == interface => {
                let _ = self.infer.unify(&recv, &wanted);
            }
            r @ Ty::Named(..) => {
                let _ = self.unify_impl_args(&r, &wanted);
            }
            _ => {}
        }
        b.deferred.push(Deferred::Bound {
            ty: recv.clone(),
            interface,
            span,
            why: format!("required by {what}"),
        });
        if has_default {
            self.out
                .default_calls
                .insert((b.file, e.id), (interface, name.name.clone()));
        }
        ret
    }

    fn variant_call(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        callee: &ast::Expr,
        def: DefId,
        args: &[ast::Arg],
        expected: Option<&Ty>,
    ) -> Ty {
        let Some(info) = self.variants.get(&def).cloned() else {
            return Ty::Error;
        };
        let name = self.variant_name(def);
        let head = match &callee.kind {
            ExprKind::Field { base, .. } => Some(base.as_ref()),
            _ => None,
        };
        let ty = match self.written_args(b, head, info.parent) {
            Some(type_args) => Ty::Named(info.parent, type_args),
            None => self.fresh_named(info.parent),
        };
        if let Some(x) = expected {
            let x = self.infer.shallow(x);
            if matches!(&x, Ty::Named(d, _) if *d == info.parent) {
                let _ = self.infer.unify(&ty, &x);
            }
        }
        self.record(b, callee, &ty);
        match info.shape {
            Shape::Tuple(types) => {
                let map: BTreeMap<ParamId, Ty> = match &ty {
                    Ty::Named(_, a) => self
                        .type_generics
                        .get(&info.parent)
                        .map(|g| {
                            g.iter()
                                .map(|g| ParamId::Decl(g.span))
                                .zip(a.clone())
                                .collect()
                        })
                        .unwrap_or_default(),
                    _ => BTreeMap::new(),
                };
                let params: Vec<(Mode, Ty)> =
                    types.iter().map(|t| (Mode::Own, t.subst(&map))).collect();
                self.args(b, e.span, &params, args, &format!("`{name}`"));
                ty
            }
            Shape::None => {
                self.report(
                    Diagnostic::new(codes::E0428, format!("variant `{name}` has no payload"))
                        .primary(e.span, "this variant takes no values")
                        .help(format!("write `{name}` without parentheses")),
                );
                for a in args {
                    self.expr(b, &a.value, None);
                }
                ty
            }
            Shape::Struct(_) => {
                self.report(
                    Diagnostic::new(codes::E0428, format!("variant `{name}` has named fields"))
                        .primary(e.span, "this variant is written with its fields")
                        .help(format!("write `{name} {{ field: value, ... }}`")),
                );
                for a in args {
                    self.expr(b, &a.value, None);
                }
                ty
            }
        }
    }

    /// Finds a function of a type by name, with the type arguments of the receiver applied.
    fn lookup_method(&mut self, recv: &Ty, name: &str) -> Option<(Found, Sig)> {
        let recv_s = self.infer.shallow(recv);
        if let Ty::Param(ParamId::Decl(span)) = recv_s {
            return self.bound_method(span, &recv_s, name);
        }
        // `self` in a default: the methods of the interface, defaults included, and those of
        // its super-interfaces.
        if let Ty::Param(ParamId::SelfType) = recv_s {
            let (def, params) = self.current_interface.clone()?;
            let (owner, map, sig) = self.iface_method(def, &params, name)?;
            let params: Vec<(Mode, Ty)> = sig
                .params
                .iter()
                .map(|(m, t)| (*m, t.subst(&map)))
                .collect();
            let ret = sig.ret.subst(&map);
            let found = Found {
                params: params.clone(),
                ret: ret.clone(),
                has_self: sig.self_mode.is_some(),
                public: true,
                module: None,
                method: Some(MethodCall {
                    owner,
                    name: name.to_string(),
                    interface: Some(owner),
                }),
                bounds: Vec::new(),
            };
            let sig = Sig {
                generics: Vec::new(),
                self_mode: sig.self_mode,
                params,
                ret,
            };
            return Some((found, sig));
        }
        let Ty::Named(def, args) = recv_s else {
            return None;
        };
        if self.def_kind(def) == Some(DefKind::Interface) {
            let (owner, mut map, sig) = self.iface_method(def, &args, name)?;
            map.insert(ParamId::SelfType, Ty::Named(def, args.clone()));
            let found = Found {
                params: sig
                    .params
                    .iter()
                    .map(|(m, t)| (*m, t.subst(&map)))
                    .collect(),
                ret: sig.ret.subst(&map),
                has_self: sig.self_mode.is_some(),
                public: true,
                module: None,
                method: Some(MethodCall {
                    owner,
                    name: name.to_string(),
                    interface: Some(owner),
                }),
                bounds: Vec::new(),
            };
            let sig = Sig {
                generics: Vec::new(),
                self_mode: sig.self_mode,
                params: found.params.clone(),
                ret: found.ret.clone(),
            };
            return Some((found, sig));
        }
        let index = *self.method_table.get(&def)?.get(name)?;
        let m = self.methods.get(index)?.clone();
        // The methods of a block for one instantiation exist only for it (chapter 4,
        // section 8.3, rule 8).
        if !self.instance_matches(&m, &args) {
            return None;
        }
        // Only what the head adds: the bounds of the type's own declaration are checked where
        // the type is used.
        let own: Vec<Vec<DefId>> = self
            .type_generics
            .get(&def)
            .map(|g| {
                g.iter()
                    .map(|g| {
                        g.bounds
                            .iter()
                            .filter_map(|b| match b {
                                Ty::Named(i, _) => Some(*i),
                                _ => None,
                            })
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut bounds: Vec<(Ty, DefId)> = Vec::new();
        for (i, (head, arg)) in m.impl_bounds.iter().zip(&args).enumerate() {
            for bound in head {
                if let Ty::Named(interface, _) = bound {
                    if !own.get(i).is_some_and(|o| o.contains(interface)) {
                        bounds.push((arg.clone(), *interface));
                    }
                }
            }
            // And what the method's `where` adds (chapter 4, section 8.6).
            for bound in m.where_bounds.get(i).into_iter().flatten() {
                if let Ty::Named(interface, _) = bound {
                    bounds.push((arg.clone(), *interface));
                }
            }
        }
        // `Result` is `Display` also when its error only implements `Error` (chapter 4,
        // section 9, rule 7), which no head can say: the whole type is checked.
        if Some(def) == self.b.result && m.interface.is_some() && m.interface == self.b.display {
            if let Some(display) = self.b.display {
                bounds = vec![(Ty::Named(def, args.clone()), display)];
            }
        }
        let map: BTreeMap<ParamId, Ty> = m
            .impl_generics
            .iter()
            .map(|s| ParamId::Decl(*s))
            .zip(args)
            .collect();
        let found = Found {
            params: m
                .sig
                .params
                .iter()
                .map(|(mo, t)| (*mo, t.subst(&map)))
                .collect(),
            ret: m.sig.ret.subst(&map),
            has_self: m.sig.self_mode.is_some(),
            public: m.public,
            module: Some(m.module),
            method: Some(MethodCall {
                owner: m.owner,
                name: m.name.clone(),
                interface: m.interface,
            }),
            bounds,
        };
        // Method type parameters are substituted by the caller.
        let sig = Sig {
            generics: m.sig.generics.clone(),
            self_mode: m.sig.self_mode,
            params: found.params.clone(),
            ret: found.ret.clone(),
        };
        Some((found, sig))
    }

    /// Whether type arguments are those of the block of a method for one instantiation
    /// (chapter 4, section 8.3, rule 8); variables among them take the types of the block.
    fn instance_matches(&mut self, m: &Method, args: &[Ty]) -> bool {
        let pairs: Vec<(Ty, Ty)> = m
            .instance
            .iter()
            .zip(args)
            .filter_map(|(fixed, arg)| fixed.clone().map(|f| (arg.clone(), f)))
            .collect();
        pairs.is_empty() || self.infer.try_unify_all(&pairs)
    }

    /// For E0424: the head of the block for one instantiation that defines `name` for another
    /// instantiation of the type of `recv`, and the span of the block.
    pub(crate) fn other_instance(&self, recv: &Ty, name: &str) -> Option<(String, Span)> {
        let Ty::Named(def, _) = self.infer.resolve(recv) else {
            return None;
        };
        let index = *self.method_table.get(&def)?.get(name)?;
        let m = self.methods.get(index)?;
        if m.instance.is_empty() {
            return None;
        }
        let head: Vec<Ty> = m
            .instance
            .iter()
            .zip(&m.impl_generics)
            .map(|(t, s)| t.clone().unwrap_or(Ty::Param(ParamId::Decl(*s))))
            .collect();
        Some((self.show(&Ty::Named(def, head)), m.block))
    }

    /// E0424 with what names the block that defines the member for another instantiation.
    pub(crate) fn note_other_instance(
        &self,
        d: Diagnostic,
        recv: &Ty,
        name: &str,
        what: &str,
    ) -> Diagnostic {
        match self.other_instance(recv, name) {
            Some((head, span)) => d
                .secondary(span, format!("`{name}` is defined in the block `{head}`"))
                .note(format!(
                    "the {what} of the block `{head} {{ ... }}` exist only for `{head}`"
                )),
            None => d,
        }
    }

    /// A method of a type parameter, from the interfaces of its bounds (chapter 4, 8.4).
    fn bound_method(&mut self, span: Span, recv: &Ty, name: &str) -> Option<(Found, Sig)> {
        let bounds = self.param_bounds.get(&span).cloned().unwrap_or_default();
        for bound in bounds {
            let Ty::Named(bound_iface, args) = bound else {
                continue;
            };
            // The methods of the bound's super-interfaces too (chapter 4, section 8.2, rule 6).
            let Some((iface, mut map, sig)) = self.iface_method(bound_iface, &args, name) else {
                continue;
            };
            map.insert(ParamId::SelfType, recv.clone());
            let params: Vec<(Mode, Ty)> = sig
                .params
                .iter()
                .map(|(m, t)| (*m, t.subst(&map)))
                .collect();
            let ret = sig.ret.subst(&map);
            let found = Found {
                params: params.clone(),
                ret: ret.clone(),
                has_self: sig.self_mode.is_some(),
                public: true,
                module: None,
                method: Some(MethodCall {
                    owner: iface,
                    name: name.to_string(),
                    interface: Some(iface),
                }),
                bounds: Vec::new(),
            };
            let sig = Sig {
                generics: Vec::new(),
                self_mode: sig.self_mode,
                params,
                ret,
            };
            return Some((found, sig));
        }
        None
    }

    /// The bounds that the head of the `impl` block of a called method adds (chapter 3,
    /// section 2, rule 6), checked for the type arguments of the receiver (E0404, E0406).
    fn require_block_bounds(&mut self, b: &mut Body, found: &Found, name: &ast::Ident) {
        for (ty, interface) in &found.bounds {
            b.deferred.push(Deferred::Bound {
                ty: ty.clone(),
                interface: *interface,
                span: name.span,
                why: format!("required by `{}`", name.name),
            });
        }
    }

    fn method_call(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        callee: &ast::Expr,
        base: &ast::Expr,
        name: &ast::Ident,
        args: &[ast::Arg],
    ) -> Ty {
        let check_args = |this: &mut Self, b: &mut Body| {
            for a in args {
                this.expr(b, &a.value, None);
            }
        };
        if self.is_path_head(b, base) {
            // `module.name` or `Type.name` that name resolution could not resolve; it has
            // reported the error already.
            check_args(self, b);
            return Ty::Error;
        }
        let recv_ty = self.expr(b, base, None);
        let recv = self.infer.shallow(&recv_ty);
        match &recv {
            Ty::Error | Ty::Never => {
                check_args(self, b);
                return Ty::Error;
            }
            Ty::Var(_) => {
                // The type may be learned later in the body (chapter 4, rule 10.3).
                let arg_tys: Vec<(Ty, Span, ast::NodeId)> = args
                    .iter()
                    .map(|a| (self.expr(b, &a.value, None), a.value.span, a.value.id))
                    .collect();
                let ty = self.infer.fresh();
                b.members.push(Member {
                    recv: recv.clone(),
                    name: name.clone(),
                    span: base.span,
                    call: Some((arg_tys, e.span, e.id)),
                    ty: ty.clone(),
                });
                let _ = callee;
                return ty;
            }
            _ => {}
        }
        let found = self.lookup_method(&recv, &name.name);
        let Some((found, sig)) = found else {
            if name.name == "copy" && args.is_empty() {
                if let Some(copyable) = self.b.copyable {
                    b.deferred.push(Deferred::Bound {
                        ty: recv.clone(),
                        interface: copyable,
                        span: name.span,
                        why: "`.copy()`".to_string(),
                    });
                }
                self.record(b, callee, &Ty::Fn(Vec::new(), Box::new(recv.clone())));
                return recv;
            }
            let shown = self.show(&recv);
            let native = match self.infer.resolve(&recv) {
                Ty::Named(d, _) => self.native_member(d, &name.name, name.span),
                _ => None,
            };
            if let Some(d) = native {
                self.report(d);
                check_args(self, b);
                return Ty::Error;
            }
            if self.opaque(&recv) {
                self.report(
                    Diagnostic::new(
                        codes::E0707,
                        format!("`{shown}` is an extern type and has no methods"),
                    )
                    .primary(name.span, "no method")
                    .help("declare an extern function that takes the value as its first parameter"),
                );
                check_args(self, b);
                return Ty::Error;
            }
            let d = Diagnostic::new(
                codes::E0424,
                format!("`{shown}` has no method `{}`", name.name),
            )
            .primary(name.span, "unknown method");
            let mut d = self.note_other_instance(d, &recv, &name.name, "methods");
            let field_fn = match &recv {
                Ty::Named(def, _) => self
                    .fields
                    .get(def)
                    .is_some_and(|f| f.iter().any(|f| f.name == name.name)),
                _ => false,
            };
            if field_fn {
                d = d.help(format!(
                    "`{0}` is a field; to call a function stored in it, write `(x.{0})(...)`",
                    name.name
                ));
            } else if let Ty::Named(def, _) = &recv {
                let candidates: Vec<String> = self
                    .method_table
                    .get(def)
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();
                if let Some(close) =
                    wolf_resolve::suggest(&name.name, candidates.iter().map(String::as_str))
                {
                    d = d.help(format!("did you mean `{close}`?"));
                }
            }
            self.report(d);
            check_args(self, b);
            return Ty::Error;
        };
        self.require_block_bounds(b, &found, name);
        if !found.has_self {
            let owner = self.show(&recv);
            self.report(
                Diagnostic::new(
                    codes::E0424,
                    format!(
                        "`{}` is an associated function of `{owner}`, not a method",
                        name.name
                    ),
                )
                .primary(name.span, "it has no `self` parameter")
                .help(format!("call it on the type: `{owner}.{}(...)`", name.name)),
            );
        }
        if !found.public && found.module.is_some_and(|m| m != b.module) {
            self.report(
                Diagnostic::new(
                    codes::E0607,
                    format!(
                        "method `{}` of `{}` is private",
                        name.name,
                        self.show(&recv)
                    ),
                )
                .primary(name.span, "private method"),
            );
        }
        let (params, ret) = self.instantiate(b, &sig, BTreeMap::new(), &name.name, name.span, None);
        self.record(b, callee, &Ty::Fn(params.clone(), Box::new(ret.clone())));
        if let Some(call) = found.method {
            self.out.method_calls.insert((b.file, e.id), call);
        }
        self.args(b, e.span, &params, args, &format!("`{}`", name.name));
        ret
    }

    fn index(&mut self, b: &mut Body, base: &ast::Expr, index: &ast::Expr) -> Ty {
        let base_ty = self.expr(b, base, None);
        let t = self.infer.shallow(&base_ty);
        if let Some(args) = self.args_of(&t, self.b.list) {
            let int = self.int();
            self.expr(b, index, Some(&int));
            return args.first().cloned().unwrap_or(Ty::Error);
        }
        if let Some(args) = self.args_of(&t, self.b.map) {
            let key = args.first().cloned().unwrap_or(Ty::Error);
            self.expr(b, index, Some(&key));
            return args.get(1).cloned().unwrap_or(Ty::Error);
        }
        let int = self.int();
        self.expr(b, index, Some(&int));
        match &t {
            Ty::Error | Ty::Never => Ty::Error,
            Ty::Var(_) => {
                self.unknown_receiver(base.span, "`[...]`");
                self.infer.poison(&t);
                Ty::Error
            }
            _ if self.is(&t, self.b.string) => {
                self.report(
                    Diagnostic::new(codes::E0405, "a `String` cannot be indexed")
                        .primary(base.span, "this is `String`")
                        .help("use a slice, which is a `String`: `s[i..=i]`")
                        .note("there is no character type; positions count Unicode scalar values"),
                );
                Ty::Error
            }
            _ => {
                let shown = self.show(&t);
                self.report(
                    Diagnostic::new(codes::E0405, format!("`{shown}` cannot be indexed"))
                        .primary(base.span, format!("this is `{shown}`"))
                        .note("only `List` and `Map` support `[...]`"),
                );
                Ty::Error
            }
        }
    }

    fn slice(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        base: &ast::Expr,
        start: Option<&ast::Expr>,
        end: Option<&ast::Expr>,
    ) -> Ty {
        let base_ty = self.expr(b, base, None);
        let int = self.int();
        for bound in [start, end].into_iter().flatten() {
            self.expr(b, bound, Some(&int));
        }
        let t = self.infer.shallow(&base_ty);
        if let Some(args) = self.args_of(&t, self.b.list) {
            if let (Some(copyable), Some(item)) = (self.b.copyable, args.first()) {
                b.deferred.push(Deferred::Bound {
                    ty: item.clone(),
                    interface: copyable,
                    span: e.span,
                    why: "slicing a list copies the elements and".to_string(),
                });
            }
            return t;
        }
        if self.is(&t, self.b.string) {
            return t;
        }
        match &t {
            Ty::Error | Ty::Never => Ty::Error,
            Ty::Var(_) => {
                self.unknown_receiver(base.span, "a slice");
                self.infer.poison(&t);
                Ty::Error
            }
            _ => {
                let shown = self.show(&t);
                self.report(
                    Diagnostic::new(codes::E0405, format!("`{shown}` cannot be sliced"))
                        .primary(base.span, format!("this is `{shown}`"))
                        .note("only `List` and `String` support slices"),
                );
                Ty::Error
            }
        }
    }

    fn try_op(&mut self, b: &mut Body, e: &ast::Expr, inner: &ast::Expr) -> Ty {
        let inner_ty = self.expr(b, inner, None);
        let t = self.infer.shallow(&inner_ty);
        let ret = b.returns.last().cloned().unwrap_or(Ty::Error);
        let ret_s = self.infer.shallow(&ret);
        if let Some(args) = self.args_of(&t, self.b.result) {
            let ok = args.first().cloned().unwrap_or(Ty::Error);
            let err = args.get(1).cloned().unwrap_or(Ty::Error);
            match &ret_s {
                Ty::Error => {}
                Ty::Var(_) => {
                    let ok2 = self.infer.fresh();
                    let r = self
                        .b
                        .result
                        .map_or(Ty::Error, |d| Ty::Named(d, vec![ok2, err.clone()]));
                    let _ = self.infer.unify(&ret_s, &r);
                }
                _ => match self.args_of(&ret_s, self.b.result) {
                    Some(ret_args) => {
                        let target = ret_args.get(1).cloned().unwrap_or(Ty::Error);
                        let target_s = self.infer.shallow(&target);
                        let err_s = self.infer.shallow(&err);
                        let coerced = match (&target_s, &err_s) {
                            // A value of another interface is not converted (chapter 4,
                            // section 8.5, rule 1).
                            (Ty::Named(iface, _), Ty::Named(def, _))
                                if self.def_kind(*iface) == Some(DefKind::Interface)
                                    && self.def_kind(*def) != Some(DefKind::Interface)
                                    && iface != def =>
                            {
                                if self.implements(&err_s, *iface) {
                                    self.out
                                        .try_coercions
                                        .insert((b.file, e.id), target_s.clone());
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => self.infer.unify(&err_s, &target_s),
                        };
                        if !coerced {
                            let err_shown = self.show(&err_s);
                            let target_shown = self.show(&target_s);
                            self.report(
                                Diagnostic::new(
                                    codes::E0408,
                                    format!("`?` cannot return the error `{err_shown}` from a function whose error type is `{target_shown}`"),
                                )
                                .primary(e.span, format!("the error here is `{err_shown}`"))
                                .note("`?` passes the error on unchanged, or converts it into an interface it implements, such as `Error`"),
                            );
                        }
                    }
                    None => {
                        let shown = self.show(&ret_s);
                        self.report(
                            Diagnostic::new(
                                codes::E0408,
                                "`?` on a `Result` in a function that does not return `Result`",
                            )
                            .primary(e.span, "this can return an error")
                            .note(format!("the enclosing function returns `{shown}`")),
                        );
                    }
                },
            }
            return ok;
        }
        if let Some(args) = self.args_of(&t, self.b.option) {
            let some = args.first().cloned().unwrap_or(Ty::Error);
            match &ret_s {
                Ty::Error => {}
                Ty::Var(_) => {
                    let inner = self.infer.fresh();
                    let r = self
                        .b
                        .option
                        .map_or(Ty::Error, |d| Ty::Named(d, vec![inner]));
                    let _ = self.infer.unify(&ret_s, &r);
                }
                _ if self.is(&ret_s, self.b.option) => {}
                _ => {
                    let shown = self.show(&ret_s);
                    self.report(
                        Diagnostic::new(
                            codes::E0408,
                            "`?` on an `Option` in a function that does not return `Option`",
                        )
                        .primary(e.span, "this can return `None`")
                        .note(format!("the enclosing function returns `{shown}`"))
                        .help("turn the `Option` into a value with `match`"),
                    );
                }
            }
            return some;
        }
        match &t {
            Ty::Error | Ty::Never => Ty::Error,
            Ty::Var(_) => {
                self.unknown_receiver(inner.span, "`?`");
                self.infer.poison(&t);
                Ty::Error
            }
            _ => {
                let shown = self.show(&t);
                self.report(
                    Diagnostic::new(codes::E0405, format!("`?` cannot be applied to `{shown}`"))
                        .primary(e.span, format!("this is `{shown}`"))
                        .note("`?` works on `Result` and `Option`"),
                );
                Ty::Error
            }
        }
    }

    fn unary(&mut self, b: &mut Body, e: &ast::Expr, op: UnaryOp, operand: &ast::Expr) -> Ty {
        if op == UnaryOp::Neg {
            if let ExprKind::Int(value) = operand.kind {
                // `-9223372036854775808` is the smallest `Int` (chapter 1, rule 5.1.4).
                if value <= i64::MAX as u64 + 1 {
                    let int = self.int();
                    self.record(b, operand, &int);
                    return int;
                }
            }
        }
        let t = self.expr(b, operand, None);
        let s = self.infer.shallow(&t);
        if matches!(s, Ty::Error | Ty::Never) {
            return Ty::Error;
        }
        match op {
            UnaryOp::Not => {
                let bool_ty = self.bool();
                if matches!(s, Ty::Var(_)) {
                    let _ = self.infer.unify(&s, &bool_ty);
                } else if !self.is(&s, self.b.bool) {
                    let shown = self.show(&s);
                    self.report(
                        Diagnostic::new(
                            codes::E0405,
                            format!("`!` cannot be applied to `{shown}`"),
                        )
                        .primary(e.span, format!("the operand is `{shown}`"))
                        .note("`!` works on `Bool`; `~` is the bitwise not of `Int`"),
                    );
                }
                bool_ty
            }
            UnaryOp::BitNot => {
                let int = self.int();
                if matches!(s, Ty::Var(_)) {
                    let _ = self.infer.unify(&s, &int);
                } else if !self.is(&s, self.b.int) {
                    let shown = self.show(&s);
                    self.report(
                        Diagnostic::new(
                            codes::E0405,
                            format!("`~` cannot be applied to `{shown}`"),
                        )
                        .primary(e.span, format!("the operand is `{shown}`"))
                        .note("bitwise operators work only on `Int`"),
                    );
                }
                int
            }
            UnaryOp::Neg => {
                let op = Op::Arith("-");
                if matches!(s, Ty::Var(_)) {
                    b.deferred.push(Deferred::Operator {
                        op,
                        ty: s.clone(),
                        span: e.span,
                    });
                } else if !self.op_accepts(op, &s) {
                    self.op_error(op, &s, e.span);
                }
                s
            }
        }
    }

    fn op_accepts(&self, op: Op, ty: &Ty) -> bool {
        let int = self.is(ty, self.b.int);
        let float = self.is(ty, self.b.float);
        let string = self.is(ty, self.b.string);
        match op {
            Op::Add => int || float || string,
            Op::Arith(_) => int || float,
            Op::Bits(_) => int,
            Op::Order(_) => int || float || string,
        }
    }

    fn op_error(&mut self, op: Op, ty: &Ty, span: Span) {
        let shown = self.show(ty);
        let note = match op {
            Op::Add => "`+` works on `Int`, `Float` and `String`",
            Op::Arith(_) => "arithmetic works on `Int` and `Float`",
            Op::Bits(_) => "bitwise operators and shifts work only on `Int`",
            Op::Order(_) => {
                "`<`, `<=`, `>` and `>=` work on `Int`, `Float` and `String`; compare other types with `a.compare(b)`"
            }
        };
        self.report(
            Diagnostic::new(
                codes::E0405,
                format!("`{}` is not defined for `{shown}`", op.symbol()),
            )
            .primary(span, format!("the operands are `{shown}`"))
            .note(note),
        );
    }

    fn binary(
        &mut self,
        b: &mut Body,
        e: &ast::Expr,
        op: BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
    ) -> Ty {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let bool_ty = self.bool();
            for side in [lhs, rhs] {
                let t = self.expr(b, side, None);
                let s = self.infer.shallow(&t);
                if matches!(s, Ty::Var(_)) {
                    let _ = self.infer.unify(&s, &bool_ty);
                } else if !matches!(s, Ty::Error | Ty::Never) && !self.is(&s, self.b.bool) {
                    let shown = self.show(&s);
                    self.report(
                        Diagnostic::new(
                            codes::E0405,
                            format!("`{}` needs `Bool` operands, found `{shown}`", op.symbol()),
                        )
                        .primary(side.span, format!("this is `{shown}`")),
                    );
                }
            }
            return bool_ty;
        }
        let l = self.expr(b, lhs, None);
        let r = self.expr(b, rhs, None);
        self.binary_types(b, e.span, op, &l, &r)
    }

    /// The type of `l op r` (also used for compound assignment).
    fn binary_types(&mut self, b: &mut Body, span: Span, op: BinaryOp, l: &Ty, r: &Ty) -> Ty {
        let comparison = op.is_comparison();
        let bool_ty = self.bool();
        let ls = self.infer.shallow(l);
        let rs = self.infer.shallow(r);
        let result = |t: Ty| if comparison { bool_ty.clone() } else { t };
        if matches!(ls, Ty::Error) || matches!(rs, Ty::Error) {
            let _ = self.infer.unify(&ls, &rs);
            return result(Ty::Error);
        }
        if matches!(ls, Ty::Never) {
            return result(rs);
        }
        if matches!(rs, Ty::Never) {
            return result(ls);
        }
        if !self.infer.unify(&ls, &rs) {
            let l_s = self.show(&ls);
            let r_s = self.show(&rs);
            let message = if comparison {
                format!("cannot compare `{l_s}` with `{r_s}`")
            } else {
                format!("`{}` cannot be applied to `{l_s}` and `{r_s}`", op.symbol())
            };
            let mut d = Diagnostic::new(codes::E0405, message)
                .primary(span, format!("`{l_s}` {} `{r_s}`", op.symbol()));
            let mixed_numbers = (self.is(&ls, self.b.int) && self.is(&rs, self.b.float))
                || (self.is(&ls, self.b.float) && self.is(&rs, self.b.int));
            if mixed_numbers {
                d = d.help("convert one side: `x.to_float()` or `x.to_int()`");
            } else if self.is(&ls, self.b.string) || self.is(&rs, self.b.string) {
                d = d.help("convert the other side with `.to_string()`");
            } else {
                d = d.note("both operands must have the same type");
            }
            self.report(d);
            return result(Ty::Error);
        }
        let t = self.infer.shallow(&ls);
        let class = match op {
            BinaryOp::Add => Some(Op::Add),
            BinaryOp::Sub => Some(Op::Arith("-")),
            BinaryOp::Mul => Some(Op::Arith("*")),
            BinaryOp::Div => Some(Op::Arith("/")),
            BinaryOp::Rem => Some(Op::Arith("%")),
            BinaryOp::BitAnd => Some(Op::Bits("&")),
            BinaryOp::BitOr => Some(Op::Bits("|")),
            BinaryOp::BitXor => Some(Op::Bits("^")),
            BinaryOp::Shl => Some(Op::Bits("<<")),
            BinaryOp::Shr => Some(Op::Bits(">>")),
            BinaryOp::Lt => Some(Op::Order("<")),
            BinaryOp::Le => Some(Op::Order("<=")),
            BinaryOp::Gt => Some(Op::Order(">")),
            BinaryOp::Ge => Some(Op::Order(">=")),
            BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::And | BinaryOp::Or => None,
        };
        match class {
            Some(class) => {
                if matches!(t, Ty::Var(_)) {
                    b.deferred.push(Deferred::Operator {
                        op: class,
                        ty: t.clone(),
                        span,
                    });
                } else if !self.op_accepts(class, &t) {
                    self.op_error(class, &t, span);
                    return result(Ty::Error);
                }
            }
            None => {
                if let Some(equatable) = self.b.equatable {
                    b.deferred.push(Deferred::Bound {
                        ty: t.clone(),
                        interface: equatable,
                        span,
                        why: format!("`{}`", op.symbol()),
                    });
                }
            }
        }
        result(t)
    }

    fn closure(
        &mut self,
        b: &mut Body,
        params: &[ast::ClosureParam],
        ret: Option<&ast::Ty>,
        body: &ast::Expr,
        expected: Option<&Ty>,
    ) -> Ty {
        let expected_fn = match expected.map(|x| self.infer.shallow(x)) {
            Some(Ty::Fn(ps, r)) if ps.len() == params.len() => Some((ps, *r)),
            _ => None,
        };
        let scope = self.scope(b);
        let mut param_tys = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let from_expected = expected_fn.as_ref().and_then(|(ps, _)| ps.get(i).cloned());
            let mode = from_expected.as_ref().map_or(Mode::Read, |(m, _)| *m);
            let ty = match &p.ty {
                Some(ann) => {
                    let t = self.lower(ann, &scope);
                    if let Some((_, x)) = &from_expected {
                        if !self.infer.unify(&t, x) {
                            self.mismatch(ann.span, x, &t);
                        }
                    }
                    t
                }
                None => from_expected.map_or_else(|| self.infer.fresh(), |(_, t)| t),
            };
            if let Some(id) = self.resolved.local_decls.get(&p.name.span) {
                b.locals.insert(*id, ty.clone());
            }
            param_tys.push((mode, ty));
        }
        let ret_ty = match ret {
            Some(r) => {
                let t = self.lower(r, &scope);
                if let Some((_, x)) = &expected_fn {
                    let _ = self.infer.unify(&t, x);
                }
                t
            }
            None => expected_fn.map_or_else(|| self.infer.fresh(), |(_, r)| r),
        };
        b.returns.push(ret_ty.clone());
        b.loops.push(Vec::new());
        match &body.kind {
            ExprKind::Block(block) => {
                let t = self.block(b, block, Some(&ret_ty), None);
                self.record(b, body, &t);
            }
            _ => {
                if self.infer.is_unknown(&ret_ty)
                    && matches!(self.infer.shallow(&ret_ty), Ty::Var(_))
                {
                    let t = self.expr(b, body, None);
                    if !self.is_never(&t) {
                        let _ = self.infer.unify(&ret_ty, &t);
                    }
                } else {
                    self.value_or_discard(b, body, &ret_ty);
                }
            }
        }
        b.returns.pop();
        b.loops.pop();
        Ty::Fn(param_tys, Box::new(ret_ty))
    }

    // ----- blocks and statements --------------------------------------------------------------

    /// Checks a block. `fn_name` is set for the body of a function, for E0421.
    pub(crate) fn block(
        &mut self,
        b: &mut Body,
        block: &ast::Block,
        expected: Option<&Ty>,
        fn_name: Option<Span>,
    ) -> Ty {
        let void_expected = expected.is_some_and(|x| self.is_void(x));
        let mut diverges = false;
        let n = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            if i + 1 == n {
                if let StmtKind::Expr(e) = &stmt.kind {
                    if void_expected {
                        let t = self.discard(b, e);
                        return if diverges || self.is_never(&t) {
                            Ty::Never
                        } else {
                            self.void()
                        };
                    }
                    return match expected {
                        Some(x) => {
                            let found = self.expr_found(b, e, Some(x));
                            if fn_name.is_some()
                                && self.is_void(&found)
                                && !self.is_error(x)
                                && !self.is_void(x)
                            {
                                self.missing_result(b, block, fn_name, x);
                                x.clone()
                            } else if diverges && !self.is_never(&found) {
                                self.coerce(b, Some(e.id), e.span, &found, x);
                                Ty::Never
                            } else {
                                self.coerce(b, Some(e.id), e.span, &found, x)
                            }
                        }
                        None => {
                            let t = self.expr(b, e, None);
                            if diverges { Ty::Never } else { t }
                        }
                    };
                }
            }
            if self.stmt(b, stmt) {
                diverges = true;
            }
        }
        let t = if diverges { Ty::Never } else { self.void() };
        if let Some(x) = expected {
            if !diverges && !self.is_void(x) && !self.is_error(x) {
                if fn_name.is_some() {
                    self.missing_result(b, block, fn_name, x);
                } else {
                    let void = self.void();
                    let end = block_end(block);
                    self.coerce(b, None, end, &void, x);
                }
                return x.clone();
            }
        }
        t
    }

    fn missing_result(
        &mut self,
        _b: &mut Body,
        block: &ast::Block,
        fn_name: Option<Span>,
        ret: &Ty,
    ) {
        let shown = self.show(ret);
        let mut d = Diagnostic::new(codes::E0421, "this function may end without a value")
            .primary(
                block_end(block),
                format!("the end is reached without a value of type `{shown}`"),
            )
            .help("end the body with a value, or `return` one on every path");
        if let Some(name) = fn_name {
            d = d.secondary(name, format!("the function returns `{shown}`"));
        }
        self.report(d);
    }

    /// Checks a statement. Returns whether it never finishes (has type Never).
    fn stmt(&mut self, b: &mut Body, stmt: &ast::Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Let {
                name, ty, value, ..
            } => {
                let t = match ty {
                    Some(ann) => {
                        let scope = self.scope(b);
                        let lowered = self.lower(ann, &scope);
                        let found = self.expr(b, value, Some(&lowered));
                        if self.is_never(&found) {
                            if let Some(name) = name {
                                self.declare(b, name, lowered);
                            }
                            return true;
                        }
                        lowered
                    }
                    None => self.expr(b, value, None),
                };
                let never = self.is_never(&t);
                if let Some(name) = name {
                    self.declare(b, name, t);
                }
                never
            }
            StmtKind::Assign { target, op, value } => {
                let target_ty = self.expr(b, target, None);
                match assign_binary(*op) {
                    None => {
                        self.expr(b, value, Some(&target_ty));
                    }
                    Some(bin) => {
                        let value_ty = self.expr(b, value, None);
                        let result = self.binary_types(b, stmt.span, bin, &target_ty, &value_ty);
                        let _ = self.infer.unify(&result, &target_ty);
                    }
                }
                false
            }
            StmtKind::LetPattern { pattern, value, .. } => {
                let t = self.expr(b, value, None);
                self.pattern(b, pattern, &t);
                if !self.irrefutable(b, pattern) {
                    self.report(
                        Diagnostic::new(codes::E0433, "this pattern may not match")
                            .primary(pattern.span, "`let` needs a pattern that always matches")
                            .help("use `if let` or `match` to handle the other values"),
                    );
                }
                self.is_never(&t)
            }
            StmtKind::While { label, cond, body } => {
                let bool_ty = self.bool();
                self.expr(b, cond, Some(&bool_ty));
                self.push_loop(b, label.as_ref(), false);
                let void = self.void();
                self.block(b, body, Some(&void), None);
                let has_break = self.pop_loop(b);
                // `while true` without its own `break` never ends (chapter 4, rule 6.9).
                matches!(cond.kind, ExprKind::Bool(true)) && !has_break
            }
            StmtKind::WhileLet { label, items, body } => {
                self.cond_items(b, items);
                self.push_loop(b, label.as_ref(), false);
                let void = self.void();
                self.block(b, body, Some(&void), None);
                self.pop_loop(b);
                false
            }
            StmtKind::For {
                label,
                var,
                iter,
                body,
            } => {
                let elem = match iter {
                    ForIter::Collection { expr, .. } => {
                        let t = self.expr(b, expr, None);
                        let s = self.infer.shallow(&t);
                        match self.args_of(&s, self.b.list) {
                            Some(args) => args.first().cloned().unwrap_or(Ty::Error),
                            None => match &s {
                                Ty::Error | Ty::Never => Ty::Error,
                                Ty::Var(_) => {
                                    self.unknown_receiver(expr.span, "`for`");
                                    self.infer.poison(&s);
                                    Ty::Error
                                }
                                _ => {
                                    let shown = self.show(&s);
                                    let mut d = Diagnostic::new(
                                        codes::E0401,
                                        format!("`for` needs a `List` or a range, found `{shown}`"),
                                    )
                                    .primary(expr.span, format!("this is `{shown}`"));
                                    if self.is(&s, self.b.map) {
                                        d = d.help("iterate over the keys: `for k in m.keys()`");
                                    } else if self.is(&s, self.b.string) {
                                        d = d.help(
                                            "iterate over the characters: `for c in s.chars()`",
                                        );
                                    }
                                    self.report(d);
                                    Ty::Error
                                }
                            },
                        }
                    }
                    ForIter::Range { start, end, .. } => {
                        let int = self.int();
                        self.expr(b, start, Some(&int));
                        self.expr(b, end, Some(&int));
                        int
                    }
                };
                self.declare(b, var, elem);
                self.push_loop(b, label.as_ref(), false);
                let void = self.void();
                self.block(b, body, Some(&void), None);
                self.pop_loop(b);
                false
            }
            StmtKind::Expr(e) => {
                let t = self.discard(b, e);
                self.is_never(&t)
            }
        }
    }

    fn push_loop(&mut self, b: &mut Body, label: Option<&ast::Ident>, is_loop: bool) {
        if let Some(loops) = b.loops.last_mut() {
            loops.push(LoopFrame {
                label: label.map(|l| l.name.clone()),
                is_loop,
                has_break: false,
                value: None,
            });
        }
    }

    /// Leaves a loop; whether it had a `break` of its own.
    fn pop_loop(&mut self, b: &mut Body) -> bool {
        b.loops
            .last_mut()
            .and_then(|l| l.pop())
            .is_none_or(|f| f.has_break)
    }

    /// Requires `ty` to implement `Display` (chapter 4, section 9).
    pub(crate) fn require_display(&mut self, b: &mut Body, ty: &Ty, span: Span, why: &str) {
        if let Some(display) = self.b.display {
            b.deferred.push(Deferred::Bound {
                ty: ty.clone(),
                interface: display,
                span,
                why: why.to_string(),
            });
        }
    }

    pub(crate) fn declare(&mut self, b: &mut Body, name: &ast::Ident, ty: Ty) {
        if let Some(id) = self.resolved.local_decls.get(&name.span) {
            b.locals.insert(*id, ty);
        }
    }
}

fn assign_binary(op: AssignOp) -> Option<BinaryOp> {
    match op {
        AssignOp::Assign => None,
        AssignOp::Add => Some(BinaryOp::Add),
        AssignOp::Sub => Some(BinaryOp::Sub),
        AssignOp::Mul => Some(BinaryOp::Mul),
        AssignOp::Div => Some(BinaryOp::Div),
        AssignOp::Rem => Some(BinaryOp::Rem),
        AssignOp::BitAnd => Some(BinaryOp::BitAnd),
        AssignOp::BitOr => Some(BinaryOp::BitOr),
        AssignOp::BitXor => Some(BinaryOp::BitXor),
        AssignOp::Shl => Some(BinaryOp::Shl),
        AssignOp::Shr => Some(BinaryOp::Shr),
    }
}

/// The closing brace of a block.
fn block_end(block: &ast::Block) -> Span {
    let end = block.span.end;
    Span::new(
        block.span.file,
        end.saturating_sub(1).max(block.span.start),
        end,
    )
}

/// How a callee is named in messages.
fn callee_name(callee: &ast::Expr) -> String {
    match &callee.kind {
        ExprKind::Name(name) => format!("`{}`", name.name),
        ExprKind::Field { name, .. } => format!("`{}`", name.name),
        ExprKind::TypeArgs { base, .. } => callee_name(base),
        _ => "this function".to_string(),
    }
}

/// `Type` for `Type.<Args>`, which name resolution records under `Type`; other expressions as
/// they are.
fn without_type_args(e: &ast::Expr) -> &ast::Expr {
    match &e.kind {
        ExprKind::TypeArgs { base, .. } => base,
        _ => e,
    }
}
