//! IR to the Go tree (spec chapter 8).
//!
//! Main decisions, all [INTERNAL] in the spec (ADR 0009):
//!
//! - Each module is a package. std has no packages: its declarations and intrinsics are part of
//!   the runtime package `wolfrt`.
//! - `Option` and `Result` use the hybrid representation of ADR 0002. Every local, field and
//!   parameter holds the data form (`wolfrt.Option[T]`, `wolfrt.Result[T, E]`); only the results
//!   of declared functions use the idiomatic forms `(T, bool)`, `*T`, `(T, error)` and `error`.
//!   Each call knows the *form* of its result, and conversions are inserted where a result is
//!   used as data.
//! - `mut` parameters are pointers. The variable of `for x in mut` is a pointer to the element,
//!   and bindings of `match mut` are aliases of the places they name.
//! - Variants of an enum with payloads are pointers to structs, so that `match mut` can change a
//!   payload in place.
//! - Bounds `Equatable`, `Copyable` and `Comparable` are passed as functions (dictionary
//!   passing, spec chapter 8, section 5.4); `Hashable` is `comparable`, interfaces of the program
//!   are constraints.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ir::{
    self as ir, Arm, BinaryOp, Block, Callee, DefKind, DefRef, Expr as IrExpr, ExprKind, LocalRef,
    Mode, ModuleRole, NativeTy, Pattern, Payload, Program, ProgramKind, Stmt as IrStmt,
    Type as IrType, UnaryOp,
};

use crate::go::{self, Decl, Expr, Field, Func, IfaceElem, Stmt, Type};
use crate::names::{camel, case, local as local_name_of, type_name, value_name};

/// A construct the backend does not support yet.
pub type Error = String;

/// How the result of a call is represented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    /// One value; for `Option` and `Result`, the data form.
    Data,
    /// `(T, bool)`
    OptPair,
    /// `*T`
    OptPtr,
    /// `(T, error)`
    ResPair,
    /// `error`, for `Result<Void, E>`.
    ResErr,
}

/// Where the value of a block goes.
#[derive(Clone)]
enum Target {
    Discard,
    Return,
    Assign(Expr),
}

/// How a local is reached.
#[derive(Clone)]
enum Local {
    /// A variable that holds the value.
    Val(String),
    /// A pointer: the value is `*name`.
    Ptr(String),
    /// Another place: a binding of `match mut`.
    Alias(Expr),
}

/// The bounds that become dictionary parameters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Dict {
    Eq,
    Copy,
    Cmp,
    /// `Display`: `func(T) string`.
    Show,
}

/// The signature facts calls need.
#[derive(Clone)]
struct FnInfo {
    params: Vec<(Mode, IrType)>,
    ret: IrType,
    generics: Vec<ir::Generic>,
    /// For methods: the type's parameters with the bounds of the `impl` block.
    owner_generics: Vec<ir::Generic>,
    owner: Option<DefRef>,
    self_mode: Option<Mode>,
    interface: Option<DefRef>,
}

struct Loop {
    /// The Go label, once a `break` or a `continue` needs one.
    label: Option<String>,
    /// The label of the program, if the loop has one.
    user: Option<String>,
    /// Go `switch` statements around the loop when it started.
    switches: usize,
    /// For a `loop` whose value is used: where `break value` sends the value.
    target: Option<Target>,
}

/// Facts about the function being emitted.
#[derive(Default)]
struct FnState {
    locals: BTreeMap<LocalRef, Local>,
    used: BTreeSet<String>,
    /// Names of the program's own locals.
    user: BTreeSet<String>,
    reads: BTreeSet<LocalRef>,
    assigned: BTreeSet<LocalRef>,
    /// Result types of the function and the closures around the current point, innermost
    /// last, with the form of the result.
    rets: Vec<(IrType, Form)>,
    self_local: Option<Local>,
    /// Dictionary parameters in scope: type parameter and bound to the parameter's name.
    dicts: BTreeMap<(String, Dict), String>,
    loops: Vec<Loop>,
    switches: usize,
}

/// A Go package for a module.
#[derive(Clone)]
struct Pkg {
    /// The import path.
    path: String,
    /// The package clause.
    name: String,
    /// The file of the module, relative to `out/go/`.
    file: String,
    /// The file of its tests.
    test_file: String,
    /// The package clause of the test file (`x_test` for `tests/`).
    test_package: String,
}

/// Imports and names of the file being written.
#[derive(Default)]
struct FileCtx {
    imports: BTreeSet<String>,
    idents: BTreeSet<String>,
}

/// A test of a test build: its package, Go name and the name in the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestEntry {
    /// The import path of the package that holds it.
    pub package: String,
    /// The Go test function.
    pub go_name: String,
    /// `src/main.wolf: adds two numbers`
    pub name: String,
}

pub(crate) struct Emitter<'p> {
    p: &'p Program,
    tests: bool,
    module_path: String,
    root_pkg: String,
    fns: BTreeMap<DefRef, FnInfo>,
    iface_fns: BTreeMap<(DefRef, String), FnInfo>,
    /// The interface whose default is being emitted: there `Self` is that interface.
    self_iface: Option<IrType>,
    enums: BTreeMap<DefRef, &'p ir::Enum>,
    structs: BTreeMap<DefRef, &'p ir::Struct>,
    ifaces: BTreeMap<DefRef, &'p ir::Interface>,
    /// The Go name of each declaration in its package.
    names: BTreeMap<DefRef, String>,
    /// Declarations of std: the runtime piece that holds them.
    std_pieces: BTreeMap<DefRef, &'static str>,
    /// Names the root package of a library exports again.
    reexported: BTreeMap<DefRef, String>,
    pkgs: Vec<Option<Pkg>>,
    /// Types whose methods take a pointer receiver.
    ptr_recv: BTreeSet<DefRef>,
    /// Fields that make a struct recursive through `Option` or `Result` (`next: Option<Node>`):
    /// they hold a pointer, `*wolfrt.Option[Node]` (struct, field index).
    boxed: BTreeSet<(DefRef, usize)>,
    /// Types that implement `Error`.
    error_types: BTreeSet<DefRef>,
    error_iface: Option<DefRef>,
    comparable: Option<DefRef>,
    display: Option<DefRef>,
    equatable: Option<DefRef>,
    copyable: Option<DefRef>,
    hashable: Option<DefRef>,
    /// Runtime pieces used.
    pub(crate) rt: BTreeSet<&'static str>,
    /// Types whose generated `Equal` and `Copy` are used.
    pub(crate) need_eq: BTreeSet<DefRef>,
    pub(crate) need_copy: BTreeSet<DefRef>,
    /// Interfaces whose values are copied: they get a `Copy<Name>` method.
    pub(crate) need_copy_iface: BTreeSet<DefRef>,
    /// Tests of the test build, in order.
    pub(crate) test_list: Vec<TestEntry>,
    test_names: BTreeSet<String>,
    // State of the module being emitted.
    module: usize,
    file: FileCtx,
    /// Top-level Go names of the current package.
    taken: BTreeSet<String>,
    f: FnState,
}

// ----- small helpers --------------------------------------------------------------------------

fn id(name: &str) -> Expr {
    Expr::Ident(name.to_string())
}

fn lit(text: &str) -> Expr {
    Expr::Lit(text.to_string())
}

fn sel(base: Expr, name: &str) -> Expr {
    Expr::Sel(Box::new(base), name.to_string())
}

fn call(f: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call(Box::new(f), Vec::new(), args, false)
}

fn callt(f: Expr, targs: Vec<Type>, args: Vec<Expr>) -> Expr {
    Expr::Call(Box::new(f), targs, args, false)
}

fn bin(a: Expr, op: &'static str, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), op, Box::new(b))
}

fn not(e: Expr) -> Expr {
    match e {
        Expr::Unary("!", inner) => *inner,
        Expr::Binary(a, "==", b) => Expr::Binary(a, "!=", b),
        Expr::Binary(a, "!=", b) => Expr::Binary(a, "==", b),
        other => Expr::Unary("!", Box::new(other)),
    }
}

fn star(e: Expr) -> Expr {
    match e {
        Expr::Unary("&", inner) => *inner,
        other => Expr::Star(Box::new(other)),
    }
}

fn addr(e: Expr) -> Expr {
    match e {
        Expr::Star(inner) => *inner,
        other => Expr::Unary("&", Box::new(other)),
    }
}

fn unit() -> Expr {
    Expr::Composite(Type::EmptyStruct, Vec::new())
}

fn int64() -> Type {
    Type::name("int64")
}

/// Expressions that may be evaluated twice or out of order.
fn pure(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) | Expr::Qual(..) | Expr::Lit(_) | Expr::Str(_) => true,
        Expr::Sel(base, _) | Expr::Star(base) => pure(base),
        Expr::Unary(_, base) => pure(base),
        Expr::Binary(a, _, b) => pure(a) && pure(b),
        Expr::Composite(_, elems) => elems.is_empty(),
        _ => false,
    }
}

/// An integer constant without a type: Go would give it the type `int`.
fn untyped_int(e: &Expr) -> bool {
    match e {
        Expr::Lit(t) => {
            t.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                && !t.contains('.')
                && !t.contains('e')
        }
        Expr::Unary("-", x) => untyped_int(x),
        _ => false,
    }
}

/// A constant expression of Go.
fn constant(e: &Expr) -> bool {
    match e {
        Expr::Lit(_) => true,
        Expr::Unary(_, x) => constant(x),
        Expr::Binary(a, _, b) => constant(a) && constant(b),
        _ => false,
    }
}

/// Whether a Go statement list ends in a terminating statement (Go spec, "Terminating
/// statements"), so that nothing needs to follow it.
fn terminates(stmts: &[Stmt]) -> bool {
    match stmts.last() {
        Some(s) => terminating(s),
        None => false,
    }
}

fn terminating(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) => true,
        Stmt::Expr(Expr::Call(f, _, _, _)) => matches!(f.as_ref(), Expr::Ident(n) if n == "panic"),
        Stmt::If(_, _, then, Some(otherwise)) => terminates(then) && terminates(otherwise),
        Stmt::For(_, None, _, body) => !breaks(body, true),
        Stmt::Switch(_, cases) => {
            cases.iter().any(|(l, _)| l.is_none())
                && cases.iter().all(|(_, b)| terminates(b) && !breaks(b, true))
        }
        Stmt::TypeSwitch(_, _, cases) => {
            cases.iter().any(|(l, _)| l.is_none())
                && cases.iter().all(|(_, b)| terminates(b) && !breaks(b, true))
        }
        Stmt::Labeled(label, inner) => terminating(inner) && !breaks_label(inner, label),
        _ => false,
    }
}

/// Whether a body has an unlabeled `break` that leaves the statement around it.
fn breaks(stmts: &[Stmt], top: bool) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Break(None) => top,
        Stmt::If(_, _, a, b) => breaks(a, top) || b.as_ref().is_some_and(|b| breaks(b, top)),
        Stmt::Labeled(_, inner) => breaks(std::slice::from_ref(inner), top),
        _ => false,
    })
}

fn breaks_label(s: &Stmt, label: &str) -> bool {
    let inner = |b: &[Stmt]| b.iter().any(|s| breaks_label(s, label));
    match s {
        Stmt::Break(Some(l)) => l == label,
        Stmt::If(_, _, a, b) => inner(a) || b.as_ref().is_some_and(|b| inner(b)),
        Stmt::For(_, _, _, b) | Stmt::Range(_, _, _, b) => inner(b),
        Stmt::Switch(_, cases) => cases.iter().any(|(_, b)| inner(b)),
        Stmt::TypeSwitch(_, _, cases) => cases.iter().any(|(_, b)| inner(b)),
        Stmt::Labeled(_, s) => breaks_label(s, label),
        _ => false,
    }
}

/// `t` with `Self` replaced.
fn replace_self(t: &IrType, with: &IrType) -> IrType {
    match t {
        IrType::SelfType => with.clone(),
        IrType::List(a) => IrType::List(Box::new(replace_self(a, with))),
        IrType::Map(a, b) => IrType::Map(
            Box::new(replace_self(a, with)),
            Box::new(replace_self(b, with)),
        ),
        IrType::Option(a) => IrType::Option(Box::new(replace_self(a, with))),
        IrType::Result(a, b) => IrType::Result(
            Box::new(replace_self(a, with)),
            Box::new(replace_self(b, with)),
        ),
        IrType::Named(d, args) => {
            IrType::Named(*d, args.iter().map(|a| replace_self(a, with)).collect())
        }
        IrType::Fn(ps, r) => IrType::Fn(
            ps.iter()
                .map(|(m, t)| (*m, replace_self(t, with)))
                .collect(),
            Box::new(replace_self(r, with)),
        ),
        _ => t.clone(),
    }
}

fn subst(t: &IrType, map: &BTreeMap<String, IrType>) -> IrType {
    match t {
        IrType::Param(n) => map.get(n).cloned().unwrap_or_else(|| t.clone()),
        IrType::List(a) => IrType::List(Box::new(subst(a, map))),
        IrType::Map(a, b) => IrType::Map(Box::new(subst(a, map)), Box::new(subst(b, map))),
        IrType::Option(a) => IrType::Option(Box::new(subst(a, map))),
        IrType::Result(a, b) => IrType::Result(Box::new(subst(a, map)), Box::new(subst(b, map))),
        IrType::Named(d, args) => IrType::Named(*d, args.iter().map(|a| subst(a, map)).collect()),
        IrType::Fn(ps, r) => IrType::Fn(
            ps.iter().map(|(m, t)| (*m, subst(t, map))).collect(),
            Box::new(subst(r, map)),
        ),
        _ => t.clone(),
    }
}

/// Binds the type parameters of `pattern` to the parts of `actual`.
fn unify(pattern: &IrType, actual: &IrType, map: &mut BTreeMap<String, IrType>) {
    match (pattern, actual) {
        (IrType::Param(n), a) => {
            if !matches!(a, IrType::Never) {
                map.entry(n.clone()).or_insert_with(|| a.clone());
            }
        }
        (IrType::List(a), IrType::List(b)) | (IrType::Option(a), IrType::Option(b)) => {
            unify(a, b, map)
        }
        (IrType::Map(a, b), IrType::Map(c, d)) | (IrType::Result(a, b), IrType::Result(c, d)) => {
            unify(a, c, map);
            unify(b, d, map);
        }
        (IrType::Named(d1, a1), IrType::Named(d2, a2)) if d1 == d2 => {
            for (x, y) in a1.iter().zip(a2) {
                unify(x, y, map);
            }
        }
        (IrType::Fn(p1, r1), IrType::Fn(p2, r2)) => {
            for ((_, x), (_, y)) in p1.iter().zip(p2) {
                unify(x, y, map);
            }
            unify(r1, r2, map);
        }
        _ => {}
    }
}

fn mentions(t: &IrType, name: &str) -> bool {
    match t {
        IrType::Param(n) => n == name,
        IrType::List(a) | IrType::Option(a) => mentions(a, name),
        IrType::Map(a, b) | IrType::Result(a, b) => mentions(a, name) || mentions(b, name),
        IrType::Named(_, args) => args.iter().any(|a| mentions(a, name)),
        IrType::Fn(ps, r) => ps.iter().any(|(_, t)| mentions(t, name)) || mentions(r, name),
        _ => false,
    }
}

/// Whether emitting the expression may need statements before it.
fn needs_stmts(e: &IrExpr) -> bool {
    match &e.kind {
        ExprKind::If { .. }
        | ExprKind::Match { .. }
        | ExprKind::Block(_)
        | ExprKind::Try { .. }
        | ExprKind::Return(_)
        | ExprKind::Loop { .. }
        | ExprKind::Break { .. }
        | ExprKind::Continue { .. }
        | ExprKind::Panic { .. } => true,
        ExprKind::Interpolate(parts) => parts.iter().any(needs_stmts),
        ExprKind::Call { callee, args } => {
            let c = match callee {
                Callee::Method { recv, .. }
                | Callee::Interface { recv, .. }
                | Callee::Default { recv, .. } => needs_stmts(recv),
                Callee::Value(f) => needs_stmts(f),
                Callee::Fn(_) => false,
            };
            c || args.iter().any(|a| needs_stmts(&a.value))
        }
        ExprKind::Field { base, .. } | ExprKind::Copy(base) => needs_stmts(base),
        ExprKind::Index { base, index } => needs_stmts(base) || needs_stmts(index),
        ExprKind::Slice {
            base, start, end, ..
        } => {
            needs_stmts(base)
                || start.as_ref().is_some_and(|s| needs_stmts(s))
                || end.as_ref().is_some_and(|s| needs_stmts(s))
        }
        ExprKind::Struct { fields, base, .. } => {
            fields.iter().any(|(_, v)| needs_stmts(v))
                || base.as_ref().is_some_and(|b| needs_stmts(b))
        }
        ExprKind::Variant { values, .. } | ExprKind::List(values) => values.iter().any(needs_stmts),
        ExprKind::Map(entries) => entries
            .iter()
            .any(|(k, v)| needs_stmts(k) || needs_stmts(v)),
        ExprKind::Some(v) | ExprKind::Ok(v) | ExprKind::Err(v) => needs_stmts(v),
        ExprKind::Unary { operand, .. } => needs_stmts(operand),
        ExprKind::Binary { lhs, rhs, .. } => needs_stmts(lhs) || needs_stmts(rhs),
        ExprKind::Coerce { value, .. } => needs_stmts(value),
        ExprKind::Assert { cond, .. } => needs_stmts(cond),
        _ => false,
    }
}

// ----- the scan of locals ---------------------------------------------------------------------

/// Locals that a body reads and locals it assigns: Go rejects variables that are never read.
#[derive(Default)]
struct Scan {
    reads: BTreeSet<LocalRef>,
    assigned: BTreeSet<LocalRef>,
}

impl Scan {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn stmt(&mut self, s: &IrStmt) {
        match s {
            IrStmt::Let { value, .. } | IrStmt::LetPattern { value, .. } => self.expr(value),
            IrStmt::Assign { target, value, .. } => {
                match &target.kind {
                    ExprKind::Local(l) => {
                        self.assigned.insert(*l);
                    }
                    _ => self.expr(target),
                }
                self.expr(value);
            }
            IrStmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            IrStmt::ForList { list, body, .. } => {
                self.expr(list);
                self.block(body);
            }
            IrStmt::ForRange {
                start, end, body, ..
            } => {
                self.expr(start);
                self.expr(end);
                self.block(body);
            }
            IrStmt::Expr(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &IrExpr) {
        match &e.kind {
            ExprKind::Local(l) => {
                self.reads.insert(*l);
            }
            ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Method { recv, .. }
                    | Callee::Interface { recv, .. }
                    | Callee::Default { recv, .. } => self.expr(recv),
                    Callee::Value(f) => self.expr(f),
                    Callee::Fn(_) => {}
                }
                for a in args {
                    self.expr(&a.value);
                }
            }
            ExprKind::Field { base, .. }
            | ExprKind::Copy(base)
            | ExprKind::Some(base)
            | ExprKind::Ok(base)
            | ExprKind::Err(base) => self.expr(base),
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base);
                if let Some(s) = start {
                    self.expr(s);
                }
                if let Some(s) = end {
                    self.expr(s);
                }
            }
            ExprKind::Struct { fields, base, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
                if let Some(b) = base {
                    self.expr(b);
                }
            }
            ExprKind::Interpolate(values) => {
                for v in values {
                    self.expr(v);
                }
            }
            ExprKind::Loop { body, .. } => self.block(body),
            ExprKind::Break { value: Some(v), .. } => self.expr(v),
            ExprKind::Variant { values, .. } | ExprKind::List(values) => {
                for v in values {
                    self.expr(v);
                }
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Closure { body, .. } => self.block(body),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.block(o);
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for a in arms {
                    if let Some(g) = &a.guard {
                        self.expr(g);
                    }
                    self.block(&a.body);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(Some(v)) => self.expr(v),
            ExprKind::Try { value, .. } | ExprKind::Coerce { value, .. } => self.expr(value),
            ExprKind::Panic { message, .. } => self.expr(message),
            ExprKind::Assert { cond, .. } => self.expr(cond),
            _ => {}
        }
    }
}

fn binds(p: &Pattern) -> bool {
    match p {
        Pattern::Bind(_) => true,
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => binds(i),
        Pattern::Variant { fields, .. } | Pattern::Struct { fields, .. } => {
            fields.iter().any(binds)
        }
        Pattern::Or(alts) => alts.iter().any(binds),
        _ => false,
    }
}

fn irrefutable(p: &Pattern) -> bool {
    matches!(p, Pattern::Wild | Pattern::Bind(_))
}

/// Patterns that test only the top level (or nothing).
fn top_only(p: &Pattern) -> bool {
    match p {
        Pattern::Wild | Pattern::Bind(_) | Pattern::None | Pattern::Bool(_) => true,
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => irrefutable(i),
        Pattern::Variant { fields, .. } | Pattern::Struct { fields, .. } => {
            fields.iter().all(irrefutable)
        }
        Pattern::Or(alts) => alts.iter().all(|a| top_only(a) && !binds(a)),
        Pattern::Int(_) | Pattern::Str(_) | Pattern::Const(_) => false,
    }
}

fn and_all(conds: Vec<Expr>) -> Option<Expr> {
    let mut it = conds.into_iter();
    let first = it.next()?;
    Some(it.fold(first, |acc, c| bin(acc, "&&", c)))
}

fn float_lit(v: f64) -> Result<Expr, Error> {
    if v.is_nan() || v.is_infinite() {
        return Err("a non-finite float literal".to_string());
    }
    let text = format!("{v:?}");
    Ok(lit(&text))
}

/// `TestAddsTwoNumbers` for `adds two numbers`.
#[cfg(test)]
pub(crate) fn test_name_for(name: &str) -> String {
    test_name(name)
}

fn test_name(name: &str) -> String {
    let mut out = String::from("Test");
    for word in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    out
}

impl<'p> Emitter<'p> {
    pub(crate) fn new(p: &'p Program, tests: bool) -> Emitter<'p> {
        let module_path = crate::module_path(&p.name);
        let root_pkg: String = p.name.chars().filter(|c| *c != '-').collect();
        let mut e = Emitter {
            p,
            tests,
            module_path,
            root_pkg,
            fns: BTreeMap::new(),
            iface_fns: BTreeMap::new(),
            self_iface: None,
            enums: BTreeMap::new(),
            structs: BTreeMap::new(),
            ifaces: BTreeMap::new(),
            names: BTreeMap::new(),
            std_pieces: BTreeMap::new(),
            reexported: BTreeMap::new(),
            pkgs: Vec::new(),
            ptr_recv: BTreeSet::new(),
            boxed: BTreeSet::new(),
            error_types: BTreeSet::new(),
            error_iface: None,
            comparable: None,
            display: None,
            equatable: None,
            copyable: None,
            hashable: None,
            rt: BTreeSet::new(),
            need_eq: BTreeSet::new(),
            need_copy: BTreeSet::new(),
            need_copy_iface: BTreeSet::new(),
            test_list: Vec::new(),
            test_names: BTreeSet::new(),
            module: 0,
            file: FileCtx::default(),
            taken: BTreeSet::new(),
            f: FnState::default(),
        };
        e.tables();
        e
    }

    /// Forgets the test names of an earlier round of emitting.
    pub(crate) fn reset_tests(&mut self) {
        self.test_names.clear();
    }

    fn wolfrt(&self) -> String {
        format!("{}/wolfrt", self.module_path)
    }

    /// A name of the runtime, using the piece that defines it.
    fn rt(&mut self, piece: &'static str, name: &str) -> Expr {
        self.rt.insert(piece);
        let path = self.wolfrt();
        self.file.imports.insert(path.clone());
        Expr::Qual(path, name.to_string())
    }

    fn rt_call(&mut self, piece: &'static str, name: &str, args: Vec<Expr>) -> Expr {
        let f = self.rt(piece, name);
        call(f, args)
    }

    fn rt_type(&mut self, piece: &'static str, name: &str, args: Vec<Type>) -> Type {
        self.rt.insert(piece);
        let path = self.wolfrt();
        self.file.imports.insert(path.clone());
        Type::Name(Some(path), name.to_string(), args)
    }

    /// A function of a standard Go package.
    fn std_call(&mut self, path: &str, name: &str, args: Vec<Expr>) -> Expr {
        self.file.imports.insert(path.to_string());
        call(Expr::Qual(path.to_string(), name.to_string()), args)
    }

    // ----- tables -----------------------------------------------------------------------------

    fn tables(&mut self) {
        let p = self.p;
        let info = |f: &ir::Function, owner: Option<(&[ir::Generic], DefRef)>| FnInfo {
            params: f.params.iter().map(|x| (x.mode, x.ty.clone())).collect(),
            ret: f.ret.clone(),
            generics: f.generics.clone(),
            owner_generics: owner
                .map(|(gs, _)| {
                    gs.iter()
                        .enumerate()
                        .map(|(i, g)| ir::Generic {
                            name: g.name.clone(),
                            bounds: f.impl_bounds.get(i).cloned().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            owner: owner.map(|(_, d)| d),
            self_mode: f.self_mode,
            interface: f.interface,
        };
        // Packages.
        for m in &p.modules {
            let pkg = match m.role {
                ModuleRole::Std => None,
                ModuleRole::Entry => {
                    let (name, file) = match p.kind {
                        ProgramKind::Cli => ("main".to_string(), "main.go".to_string()),
                        ProgramKind::Lib => {
                            (self.root_pkg.clone(), format!("{}.go", self.root_pkg))
                        }
                    };
                    Some(Pkg {
                        path: self.module_path.clone(),
                        test_file: file.replace(".go", "_test.go"),
                        test_package: name.clone(),
                        name,
                        file,
                    })
                }
                ModuleRole::Src | ModuleRole::Native => {
                    // `native.slug` is written next to the native code: `native/wolf/slug/`.
                    let segments: Vec<String> = if m.role == ModuleRole::Native {
                        ["native".to_string(), "wolf".to_string()]
                            .into_iter()
                            .chain(m.path.iter().skip(1).cloned())
                            .collect()
                    } else {
                        m.path.clone()
                    };
                    let dir = segments
                        .iter()
                        .map(|s| crate::names::safe(s.clone()))
                        .collect::<Vec<_>>()
                        .join("/");
                    let name = m
                        .path
                        .last()
                        .map(|s| crate::names::safe(s.clone()))
                        .unwrap_or_default();
                    Some(Pkg {
                        path: format!("{}/{dir}", self.module_path),
                        file: format!("{dir}/{name}.go"),
                        test_file: format!("{dir}/{name}_test.go"),
                        test_package: name.clone(),
                        name,
                    })
                }
                ModuleRole::Example => {
                    let dir = m.path.join("/");
                    Some(Pkg {
                        path: format!("{}/{dir}", self.module_path),
                        name: "main".to_string(),
                        file: format!("{dir}/main.go"),
                        test_file: format!("{dir}/main_test.go"),
                        test_package: "main".to_string(),
                    })
                }
                ModuleRole::Test => {
                    let rest: Vec<&str> = m.path.iter().skip(1).map(String::as_str).collect();
                    let name = format!("{}_test", self.root_pkg);
                    Some(Pkg {
                        path: format!("{}_test", self.module_path),
                        file: format!("{}_test.go", rest.join("_")),
                        test_file: format!("{}_test.go", rest.join("_")),
                        test_package: name.clone(),
                        name,
                    })
                }
            };
            self.pkgs.push(pkg);
        }
        // Declarations, with user names first and derived names after them.
        let mut derived: Vec<(DefRef, usize, String)> = Vec::new();
        let mut taken: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for (index, m) in p.modules.iter().enumerate() {
            let std = m.role == ModuleRole::Std;
            // A private type `Chain` and a private function `chain` are both `chain` in Go: the
            // declaration that comes later gets a `_` suffix (spec chapter 8, section 3, rule 4).
            let mut put = |names: &mut BTreeMap<DefRef, String>, def: DefRef, mut n: String| {
                let set = taken.entry(index).or_default();
                while set.contains(&n) {
                    n.push('_');
                }
                set.insert(n.clone());
                names.insert(def, n);
            };
            for item in &m.items {
                match item {
                    ir::Item::Struct(s) => {
                        put(&mut self.names, s.def, type_name(&s.name, s.public || std));
                        self.structs.insert(s.def, s);
                        if s.methods.iter().any(|f| f.self_mode == Some(Mode::Mut)) {
                            self.ptr_recv.insert(s.def);
                        }
                        for f in &s.methods {
                            self.fns.insert(f.def, info(f, Some((&s.generics, s.def))));
                            if f.self_mode.is_some() && f.generics.is_empty() {
                                let exported = f.public || f.interface.is_some() || std;
                                self.names.insert(f.def, value_name(&f.name, exported));
                            } else {
                                let base = format!("{}{}", s.name, case(&camel(&f.name), true));
                                let exported = f.public && f.interface.is_none() && s.public;
                                derived.push((f.def, index, case(&base, exported || std)));
                            }
                        }
                    }
                    ir::Item::Enum(e) => {
                        put(&mut self.names, e.def, type_name(&e.name, e.public || std));
                        self.enums.insert(e.def, e);
                        let plain = e.variants.iter().all(|v| v.payload.is_empty());
                        if plain && e.methods.iter().any(|f| f.self_mode == Some(Mode::Mut)) {
                            self.ptr_recv.insert(e.def);
                        }
                        for v in &e.variants {
                            let base = format!("{}{}", e.name, v.name);
                            derived.push((v.def, index, case(&base, e.public || std)));
                        }
                        for f in &e.methods {
                            self.fns.insert(f.def, info(f, Some((&e.generics, e.def))));
                            if plain && f.self_mode.is_some() && f.generics.is_empty() {
                                let exported = f.public || f.interface.is_some() || std;
                                self.names.insert(f.def, value_name(&f.name, exported));
                            } else {
                                let base = format!("{}{}", e.name, case(&camel(&f.name), true));
                                let exported = f.public && f.interface.is_none() && e.public;
                                derived.push((f.def, index, case(&base, exported || std)));
                            }
                        }
                    }
                    ir::Item::Interface(i) => {
                        put(&mut self.names, i.def, type_name(&i.name, i.public || std));
                        self.ifaces.insert(i.def, i);
                        for f in &i.methods {
                            self.iface_fns
                                .insert((i.def, f.name.clone()), info(f, None));
                        }
                        if std {
                            match i.name.as_str() {
                                "Comparable" => self.comparable = Some(i.def),
                                "Display" => self.display = Some(i.def),
                                "Equatable" => self.equatable = Some(i.def),
                                "Copyable" => self.copyable = Some(i.def),
                                "Hashable" => self.hashable = Some(i.def),
                                "Error" => self.error_iface = Some(i.def),
                                _ => {}
                            }
                        }
                    }
                    ir::Item::Fn(f) => {
                        put(&mut self.names, f.def, value_name(&f.name, f.public));
                        self.fns.insert(f.def, info(f, None));
                    }
                    ir::Item::Const(c) => {
                        put(&mut self.names, c.def, value_name(&c.name, c.public || std));
                    }
                    ir::Item::ExternFn(x) => {
                        put(&mut self.names, x.def, value_name(&x.name, x.public));
                        self.fns.insert(
                            x.def,
                            FnInfo {
                                params: x.params.clone(),
                                ret: x.ret.clone(),
                                generics: Vec::new(),
                                owner_generics: Vec::new(),
                                owner: None,
                                self_mode: None,
                                interface: None,
                            },
                        );
                    }
                    ir::Item::ExternType(x) => {
                        put(&mut self.names, x.def, type_name(&x.name, true));
                    }
                    ir::Item::Builtin(b) => {
                        for f in &b.methods {
                            self.fns.insert(f.def, info(f, None));
                        }
                    }
                    ir::Item::Test(_) => {}
                }
            }
        }
        self.recursion();
        for (def, index, name) in derived {
            let set = taken.entry(index).or_default();
            let mut n = name;
            while set.contains(&n) {
                n.push('_');
            }
            set.insert(n.clone());
            self.names.insert(def, n);
        }
        // Types that implement `Error`.
        if let Some(err) = self.error_iface {
            for (def, s) in &self.structs {
                if s.implements
                    .iter()
                    .any(|t| matches!(t, IrType::Named(d, _) if *d == err))
                {
                    self.error_types.insert(*def);
                }
            }
            for (def, e) in &self.enums {
                if e.implements
                    .iter()
                    .any(|t| matches!(t, IrType::Named(d, _) if *d == err))
                {
                    self.error_types.insert(*def);
                }
            }
        }
        // std lives in the runtime.
        for m in p.modules.iter().filter(|m| m.role == ModuleRole::Std) {
            for item in &m.items {
                let (def, piece): (DefRef, &'static str) = match item {
                    ir::Item::Struct(s) => match s.name.as_str() {
                        "ParseError" => (s.def, "ParseError"),
                        "Entry" => (s.def, "Entry"),
                        _ => continue,
                    },
                    ir::Item::Enum(e) => {
                        let piece = match e.name.as_str() {
                            "Ordering" => "Ordering",
                            "FsError" => "FsError",
                            _ => continue,
                        };
                        for v in &e.variants {
                            self.std_pieces.insert(v.def, piece);
                        }
                        (e.def, piece)
                    }
                    ir::Item::Interface(i) if i.name == "Error" => (i.def, "Error"),
                    ir::Item::Interface(i) if i.name == "Display" => (i.def, "Display"),
                    ir::Item::Const(c) => (c.def, "math"),
                    _ => continue,
                };
                self.std_pieces.insert(def, piece);
            }
        }
        // Names the root package of a library exports again.
        for m in p.modules.iter().filter(|m| m.role == ModuleRole::Entry) {
            for r in &m.reexports {
                let kind = p.def(r.def).map(|d| d.kind);
                let n = match kind {
                    Some(DefKind::Struct | DefKind::Enum | DefKind::Interface) => {
                        type_name(&r.name, true)
                    }
                    _ => value_name(&r.name, true),
                };
                self.reexported.insert(r.def, n);
            }
        }
    }

    // ----- names and references ---------------------------------------------------------------

    fn def_module(&self, def: DefRef) -> Option<usize> {
        self.p.def(def).map(|d| d.module)
    }

    fn pkg_path(&self, module: usize) -> Option<String> {
        self.pkgs.get(module).cloned().flatten().map(|p| p.path)
    }

    fn current_path(&self) -> String {
        self.pkg_path(self.module).unwrap_or_default()
    }

    fn name_of(&self, def: DefRef) -> String {
        self.names
            .get(&def)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// The package path that holds a declaration, or `None` for the current package.
    fn qualifier(&mut self, def: DefRef) -> Option<String> {
        if let Some(piece) = self.std_pieces.get(&def).copied() {
            self.rt.insert(piece);
            let path = self.wolfrt();
            self.file.imports.insert(path.clone());
            return Some(path);
        }
        let module = self.def_module(def)?;
        let role = self.p.modules.get(self.module).map(|m| m.role);
        if matches!(role, Some(ModuleRole::Test | ModuleRole::Example))
            && self.reexported.contains_key(&def)
        {
            let path = self.module_path.clone();
            if path != self.current_path() {
                self.file.imports.insert(path.clone());
                return Some(path);
            }
        }
        let path = self.pkg_path(module)?;
        if path == self.current_path() {
            return None;
        }
        self.file.imports.insert(path.clone());
        Some(path)
    }

    /// A reference to a top-level declaration.
    fn def_expr(&mut self, def: DefRef) -> Expr {
        let role = self.p.modules.get(self.module).map(|m| m.role);
        let name = match self.reexported.get(&def) {
            Some(n) if matches!(role, Some(ModuleRole::Test | ModuleRole::Example)) => n.clone(),
            _ => self.name_of(def),
        };
        match self.qualifier(def) {
            Some(path) => Expr::Qual(path, name),
            None => id(&name),
        }
    }

    fn def_type(&mut self, def: DefRef, args: Vec<Type>) -> Type {
        let role = self.p.modules.get(self.module).map(|m| m.role);
        let name = match self.reexported.get(&def) {
            Some(n) if matches!(role, Some(ModuleRole::Test | ModuleRole::Example)) => n.clone(),
            _ => self.name_of(def),
        };
        let q = self.qualifier(def);
        Type::Name(q, name, args)
    }

    /// A new name for a local of the current function.
    fn fresh(&mut self, base: &str) -> String {
        let mut name = base.to_string();
        let mut n = 2;
        while self.f.used.contains(&name) || self.taken.contains(&name) {
            name = format!("{base}{n}");
            n += 1;
        }
        self.f.used.insert(name.clone());
        self.file.idents.insert(name.clone());
        name
    }

    fn local_name(&mut self, local: LocalRef) -> String {
        let base = self
            .p
            .locals
            .get(&local)
            .map(|l| local_name_of(&l.name))
            .unwrap_or_else(|| "x".to_string());
        let n = self.fresh(&base);
        self.f.user.insert(n.clone());
        n
    }

    fn local_ty(&self, local: LocalRef) -> IrType {
        self.p
            .locals
            .get(&local)
            .map(|l| l.ty.clone())
            .unwrap_or(IrType::Void)
    }

    fn is_read(&self, local: LocalRef) -> bool {
        self.f.reads.contains(&local)
    }

    // ----- types ------------------------------------------------------------------------------

    fn plain_enum(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if self.enums.get(d).is_some_and(|e| e.variants.iter().all(|v| v.payload.is_empty())))
    }

    /// Whether a value is a variant of an enum with payloads: Go gives it the type of the
    /// variant's struct, so a type argument inferred from it would be that struct and not the
    /// enum's interface.
    fn holds_variant(&self, e: &IrExpr) -> bool {
        matches!(e.kind, ExprKind::Variant { .. }) && self.payload_enum(&e.ty)
    }

    fn payload_enum(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if self.enums.get(d).is_some_and(|e| !e.variants.iter().all(|v| v.payload.is_empty())))
    }

    fn is_struct(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if self.structs.contains_key(d))
    }

    /// Types that Go compares with `==` and copies by assignment.
    fn primitive(&self, t: &IrType) -> bool {
        matches!(
            t,
            IrType::Int
                | IrType::Float
                | IrType::Bool
                | IrType::String
                | IrType::Void
                | IrType::Never
        ) || self.plain_enum(t)
    }

    /// Whether `E` of a `Result<T, E>` is `Error` or implements it.
    fn error_like(&self, t: &IrType) -> bool {
        match t {
            IrType::Named(d, _) => Some(*d) == self.error_iface || self.error_types.contains(d),
            _ => false,
        }
    }

    /// The form of the result of a declared function (ADR 0002, spec chapter 8, 4.1).
    fn ret_form(&self, t: &IrType) -> Form {
        match t {
            IrType::Option(inner) => {
                if self.is_struct(inner) || self.payload_enum(inner) {
                    Form::OptPtr
                } else {
                    Form::OptPair
                }
            }
            IrType::Result(ok, e) if self.error_like(e) => {
                if matches!(**ok, IrType::Void) {
                    Form::ResErr
                } else {
                    Form::ResPair
                }
            }
            _ => Form::Data,
        }
    }

    /// A type in a data position.
    fn ty(&mut self, t: &IrType) -> Result<Type, Error> {
        Ok(match t {
            IrType::Int => int64(),
            IrType::Float => Type::name("float64"),
            IrType::Bool => Type::name("bool"),
            IrType::String => Type::name("string"),
            IrType::Void | IrType::Never => Type::EmptyStruct,
            IrType::List(inner) => Type::Slice(Box::new(self.ty(inner)?)),
            IrType::Map(k, v) => {
                let args = vec![self.ty(k)?, self.ty(v)?];
                self.rt_type("Map", "Map", args)
            }
            IrType::Option(inner) => {
                let args = vec![self.ty(inner)?];
                self.rt_type("Option", "Option", args)
            }
            IrType::Result(a, b) => {
                let args = vec![self.ty(a)?, self.ty(b)?];
                self.rt_type("Result", "Result", args)
            }
            IrType::Named(def, args) => {
                let mut list = Vec::new();
                for a in args {
                    list.push(self.ty(a)?);
                }
                self.def_type(*def, list)
            }
            IrType::Fn(params, ret) => {
                let mut ps = Vec::new();
                for (m, t) in params {
                    ps.push(self.param_ty(*m, t)?);
                }
                let rs = match ret.as_ref() {
                    IrType::Void | IrType::Never => Vec::new(),
                    other => vec![self.ty(other)?],
                };
                Type::Func(ps, rs)
            }
            IrType::Param(n) => Type::name(n),
            IrType::SelfType => match self.self_iface.clone() {
                // In a default, `Self` is the interface.
                Some(t) => return self.ty(&t),
                None => return Err("`Self` in a Go signature".to_string()),
            },
        })
    }

    fn param_ty(&mut self, mode: Mode, t: &IrType) -> Result<Type, Error> {
        let inner = self.ty(t)?;
        Ok(if mode == Mode::Mut {
            Type::Ptr(Box::new(inner))
        } else {
            inner
        })
    }

    /// The result types of a declared function.
    fn results(&mut self, t: &IrType) -> Result<Vec<Type>, Error> {
        Ok(match (self.ret_form(t), t) {
            (_, IrType::Void | IrType::Never) => Vec::new(),
            (Form::OptPair, IrType::Option(inner)) => vec![self.ty(inner)?, Type::name("bool")],
            (Form::OptPtr, IrType::Option(inner)) => vec![Type::Ptr(Box::new(self.ty(inner)?))],
            (Form::ResPair, IrType::Result(ok, _)) => vec![self.ty(ok)?, Type::name("error")],
            (Form::ResErr, _) => vec![Type::name("error")],
            _ => vec![self.ty(t)?],
        })
    }

    /// The type of a variant's struct: `*ShapeCircle`.
    fn variant_ty(&mut self, variant: DefRef, enum_ty: &IrType) -> Result<Type, Error> {
        let mut args = Vec::new();
        if let IrType::Named(_, a) = enum_ty {
            for t in a {
                args.push(self.ty(t)?);
            }
        }
        Ok(Type::Ptr(Box::new(self.def_type(variant, args))))
    }

    /// The zero value of a Go type, for returns that carry an error.
    fn zero(&mut self, t: &IrType) -> Result<Expr, Error> {
        Ok(match t {
            IrType::Int | IrType::Float => lit("0"),
            IrType::Bool => lit("false"),
            IrType::String => Expr::Str(String::new()),
            IrType::List(_) | IrType::Fn(..) => lit("nil"),
            IrType::Named(d, _) if self.ifaces.contains_key(d) || self.payload_enum(t) => {
                lit("nil")
            }
            IrType::Named(..) if self.plain_enum(t) => lit("0"),
            IrType::Param(_) => {
                let ty = self.ty(t)?;
                star(call(id("new"), vec![Expr::Type(ty)]))
            }
            _ => {
                let ty = self.ty(t)?;
                Expr::Composite(ty, Vec::new())
            }
        })
    }

    /// Type parameters with their Go constraints.
    fn tparams(&mut self, generics: &[ir::Generic]) -> Result<Vec<(String, Type)>, Error> {
        let mut out = Vec::new();
        for g in generics {
            let mut parts = Vec::new();
            for b in &g.bounds {
                let IrType::Named(def, _) = b else { continue };
                if Some(*def) == self.hashable {
                    parts.push(Type::name("comparable"));
                } else if [self.equatable, self.copyable, self.comparable, self.display]
                    .contains(&Some(*def))
                {
                    continue;
                } else {
                    parts.push(self.ty(b)?);
                }
            }
            let c = match parts.len() {
                0 => Type::name("any"),
                1 => parts.pop().unwrap_or_else(|| Type::name("any")),
                _ => Type::Union(parts),
            };
            out.push((g.name.clone(), c));
        }
        Ok(out)
    }

    /// The dictionary parameters of generics: `eqT func(T, T) bool` and so on.
    fn dict_params(&mut self, generics: &[ir::Generic]) -> Result<Vec<(String, Type)>, Error> {
        let mut out = Vec::new();
        for g in generics {
            for d in self.dicts_of(&g.bounds) {
                let t = IrType::Param(g.name.clone());
                let ty = self.dict_ty(d, &t)?;
                let name = self.fresh(&format!(
                    "{}{}",
                    match d {
                        Dict::Eq => "eq",
                        Dict::Copy => "copy",
                        Dict::Cmp => "cmp",
                        Dict::Show => "show",
                    },
                    g.name
                ));
                self.f.dicts.insert((g.name.clone(), d), name.clone());
                out.push((name, ty));
            }
        }
        Ok(out)
    }

    fn dicts_of(&self, bounds: &[IrType]) -> Vec<Dict> {
        let mut out = Vec::new();
        for b in bounds {
            let IrType::Named(def, _) = b else { continue };
            if Some(*def) == self.equatable {
                out.push(Dict::Eq);
            } else if Some(*def) == self.copyable {
                out.push(Dict::Copy);
            } else if Some(*def) == self.comparable {
                out.push(Dict::Cmp);
            } else if Some(*def) == self.display {
                out.push(Dict::Show);
            }
        }
        out
    }

    fn dict_ty(&mut self, d: Dict, t: &IrType) -> Result<Type, Error> {
        let g = self.ty(t)?;
        Ok(match d {
            Dict::Eq => Type::Func(vec![g.clone(), g], vec![Type::name("bool")]),
            Dict::Copy => Type::Func(vec![g.clone()], vec![g]),
            Dict::Cmp => {
                let o = self.rt_type("Ordering", "Ordering", Vec::new());
                Type::Func(vec![g.clone(), g], vec![o])
            }
            Dict::Show => Type::Func(vec![g], vec![Type::name("string")]),
        })
    }

    /// The dictionary for a bound and a type.
    fn dict(&mut self, d: Dict, t: &IrType) -> Result<Expr, Error> {
        match d {
            Dict::Eq => self.eq_fn(t),
            Dict::Copy => self.copy_fn(t),
            Dict::Cmp => self.cmp_fn(t),
            Dict::Show => self.show_fn(t),
        }
    }

    /// A function `func(x T) string` that is `to_string` of `Display` for `t`.
    fn show_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        match t {
            IrType::List(_) | IrType::Map(..) | IrType::Option(_) | IrType::Result(..) => {
                let ty = self.ty(t)?;
                let text = self.text_expr(id("x"), t)?;
                Ok(Expr::FuncLit(
                    vec![("x".to_string(), ty)],
                    vec![Type::name("string")],
                    vec![Stmt::Return(vec![text])],
                ))
            }
            IrType::Int => Ok(self.rt("Display", "IntString")),
            IrType::Float => Ok(self.rt("FloatString", "FloatString")),
            IrType::Bool => Ok(Expr::Qual("strconv".to_string(), "FormatBool".to_string()))
                .inspect(|_| {
                    self.file.imports.insert("strconv".to_string());
                }),
            IrType::String => Ok(self.rt("Display", "StringString")),
            IrType::Param(n) => self
                .f
                .dicts
                .get(&(n.clone(), Dict::Show))
                .map(|d| id(d))
                .ok_or_else(|| format!("to_string on `{n}` without `Display`")),
            IrType::Named(def, args) if args.is_empty() && !self.ptr_recv.contains(def) => {
                let ty = self.ty(t)?;
                Ok(sel(Expr::Type(ty), "ToString"))
            }
            _ => {
                let ty = self.ty(t)?;
                Ok(Expr::FuncLit(
                    vec![("x".to_string(), ty)],
                    vec![Type::name("string")],
                    vec![Stmt::Return(vec![call(
                        sel(id("x"), "ToString"),
                        Vec::new(),
                    )])],
                ))
            }
        }
    }

    /// The text of a list, a map, an option or a result `x` of type `t`, composed from the text
    /// of its parts (spec chapter 7, section 4.1).
    fn text_expr(&mut self, x: Expr, t: &IrType) -> Result<Expr, Error> {
        let (name, parts) = match t {
            IrType::List(e) => ("ListText", vec![self.part_fn(e)?]),
            IrType::Map(k, v) => ("MapText", vec![self.part_fn(k)?, self.part_fn(v)?]),
            IrType::Option(e) => ("OptionText", vec![self.part_fn(e)?]),
            IrType::Result(v, e) => ("ResultText", vec![self.part_fn(v)?, self.err_fn(e)?]),
            other => return Err(format!("the text of {other:?} in Go")),
        };
        let mut args = vec![x];
        args.extend(parts);
        Ok(self.rt_call("Text", name, args))
    }

    /// The text of a part of a collection: a `String` in quotes; in generic code the runtime
    /// tells a `String` from other types.
    fn part_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        match t {
            IrType::String => Ok(self.rt("Text", "Quote")),
            IrType::Param(_) => {
                let show = self.show_fn(t)?;
                Ok(self.rt_call("Text", "Part", vec![show]))
            }
            _ => self.show_fn(t),
        }
    }

    /// The text of the error of a `Result`: by `to_string` if its type is `Display`, else by
    /// `message()`.
    fn err_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        match t {
            IrType::Param(n) if self.f.dicts.contains_key(&(n.clone(), Dict::Show)) => {
                self.part_fn(t)
            }
            IrType::Param(_) => {
                let ty = self.ty(t)?;
                let f = self.rt("Text", "ErrText");
                Ok(Expr::Inst(Box::new(f), vec![ty]))
            }
            IrType::Named(def, _) if !self.is_display(*def) => {
                let ty = self.ty(t)?;
                Ok(Expr::FuncLit(
                    vec![("e".to_string(), ty)],
                    vec![Type::name("string")],
                    vec![Stmt::Return(vec![call(
                        sel(id("e"), "Message"),
                        Vec::new(),
                    )])],
                ))
            }
            _ => self.part_fn(t),
        }
    }

    /// Whether a struct or an enum implements `Display`; an interface value is `Display` only
    /// if it is that interface.
    fn is_display(&self, def: DefRef) -> bool {
        let list = self
            .structs
            .get(&def)
            .map(|s| &s.implements)
            .or_else(|| self.enums.get(&def).map(|e| &e.implements));
        match list {
            Some(l) => l
                .iter()
                .any(|t| matches!(t, IrType::Named(i, _) if Some(*i) == self.display)),
            None => Some(def) == self.display,
        }
    }

    // ----- equality, copies and comparison ----------------------------------------------------

    /// `a == b` for values of type `t`.
    fn eq_expr(&mut self, a: Expr, b: Expr, t: &IrType) -> Result<Expr, Error> {
        if self.primitive(t) {
            return Ok(bin(a, "==", b));
        }
        Ok(match t {
            IrType::List(inner) if self.primitive(inner) => {
                self.std_call("slices", "Equal", vec![a, b])
            }
            IrType::List(inner) => {
                let f = self.eq_fn(inner)?;
                self.std_call("slices", "EqualFunc", vec![a, b, f])
            }
            IrType::Option(inner) => {
                let f = self.eq_fn(inner)?;
                self.rt_call("Option", "OptionEqual", vec![a, b, f])
            }
            IrType::Result(x, y) => {
                let f = self.eq_fn(x)?;
                let g = self.eq_fn(y)?;
                self.rt_call("Result", "ResultEqual", vec![a, b, f, g])
            }
            IrType::Map(_, v) => {
                let f = self.eq_fn(v)?;
                self.rt_call("Map", "MapEqual", vec![a, b, f])
            }
            IrType::Param(n) => {
                let d = self
                    .f
                    .dicts
                    .get(&(n.clone(), Dict::Eq))
                    .cloned()
                    .ok_or_else(|| format!("== on `{n}` without `Equatable`"))?;
                call(id(&d), vec![a, b])
            }
            IrType::Named(def, args) => {
                if !self.std_pieces.contains_key(def) {
                    self.need_eq.insert(*def);
                }
                let mut dicts = Vec::new();
                for x in args {
                    dicts.push(self.eq_fn(x)?);
                }
                let mut all = vec![b];
                all.extend(dicts);
                call(sel(a, "Equal"), all)
            }
            other => return Err(format!("== on {other:?} in Go")),
        })
    }

    /// A function `func(a, b T) bool` that is `==` for `t`.
    fn eq_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        if self.primitive(t) {
            let ty = self.ty(t)?;
            let f = self.rt("Eq", "Eq");
            return Ok(Expr::Inst(Box::new(f), vec![ty]));
        }
        match t {
            IrType::Param(n) => self
                .f
                .dicts
                .get(&(n.clone(), Dict::Eq))
                .map(|d| id(d))
                .ok_or_else(|| format!("== on `{n}` without `Equatable`")),
            IrType::Named(def, args) if args.is_empty() => {
                if !self.std_pieces.contains_key(def) {
                    self.need_eq.insert(*def);
                }
                let ty = self.ty(t)?;
                Ok(sel(Expr::Type(ty), "Equal"))
            }
            _ => {
                let ty = self.ty(t)?;
                let body = self.eq_expr(id("a"), id("b"), t)?;
                Ok(Expr::FuncLit(
                    vec![("a".to_string(), ty.clone()), ("b".to_string(), ty)],
                    vec![Type::name("bool")],
                    vec![Stmt::Return(vec![body])],
                ))
            }
        }
    }

    /// `.copy()` of a value of type `t`.
    fn copy_expr(&mut self, x: Expr, t: &IrType) -> Result<Expr, Error> {
        if self.primitive(t) {
            return Ok(x);
        }
        Ok(match t {
            IrType::List(inner) if self.primitive(inner) => {
                self.std_call("slices", "Clone", vec![x])
            }
            IrType::List(inner) => {
                let f = self.copy_fn(inner)?;
                self.rt_call("ListEqual", "CopyList", vec![x, f])
            }
            IrType::Option(inner) => {
                let f = self.copy_fn(inner)?;
                self.rt_call("Option", "CopyOption", vec![x, f])
            }
            IrType::Result(a, b) => {
                let f = self.copy_fn(a)?;
                let g = self.copy_fn(b)?;
                self.rt_call("Result", "CopyResult", vec![x, f, g])
            }
            IrType::Map(_, v) => {
                let f = self.copy_fn(v)?;
                call(sel(x, "Copy"), vec![f])
            }
            IrType::Param(n) => {
                let d = self
                    .f
                    .dicts
                    .get(&(n.clone(), Dict::Copy))
                    .cloned()
                    .ok_or_else(|| format!(".copy() on `{n}` without `Copyable`"))?;
                call(id(&d), vec![x])
            }
            IrType::Named(def, args) if !self.ifaces.contains_key(def) => {
                if !self.std_pieces.contains_key(def) {
                    self.need_copy.insert(*def);
                }
                let mut dicts = Vec::new();
                for a in args {
                    dicts.push(self.copy_fn(a)?);
                }
                call(sel(x, "Copy"), dicts)
            }
            IrType::Named(def, args) if args.is_empty() => {
                self.need_copy_iface.insert(*def);
                let m = self.iface_copy_name(*def);
                call(sel(x, &m), Vec::new())
            }
            other => return Err(format!(".copy() of {other:?} in Go")),
        })
    }

    fn copy_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        if self.primitive(t) {
            let ty = self.ty(t)?;
            let f = self.rt("Eq", "Same");
            return Ok(Expr::Inst(Box::new(f), vec![ty]));
        }
        match t {
            IrType::Param(n) => self
                .f
                .dicts
                .get(&(n.clone(), Dict::Copy))
                .map(|d| id(d))
                .ok_or_else(|| format!(".copy() on `{n}` without `Copyable`")),
            IrType::Named(def, args) if args.is_empty() && !self.ifaces.contains_key(def) => {
                if !self.std_pieces.contains_key(def) {
                    self.need_copy.insert(*def);
                }
                let ty = self.ty(t)?;
                Ok(sel(Expr::Type(ty), "Copy"))
            }
            IrType::Named(def, args) if args.is_empty() => {
                // A method expression of the interface: `Animal.CopyAnimal`.
                self.need_copy_iface.insert(*def);
                let m = self.iface_copy_name(*def);
                let ty = self.ty(t)?;
                Ok(sel(Expr::Type(ty), &m))
            }
            _ => {
                let ty = self.ty(t)?;
                let body = self.copy_expr(id("x"), t)?;
                Ok(Expr::FuncLit(
                    vec![("x".to_string(), ty.clone())],
                    vec![ty],
                    vec![Stmt::Return(vec![body])],
                ))
            }
        }
    }

    /// A function `func(a, b T) wolfrt.Ordering` that is `compare` for `t`.
    fn cmp_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        match t {
            IrType::Int => Ok(self.rt("Ordering", "CompareInt")),
            IrType::String => Ok(self.rt("Ordering", "CompareString")),
            IrType::Param(n) => self
                .f
                .dicts
                .get(&(n.clone(), Dict::Cmp))
                .map(|d| id(d))
                .ok_or_else(|| format!("compare on `{n}` without `Comparable`")),
            IrType::Named(def, args) if args.is_empty() && !self.ptr_recv.contains(def) => {
                let ty = self.ty(t)?;
                Ok(sel(Expr::Type(ty), "Compare"))
            }
            _ => {
                let ty = self.ty(t)?;
                let o = self.rt_type("Ordering", "Ordering", Vec::new());
                Ok(Expr::FuncLit(
                    vec![("a".to_string(), ty.clone()), ("b".to_string(), ty)],
                    vec![o],
                    vec![Stmt::Return(vec![call(
                        sel(id("a"), "Compare"),
                        vec![id("b")],
                    )])],
                ))
            }
        }
    }

    // ----- modules ----------------------------------------------------------------------------

    /// Emits one module: its file and the file of its tests, if any.
    pub(crate) fn module(&mut self, index: usize) -> Result<Vec<(String, String)>, Error> {
        let p = self.p;
        let Some(m) = p.modules.get(index) else {
            return Ok(Vec::new());
        };
        let Some(pkg) = self.pkgs.get(index).cloned().flatten() else {
            return Ok(Vec::new());
        };
        self.module = index;
        self.taken = self
            .names
            .iter()
            .filter(|(d, _)| self.def_module(**d) == Some(index))
            .map(|(_, n)| n.clone())
            .collect();
        let mut out = Vec::new();
        // The module's own file.
        if m.role != ModuleRole::Test {
            self.file = FileCtx::default();
            let mut decls = Vec::new();
            for item in &m.items {
                self.item(m, item, &mut decls)?;
            }
            self.reexports(m, &mut decls)?;
            let is_main = matches!(m.role, ModuleRole::Entry | ModuleRole::Example)
                && (m.role == ModuleRole::Example || self.p.kind == ProgramKind::Cli)
                && m.items
                    .iter()
                    .any(|it| matches!(it, ir::Item::Fn(f) if f.name == "main"));
            if is_main {
                let result = m.items.iter().any(|it| {
                    matches!(it, ir::Item::Fn(f) if f.name == "main" && matches!(f.ret, IrType::Result(..)))
                });
                let takes_args = m.items.iter().any(
                    |it| matches!(it, ir::Item::Fn(f) if f.name == "main" && !f.params.is_empty()),
                );
                let runner = match (result, takes_args) {
                    (true, true) => "RunMainArgsResult",
                    (true, false) => "RunMainResult",
                    (false, true) => "RunMainArgs",
                    (false, false) => "RunMain",
                };
                let run = self.rt_call(runner, runner, vec![id("main_")]);
                decls.push(Decl::Func(Func {
                    docs: Vec::new(),
                    recv: None,
                    name: "main".to_string(),
                    tparams: Vec::new(),
                    params: Vec::new(),
                    results: Vec::new(),
                    body: vec![Stmt::Expr(run)],
                }));
            }
            let text = self.file_text(&m.file, m.docs.clone(), &pkg.name, &pkg.path, decls);
            out.push((pkg.file.clone(), text));
        }
        // Tests and test helpers.
        let tests: Vec<&ir::Test> = m
            .items
            .iter()
            .filter_map(|it| match it {
                ir::Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        let helpers = m
            .items
            .iter()
            .any(|it| matches!(it, ir::Item::Fn(f) if f.in_tests));
        if self.tests && (!tests.is_empty() || helpers || m.role == ModuleRole::Test) {
            self.file = FileCtx::default();
            let mut decls = Vec::new();
            for item in &m.items {
                match item {
                    ir::Item::Fn(f) if f.in_tests || m.role == ModuleRole::Test => {
                        self.function_item(m, f, &mut decls)?;
                    }
                    ir::Item::Test(t) => decls.push(self.test_fn(m, t, &pkg)?),
                    _ if m.role == ModuleRole::Test => self.item(m, item, &mut decls)?,
                    _ => {}
                }
            }
            let text = self.file_text(&m.file, Vec::new(), &pkg.test_package, &pkg.path, decls);
            out.push((pkg.test_file.clone(), text));
        }
        Ok(out)
    }

    /// Prints a file, choosing import names that no name of the file hides.
    fn file_text(
        &mut self,
        source: &str,
        docs: Vec<String>,
        package: &str,
        path: &str,
        decls: Vec<Decl>,
    ) -> String {
        let ctx = std::mem::take(&mut self.file);
        let mut names: BTreeSet<String> = ctx.idents.clone();
        names.extend(self.taken.iter().cloned());
        let mut imports = BTreeMap::new();
        for import in &ctx.imports {
            if import == path {
                continue;
            }
            let default = self.package_name(import);
            let mut name = default.clone();
            let mut n = 1;
            while names.contains(&name) {
                name = if n == 1 {
                    format!("{default}pkg")
                } else {
                    format!("{default}pkg{n}")
                };
                n += 1;
            }
            names.insert(name.clone());
            imports.insert(import.clone(), name);
        }
        let docs = docs
            .iter()
            .flat_map(|d| {
                d.lines()
                    .map(|l| l.trim_end().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        go::print(&go::File {
            header: format!("Generated by wolf from {source}. Do not edit."),
            docs,
            package: package.to_string(),
            path: path.to_string(),
            imports,
            native: format!("{}/native/", self.module_path),
            decls,
        })
    }

    /// The package clause of an import path.
    fn package_name(&self, path: &str) -> String {
        for pkg in self.pkgs.iter().flatten() {
            if pkg.path == path {
                return pkg.name.clone();
            }
        }
        if path == self.module_path {
            return self.root_pkg.clone();
        }
        path.rsplit('/').next().unwrap_or(path).to_string()
    }

    /// Type aliases and forwarding functions for the re-exports of `src/lib.wolf`.
    fn reexports(&mut self, m: &'p ir::Module, out: &mut Vec<Decl>) -> Result<(), Error> {
        if m.role != ModuleRole::Entry {
            return Ok(());
        }
        for r in &m.reexports {
            let name = self.reexported.get(&r.def).cloned().unwrap_or_default();
            let Some(d) = self.p.def(r.def).cloned() else {
                continue;
            };
            match d.kind {
                DefKind::Struct | DefKind::Enum | DefKind::Interface => {
                    let generics = self
                        .structs
                        .get(&r.def)
                        .map(|s| s.generics.clone())
                        .or_else(|| self.enums.get(&r.def).map(|e| e.generics.clone()))
                        .or_else(|| self.ifaces.get(&r.def).map(|i| i.generics.clone()))
                        .unwrap_or_default();
                    let tparams = self.tparams(&generics)?;
                    let args = generics.iter().map(|g| Type::name(&g.name)).collect();
                    let ty = self.def_type(r.def, args);
                    out.push(Decl::TypeDef {
                        docs: vec![format!("{name} is {}.", self.name_of(r.def))],
                        name,
                        tparams,
                        ty,
                        alias: true,
                    });
                }
                DefKind::Fn => {
                    let info = self
                        .fns
                        .get(&r.def)
                        .cloned()
                        .ok_or("re-exported function")?;
                    self.f = FnState::default();
                    let tparams = self.tparams(&info.generics)?;
                    let mut params = self.dict_params(&info.generics)?;
                    let mut args: Vec<Expr> = params.iter().map(|(n, _)| id(n)).collect();
                    for (i, (mode, t)) in info.params.iter().enumerate() {
                        let n = format!("p{i}");
                        params.push((n.clone(), self.param_ty(*mode, t)?));
                        args.push(id(&n));
                    }
                    let results = self.results(&info.ret)?;
                    let f = self.def_expr(r.def);
                    let targs = info.generics.iter().map(|g| Type::name(&g.name)).collect();
                    let c = callt(f, targs, args);
                    let body = if results.is_empty() {
                        vec![Stmt::Expr(c)]
                    } else {
                        vec![Stmt::Return(vec![c])]
                    };
                    self.f = FnState::default();
                    out.push(Decl::Func(Func {
                        docs: vec![format!("{name} is {}.", self.name_of(r.def))],
                        recv: None,
                        name,
                        tparams,
                        params,
                        results,
                        body,
                    }));
                }
                DefKind::Const => {
                    let value = self.def_expr(r.def);
                    out.push(Decl::Consts(vec![(Vec::new(), name, None, Some(value))]));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn item(
        &mut self,
        m: &'p ir::Module,
        item: &'p ir::Item,
        out: &mut Vec<Decl>,
    ) -> Result<(), Error> {
        match item {
            ir::Item::Struct(s) => self.struct_item(s, out),
            ir::Item::Enum(e) => self.enum_item(e, out),
            ir::Item::Interface(i) => self.interface_item(i, out),
            ir::Item::Fn(f) if f.in_tests => Ok(()),
            ir::Item::Fn(f) => self.function_item(m, f, out),
            ir::Item::Const(c) => {
                let ty = self.ty(&c.ty)?;
                let value = match &c.value {
                    ir::ConstValue::Int(v) => lit(&v.to_string()),
                    ir::ConstValue::Float(v) => float_lit(*v)?,
                    ir::ConstValue::Bool(v) => lit(if *v { "true" } else { "false" }),
                    ir::ConstValue::Str(s) => Expr::Str(s.clone()),
                };
                let name = self.name_of(c.def);
                out.push(Decl::Consts(vec![(
                    c.docs.clone(),
                    name,
                    Some(ty),
                    Some(value),
                )]));
                Ok(())
            }
            ir::Item::ExternFn(x) => self.extern_fn(x, out),
            ir::Item::ExternType(x) => self.extern_type(x, out),
            ir::Item::Test(_) | ir::Item::Builtin(_) => Ok(()),
        }
    }

    fn function_item(
        &mut self,
        _m: &'p ir::Module,
        f: &'p ir::Function,
        out: &mut Vec<Decl>,
    ) -> Result<(), Error> {
        if f.body.is_none() {
            return Ok(());
        }
        let name = self.name_of(f.def);
        let func = self.function(f, name, None, &[], None)?;
        out.push(Decl::Func(func));
        Ok(())
    }

    /// A test function for `go test`.
    fn test_fn(&mut self, m: &'p ir::Module, t: &'p ir::Test, pkg: &Pkg) -> Result<Decl, Error> {
        let mut name = test_name(&t.name);
        while self.test_names.contains(&name) {
            name.push('_');
        }
        self.test_names.insert(name.clone());
        // `go test` names an external test package by its directory.
        let package = if m.role == ModuleRole::Test {
            self.module_path.clone()
        } else {
            pkg.path.clone()
        };
        self.test_list.push(TestEntry {
            package,
            go_name: name.clone(),
            name: format!("{}: {}", m.file, t.name),
        });
        self.f = FnState::default();
        let mut scan = Scan::default();
        scan.block(&t.body);
        self.f.reads = scan.reads;
        self.f.assigned = scan.assigned;
        self.f.used.insert("t".to_string());
        self.f.rets.push((IrType::Void, Form::Data));
        let mut body = Vec::new();
        self.block(&t.body, Target::Discard, &mut body)?;
        self.f = FnState::default();
        self.file.imports.insert("testing".to_string());
        let run = self.rt_call(
            "Test",
            "Test",
            vec![id("t"), Expr::FuncLit(Vec::new(), Vec::new(), body)],
        );
        Ok(Decl::Func(Func {
            docs: Vec::new(),
            recv: None,
            name,
            tparams: Vec::new(),
            params: vec![(
                "t".to_string(),
                Type::Ptr(Box::new(Type::Name(
                    Some("testing".to_string()),
                    "T".to_string(),
                    Vec::new(),
                ))),
            )],
            results: Vec::new(),
            body: vec![Stmt::Expr(run)],
        }))
    }

    // ----- types and their methods ------------------------------------------------------------

    /// The method that copies the value behind an interface: `CopyAnimal`.
    fn iface_copy_name(&self, iface: DefRef) -> String {
        let name = self
            .p
            .def(iface)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        format!("Copy{}", case(&camel(&name), true))
    }

    /// `CopyAnimal` methods of a type for the copied interfaces it implements, by `Copy`.
    fn iface_copies(
        &mut self,
        def: DefRef,
        methods: &[ir::Function],
        generic: bool,
        recv_ty: &Type,
        plain: bool,
        out: &mut Vec<Decl>,
    ) -> Result<(), Error> {
        let ifaces: BTreeSet<DefRef> = methods.iter().filter_map(|f| f.interface).collect();
        for iface in ifaces {
            if !self.need_copy_iface.contains(&iface) {
                continue;
            }
            if generic {
                return Err("a generic type as a copied interface value".to_string());
            }
            // A plain enum is an integer: the value is its own copy.
            let copy = if plain && self.ptr_recv.contains(&def) {
                star(id("a"))
            } else if plain {
                id("a")
            } else {
                self.need_copy.insert(def);
                call(sel(id("a"), "Copy"), Vec::new())
            };
            let name = self.iface_copy_name(iface);
            let ity = self.ty(&IrType::Named(iface, Vec::new()))?;
            let ptr = self.ptr_recv.contains(&def);
            let (recv, body) = if ptr {
                (
                    Type::Ptr(Box::new(recv_ty.clone())),
                    vec![
                        Stmt::Define(vec!["c".to_string()], vec![copy]),
                        Stmt::Return(vec![addr(id("c"))]),
                    ],
                )
            } else {
                (recv_ty.clone(), vec![Stmt::Return(vec![copy])])
            };
            out.push(Decl::Func(Func {
                docs: vec![format!("{name} is .copy() behind the interface.")],
                recv: Some(("a".to_string(), recv)),
                name,
                tparams: Vec::new(),
                params: Vec::new(),
                results: vec![ity],
                body,
            }));
        }
        Ok(())
    }

    /// Structs a type holds by value: itself as a struct, or inside `Option` and `Result`.
    /// Enums with payloads and lists are already indirect in Go.
    fn held(&self, t: &IrType, out: &mut Vec<DefRef>) {
        match t {
            IrType::Named(d, _) if self.structs.contains_key(d) => out.push(*d),
            IrType::Option(a) => self.held(a, out),
            IrType::Result(a, b) => {
                self.held(a, out);
                self.held(b, out);
            }
            _ => {}
        }
    }

    /// Finds the fields that make a struct contain itself (spec chapter 4, section 7.4).
    fn recursion(&mut self) {
        let defs: Vec<DefRef> = self.structs.keys().copied().collect();
        let mut reach: BTreeMap<DefRef, BTreeSet<DefRef>> = BTreeMap::new();
        for d in &defs {
            let mut seen = BTreeSet::new();
            let mut stack = vec![*d];
            while let Some(x) = stack.pop() {
                let Some(s) = self.structs.get(&x) else {
                    continue;
                };
                for f in &s.fields {
                    let mut held = Vec::new();
                    self.held(&f.ty, &mut held);
                    for y in held {
                        if seen.insert(y) {
                            stack.push(y);
                        }
                    }
                }
            }
            reach.insert(*d, seen);
        }
        for d in &defs {
            let Some(s) = self.structs.get(d) else {
                continue;
            };
            for (i, f) in s.fields.iter().enumerate() {
                if matches!(f.ty, IrType::Named(..)) {
                    continue;
                }
                let mut held = Vec::new();
                self.held(&f.ty, &mut held);
                if held
                    .iter()
                    .any(|t| t == d || reach.get(t).is_some_and(|r| r.contains(d)))
                {
                    self.boxed.insert((*d, i));
                }
            }
        }
    }

    /// Whether the field `name` of a struct of type `base` holds a pointer.
    fn field_boxed(&self, base: &IrType, name: &str) -> bool {
        let IrType::Named(d, _) = base else {
            return false;
        };
        self.structs
            .get(d)
            .and_then(|s| s.fields.iter().position(|f| f.name == name))
            .is_some_and(|i| self.boxed.contains(&(*d, i)))
    }

    fn struct_item(&mut self, s: &'p ir::Struct, out: &mut Vec<Decl>) -> Result<(), Error> {
        let name = self.name_of(s.def);
        let tparams = self.tparams(&s.generics)?;
        let mut fields = Vec::new();
        for (i, f) in s.fields.iter().enumerate() {
            let mut ty = self.ty(&f.ty)?;
            if self.boxed.contains(&(s.def, i)) {
                ty = Type::Ptr(Box::new(ty));
            }
            fields.push(Field {
                docs: f.docs.clone(),
                name: value_name(&f.name, f.public),
                ty,
            });
        }
        out.push(Decl::Struct {
            docs: s.docs.clone(),
            name: name.clone(),
            tparams,
            fields,
        });
        let args: Vec<IrType> = s
            .generics
            .iter()
            .map(|g| IrType::Param(g.name.clone()))
            .collect();
        let self_ty = IrType::Named(s.def, args);
        let recv_ty = self.ty(&self_ty)?;
        let ptr = self.ptr_recv.contains(&s.def);
        let recv = self.receiver_name(&s.name, &s.methods);
        for f in &s.methods {
            if f.body.is_none() {
                continue;
            }
            let fname = self.name_of(f.def);
            let method = f.self_mode.is_some() && f.generics.is_empty();
            let func = if method {
                let ty = if ptr {
                    Type::Ptr(Box::new(recv_ty.clone()))
                } else {
                    recv_ty.clone()
                };
                let local = if ptr {
                    Local::Ptr(recv.clone())
                } else {
                    Local::Val(recv.clone())
                };
                self.function(f, fname, Some((recv.clone(), ty, local)), &[], None)?
            } else {
                // Associated functions and generic methods are functions of the package.
                self.function(f, fname, None, &s.generics, Some(&self_ty))?
            };
            out.push(Decl::Func(func));
        }
        let rty = if ptr {
            Type::Ptr(Box::new(recv_ty.clone()))
        } else {
            recv_ty.clone()
        };
        for (iface, args, f) in self.inherited(&s.implements, &s.methods) {
            let func = self.delegate((recv.clone(), rty.clone()), &self_ty, iface, &args, f)?;
            out.push(Decl::Func(func));
        }
        if self.error_types.contains(&s.def) {
            out.push(self.error_method(recv_ty.clone(), &s.generics));
        }
        self.iface_copies(
            s.def,
            &s.methods,
            !s.generics.is_empty(),
            &recv_ty,
            false,
            out,
        )?;
        if self.need_eq.contains(&s.def) {
            let mut conds = Vec::new();
            self.f = FnState::default();
            let dict_params = self.type_dicts(&s.generics, Dict::Eq)?;
            for (i, f) in s.fields.iter().enumerate() {
                let n = value_name(&f.name, f.public);
                let (mut a, mut b) = (sel(id("a"), &n), sel(id("b"), &n));
                if self.boxed.contains(&(s.def, i)) {
                    (a, b) = (star(a), star(b));
                }
                let c = self.eq_expr(a, b, &f.ty)?;
                conds.push(c);
            }
            self.f = FnState::default();
            let mut params = vec![("b".to_string(), recv_ty.clone())];
            params.extend(dict_params);
            out.push(Decl::Func(Func {
                docs: vec!["Equal is ==.".to_string()],
                recv: Some(("a".to_string(), recv_ty.clone())),
                name: "Equal".to_string(),
                tparams: Vec::new(),
                params,
                results: vec![Type::name("bool")],
                body: vec![Stmt::Return(vec![
                    and_all(conds).unwrap_or_else(|| lit("true")),
                ])],
            }));
        }
        if self.need_copy.contains(&s.def) {
            self.f = FnState::default();
            let dict_params = self.type_dicts(&s.generics, Dict::Copy)?;
            let mut elems = Vec::new();
            let mut all_plain = true;
            for (i, f) in s.fields.iter().enumerate() {
                let n = value_name(&f.name, f.public);
                let boxed = self.boxed.contains(&(s.def, i));
                let mut c = if boxed {
                    self.copy_expr(star(sel(id("a"), &n)), &f.ty)?
                } else {
                    self.copy_expr(sel(id("a"), &n), &f.ty)?
                };
                if boxed {
                    c = self.ptr_to(c);
                }
                if boxed || !self.primitive(&f.ty) {
                    all_plain = false;
                }
                elems.push((Some(n), c));
            }
            self.f = FnState::default();
            let value = if all_plain {
                id("a")
            } else {
                Expr::Composite(recv_ty.clone(), elems)
            };
            out.push(Decl::Func(Func {
                docs: vec!["Copy is .copy().".to_string()],
                recv: Some(("a".to_string(), recv_ty.clone())),
                name: "Copy".to_string(),
                tparams: Vec::new(),
                params: dict_params,
                results: vec![recv_ty],
                body: vec![Stmt::Return(vec![value])],
            }));
        }
        Ok(())
    }

    /// Dictionary parameters of the generated `Equal` and `Copy` of a generic type: one per type
    /// parameter.
    fn type_dicts(
        &mut self,
        generics: &[ir::Generic],
        d: Dict,
    ) -> Result<Vec<(String, Type)>, Error> {
        let mut out = Vec::new();
        for g in generics {
            let t = IrType::Param(g.name.clone());
            let ty = self.dict_ty(d, &t)?;
            let name = format!("{}{}", if d == Dict::Eq { "eq" } else { "copy" }, g.name);
            self.f.dicts.insert((g.name.clone(), d), name.clone());
            out.push((name, ty));
        }
        Ok(out)
    }

    /// `Error() string` from `message()`, for types that implement `Error`.
    fn error_method(&mut self, recv_ty: Type, _generics: &[ir::Generic]) -> Decl {
        Decl::Func(Func {
            docs: Vec::new(),
            recv: Some(("e".to_string(), recv_ty)),
            name: "Error".to_string(),
            tparams: Vec::new(),
            params: Vec::new(),
            results: vec![Type::name("string")],
            body: vec![Stmt::Return(vec![call(
                sel(id("e"), "Message"),
                Vec::new(),
            )])],
        })
    }

    fn interface_item(&mut self, i: &'p ir::Interface, out: &mut Vec<Decl>) -> Result<(), Error> {
        if self.std_pieces.contains_key(&i.def)
            || [
                self.equatable,
                self.copyable,
                self.comparable,
                self.hashable,
            ]
            .contains(&Some(i.def))
            || self
                .def_module(i.def)
                .and_then(|m| self.p.modules.get(m))
                .is_some_and(|m| m.role == ModuleRole::Std)
        {
            return Ok(());
        }
        let tparams = self.tparams(&i.generics)?;
        let mut elems = Vec::new();
        for f in &i.methods {
            let mut params = Vec::new();
            for p in &f.params {
                params.push((local_name_of(&p.name), self.param_ty(p.mode, &p.ty)?));
            }
            let results = self.results(&f.ret)?;
            elems.push(IfaceElem::Method(
                f.docs.clone(),
                value_name(&f.name, true),
                params,
                results,
            ));
        }
        if self.need_copy_iface.contains(&i.def) {
            let ity = self.ty(&IrType::Named(i.def, Vec::new()))?;
            elems.push(IfaceElem::Method(
                vec!["A copy of the value (`.copy()`).".to_string()],
                self.iface_copy_name(i.def),
                Vec::new(),
                vec![ity],
            ));
        }
        out.push(Decl::Interface {
            docs: i.docs.clone(),
            name: self.name_of(i.def),
            tparams,
            elems,
        });
        // Each default is a function of the package that takes the interface value; a type that
        // gets the default has a method that calls it (spec chapter 4, section 8.2, rule 4).
        let args: Vec<IrType> = i
            .generics
            .iter()
            .map(|g| IrType::Param(g.name.clone()))
            .collect();
        let self_ty = IrType::Named(i.def, args);
        for f in i.methods.iter().filter(|f| f.body.is_some()) {
            let name = self.default_fn_name(i.def, &f.name);
            let outer = self.self_iface.replace(self_ty.clone());
            let func = self.function(f, name, None, &i.generics, Some(&self_ty));
            self.self_iface = outer;
            let mut func = func?;
            func.docs = f.docs.clone();
            out.push(Decl::Func(func));
        }
        Ok(())
    }

    /// The function that holds the default of an interface method: `NamedGreetingDefault`.
    fn default_fn_name(&self, iface: DefRef, name: &str) -> String {
        format!("{}{}Default", self.name_of(iface), value_name(name, true))
    }

    /// The defaults of the interfaces in `implements` that `own` does not replace: the
    /// interface, its type arguments and the default.
    fn inherited(
        &self,
        implements: &[IrType],
        own: &[ir::Function],
    ) -> Vec<(DefRef, Vec<IrType>, &'p ir::Function)> {
        let mut out = Vec::new();
        for t in implements {
            let IrType::Named(i, args) = t else {
                continue;
            };
            let Some(iface) = self.ifaces.get(i).copied() else {
                continue;
            };
            for f in &iface.methods {
                if f.body.is_some() && !own.iter().any(|m| m.name == f.name) {
                    out.push((*i, args.clone(), f));
                }
            }
        }
        out
    }

    /// A method `recv` of type `recv_ty` that calls the default `f` of the interface `iface`
    /// with the type arguments `args`.
    fn delegate(
        &mut self,
        recv: (String, Type),
        self_ty: &IrType,
        iface: DefRef,
        args: &[IrType],
        f: &ir::Function,
    ) -> Result<Func, Error> {
        let names: Vec<String> = self
            .ifaces
            .get(&iface)
            .map(|i| i.generics.iter().map(|g| g.name.clone()).collect())
            .unwrap_or_default();
        let subst = |t: &IrType| -> IrType {
            let mut map = BTreeMap::new();
            for (n, a) in names.iter().zip(args) {
                map.insert(n.clone(), a.clone());
            }
            let t = replace_self(t, self_ty);
            subst(&t, &map)
        };
        let mut params = Vec::new();
        for p in &f.params {
            let t = subst(&p.ty);
            params.push((local_name_of(&p.name), self.param_ty(p.mode, &t)?));
        }
        let results = self.results(&subst(&f.ret))?;
        let mut targs = Vec::new();
        for a in args {
            targs.push(self.ty(a)?);
        }
        let name = self.default_fn_name(iface, &f.name);
        let callee = match self.qualifier(iface) {
            Some(path) => Expr::Qual(path, name),
            None => id(&name),
        };
        let mut all = vec![id(&recv.0)];
        all.extend(params.iter().map(|(n, _)| id(n)));
        let c = callt(callee, targs, all);
        let body = if results.is_empty() {
            vec![Stmt::Expr(c)]
        } else {
            vec![Stmt::Return(vec![c])]
        };
        Ok(Func {
            docs: Vec::new(),
            recv: Some(recv),
            name: value_name(&f.name, true),
            tparams: Vec::new(),
            params,
            results,
            body,
        })
    }

    fn enum_item(&mut self, e: &'p ir::Enum, out: &mut Vec<Decl>) -> Result<(), Error> {
        if self.std_pieces.contains_key(&e.def)
            || self
                .def_module(e.def)
                .and_then(|m| self.p.modules.get(m))
                .is_some_and(|m| m.role == ModuleRole::Std)
        {
            return Ok(());
        }
        let name = self.name_of(e.def);
        let tparams = self.tparams(&e.generics)?;
        let args: Vec<IrType> = e
            .generics
            .iter()
            .map(|g| IrType::Param(g.name.clone()))
            .collect();
        let self_ty = IrType::Named(e.def, args);
        let enum_ty = self.ty(&self_ty)?;
        let plain = e.variants.iter().all(|v| v.payload.is_empty());
        if plain {
            out.push(Decl::TypeDef {
                docs: e.docs.clone(),
                name: name.clone(),
                tparams: Vec::new(),
                ty: int64(),
                alias: false,
            });
            let mut specs = Vec::new();
            for (i, v) in e.variants.iter().enumerate() {
                let (ty, value) = if i == 0 {
                    (Some(Type::name(&name)), Some(lit("iota")))
                } else {
                    (None, None)
                };
                specs.push((v.docs.clone(), self.name_of(v.def), ty, value));
            }
            out.push(Decl::Consts(specs));
            let ptr = self.ptr_recv.contains(&e.def);
            let recv = self.receiver_name(&e.name, &e.methods);
            for f in &e.methods {
                if f.body.is_none() {
                    continue;
                }
                let fname = self.name_of(f.def);
                let func = if f.self_mode.is_some() && f.generics.is_empty() {
                    let (ty, local) = if ptr {
                        (
                            Type::Ptr(Box::new(enum_ty.clone())),
                            Local::Ptr(recv.clone()),
                        )
                    } else {
                        (enum_ty.clone(), Local::Val(recv.clone()))
                    };
                    self.function(f, fname, Some((recv.clone(), ty, local)), &[], None)?
                } else {
                    self.function(f, fname, None, &e.generics, Some(&self_ty))?
                };
                out.push(Decl::Func(func));
            }
            let rty = if ptr {
                Type::Ptr(Box::new(enum_ty.clone()))
            } else {
                enum_ty.clone()
            };
            for (iface, args, f) in self.inherited(&e.implements, &e.methods) {
                let func = self.delegate((recv.clone(), rty.clone()), &self_ty, iface, &args, f)?;
                out.push(Decl::Func(func));
            }
            self.iface_copies(e.def, &e.methods, false, &enum_ty, true, out)?;
            if self.error_types.contains(&e.def) {
                out.push(self.error_method(enum_ty, &e.generics));
            }
            return Ok(());
        }
        if e.methods
            .iter()
            .filter_map(|f| f.interface)
            .any(|i| self.need_copy_iface.contains(&i))
        {
            return Err("an enum with payloads as a copied interface value".to_string());
        }
        // A sealed interface with a struct per variant.
        let marker = format!("is{}", e.name);
        let mut elems = vec![IfaceElem::Method(
            Vec::new(),
            marker.clone(),
            Vec::new(),
            Vec::new(),
        )];
        let iface_methods: Vec<&ir::Function> = e
            .methods
            .iter()
            .filter(|f| f.interface.is_some() && f.self_mode.is_some())
            .collect();
        let mut sigs = Vec::new();
        for f in &iface_methods {
            let mut params = Vec::new();
            for p in &f.params {
                params.push((local_name_of(&p.name), self.param_ty(p.mode, &p.ty)?));
            }
            let results = self.results(&f.ret)?;
            let mname = self.name_of_method(f);
            elems.push(IfaceElem::Method(
                Vec::new(),
                mname.clone(),
                params.clone(),
                results.clone(),
            ));
            sigs.push((f.def, mname, params, results));
        }
        let inherited = self.inherited(&e.implements, &e.methods);
        for (iface, args, f) in &inherited {
            let d = self.delegate(
                ("x".to_string(), enum_ty.clone()),
                &self_ty,
                *iface,
                args,
                f,
            )?;
            elems.push(IfaceElem::Method(Vec::new(), d.name, d.params, d.results));
        }
        let is_error = self.error_types.contains(&e.def);
        if is_error {
            elems.push(IfaceElem::Method(
                Vec::new(),
                "Error".to_string(),
                Vec::new(),
                vec![Type::name("string")],
            ));
        }
        self.f = FnState::default();
        let eq_dicts = self.type_dicts(&e.generics, Dict::Eq)?;
        self.f = FnState::default();
        let copy_dicts = self.type_dicts(&e.generics, Dict::Copy)?;
        self.f = FnState::default();
        let need_eq = self.need_eq.contains(&e.def);
        let need_copy = self.need_copy.contains(&e.def);
        if need_eq {
            let mut params = vec![("other".to_string(), enum_ty.clone())];
            params.extend(eq_dicts.clone());
            elems.push(IfaceElem::Method(
                Vec::new(),
                "Equal".to_string(),
                params,
                vec![Type::name("bool")],
            ));
        }
        if need_copy {
            elems.push(IfaceElem::Method(
                Vec::new(),
                "Copy".to_string(),
                copy_dicts.clone(),
                vec![enum_ty.clone()],
            ));
        }
        out.push(Decl::Interface {
            docs: e.docs.clone(),
            name: name.clone(),
            tparams: tparams.clone(),
            elems,
        });
        let targs: Vec<Type> = e.generics.iter().map(|g| Type::name(&g.name)).collect();
        for v in &e.variants {
            let vname = self.name_of(v.def);
            let (fnames, ftypes) = self.payload(v);
            let mut fields = Vec::new();
            for (n, t) in fnames.iter().zip(&ftypes) {
                fields.push(Field {
                    docs: Vec::new(),
                    name: n.clone(),
                    ty: self.ty(t)?,
                });
            }
            out.push(Decl::Struct {
                docs: v.docs.clone(),
                name: vname.clone(),
                tparams: tparams.clone(),
                fields,
            });
            let vty = Type::Ptr(Box::new(Type::Name(None, vname.clone(), targs.clone())));
            out.push(Decl::Func(Func {
                docs: Vec::new(),
                recv: Some((String::new(), vty.clone())),
                name: marker.clone(),
                tparams: Vec::new(),
                params: Vec::new(),
                results: Vec::new(),
                body: Vec::new(),
            }));
            for (def, mname, params, results) in &sigs {
                let fname = self.name_of(*def);
                let mut args = vec![id("x")];
                args.extend(params.iter().map(|(n, _)| id(n)));
                let targs = targs.clone();
                let c = callt(id(&fname), targs, args);
                let body = if results.is_empty() {
                    vec![Stmt::Expr(c)]
                } else {
                    vec![Stmt::Return(vec![c])]
                };
                out.push(Decl::Func(Func {
                    docs: Vec::new(),
                    recv: Some(("x".to_string(), vty.clone())),
                    name: mname.clone(),
                    tparams: Vec::new(),
                    params: params.clone(),
                    results: results.clone(),
                    body,
                }));
            }
            for (iface, args, f) in &inherited {
                let d = self.delegate(("x".to_string(), vty.clone()), &self_ty, *iface, args, f)?;
                out.push(Decl::Func(d));
            }
            if is_error {
                out.push(self.error_method(vty.clone(), &e.generics));
            }
            if need_eq {
                self.f = FnState::default();
                let _ = self.type_dicts(&e.generics, Dict::Eq)?;
                let mut conds = vec![id("ok")];
                for (n, t) in fnames.iter().zip(&ftypes) {
                    conds.push(self.eq_expr(sel(id("x"), n), sel(id("o"), n), t)?);
                }
                self.f = FnState::default();
                let o = if fnames.is_empty() { "_" } else { "o" };
                let mut params = vec![("other".to_string(), enum_ty.clone())];
                params.extend(eq_dicts.clone());
                out.push(Decl::Func(Func {
                    docs: Vec::new(),
                    recv: Some(("x".to_string(), vty.clone())),
                    name: "Equal".to_string(),
                    tparams: Vec::new(),
                    params,
                    results: vec![Type::name("bool")],
                    body: vec![
                        Stmt::Define(
                            vec![o.to_string(), "ok".to_string()],
                            vec![Expr::Assert(Box::new(id("other")), vty.clone())],
                        ),
                        Stmt::Return(vec![and_all(conds).unwrap_or_else(|| lit("true"))]),
                    ],
                }));
            }
            if need_copy {
                self.f = FnState::default();
                let _ = self.type_dicts(&e.generics, Dict::Copy)?;
                let mut elems = Vec::new();
                for (n, t) in fnames.iter().zip(&ftypes) {
                    elems.push((Some(n.clone()), self.copy_expr(sel(id("x"), n), t)?));
                }
                self.f = FnState::default();
                let lit_ty = Type::Name(None, vname.clone(), targs.clone());
                out.push(Decl::Func(Func {
                    docs: Vec::new(),
                    recv: Some(("x".to_string(), vty.clone())),
                    name: "Copy".to_string(),
                    tparams: Vec::new(),
                    params: copy_dicts.clone(),
                    results: vec![enum_ty.clone()],
                    body: vec![Stmt::Return(vec![addr(Expr::Composite(lit_ty, elems))])],
                }));
            }
        }
        // The functions of the enum, as functions of the package.
        for f in &e.methods {
            if f.body.is_none() {
                continue;
            }
            let fname = self.name_of(f.def);
            let func = self.function(f, fname, None, &e.generics, Some(&self_ty))?;
            out.push(Decl::Func(func));
        }
        Ok(())
    }

    /// The Go name of a method: interface methods are always exported.
    fn name_of_method(&self, f: &ir::Function) -> String {
        value_name(&f.name, true)
    }

    /// Names and types of a variant's payload.
    fn payload(&self, v: &ir::Variant) -> (Vec<String>, Vec<IrType>) {
        match &v.payload {
            Payload::None => (Vec::new(), Vec::new()),
            Payload::Tuple(types) => (
                (0..types.len()).map(|i| format!("F{i}")).collect(),
                types.clone(),
            ),
            Payload::Struct(fs) => (
                fs.iter().map(|f| value_name(&f.name, true)).collect(),
                fs.iter().map(|f| f.ty.clone()).collect(),
            ),
        }
    }

    fn extern_fn(&mut self, x: &'p ir::ExternFn, out: &mut Vec<Decl>) -> Result<(), Error> {
        let Some(path) = x.paths.get("go") else {
            return Ok(());
        };
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (i, (m, t)) in x.params.iter().enumerate() {
            let n = format!("p{i}");
            params.push((n.clone(), self.param_ty(*m, t)?));
            // A function of a native module takes the Go types the reader found.
            let a = match (x.native.get("go").and_then(|s| s.params.get(i)), t) {
                (Some(nt), _) => self.go_arg(id(&n), nt, t)?,
                // A pointer to a copy, or nil (spec chapter 8, section 8, D8.12).
                (None, IrType::Option(inner)) => {
                    let f = self.extern_out_fn(inner)?;
                    self.rt_call("externOption", "OptionToPtr", vec![id(&n), f])
                }
                (None, _) => self.extern_arg(id(&n), t)?,
            };
            args.push(a);
        }
        let path = &self.extern_path(path);
        // A method expression, `(*pkg.Type).Method` or `pkg.Type.Method`, is called on the first
        // parameter (spec chapter 7, section 8.2).
        let c = match go_target(path) {
            Some(GoTarget::Method(method)) => {
                let mut rest = args.into_iter();
                let recv = rest
                    .next()
                    .ok_or_else(|| format!("the extern method `{}` has no receiver", x.name))?;
                call(sel(recv, &method), rest.collect())
            }
            Some(GoTarget::Func(import, name)) => {
                self.file.imports.insert(import.clone());
                call(Expr::Qual(import, name), args)
            }
            None => return Err(format!("the Go path `{path}` of `{}`", x.name)),
        };
        let results = self.results(&x.ret)?;
        let form = self.ret_form(&x.ret);
        let native = x.native.get("go").map(|s| &s.ret);
        let body = self.extern_body(c, &x.ret, form, results.len(), native)?;
        out.push(Decl::Func(Func {
            docs: Vec::new(),
            recv: None,
            name: self.name_of(x.def),
            tparams: Vec::new(),
            params,
            results,
            body,
        }));
        Ok(())
    }

    /// The body of an extern function: the call, with its result converted (spec chapter 8,
    /// section 8).
    fn extern_body(
        &mut self,
        c: Expr,
        ret: &IrType,
        form: Form,
        count: usize,
        native: Option<&NativeTy>,
    ) -> Result<Vec<Stmt>, Error> {
        if count == 0 {
            return Ok(vec![Stmt::Expr(c)]);
        }
        // The inner native type of `(T, error)` and `(T, bool)`.
        let native_inner = match native {
            Some(NativeTy::Result(x) | NativeTy::Option(x)) => Some(x.as_ref().clone()),
            _ => None,
        };
        // A Go error becomes an `Error` whose message is its text (spec chapter 7, section 8.4).
        let pair = |second: &str, ok: &IrType, this: &mut Self| -> Result<Vec<Stmt>, Error> {
            let v = match &native_inner {
                Some(nt) => this.go_value(id("v"), nt, ok)?,
                None => this.extern_value(id("v"), ok)?,
            };
            let second_value = if second == "err" {
                this.rt_call("externError", "FromExternError", vec![id("err")])
            } else {
                id(second)
            };
            Ok(vec![
                Stmt::Define(vec!["v".to_string(), second.to_string()], vec![c.clone()]),
                Stmt::Return(vec![v, second_value]),
            ])
        };
        Ok(match (form, ret) {
            (Form::ResErr, _) => {
                let e = self.rt_call("externError", "FromExternError", vec![c]);
                vec![Stmt::Return(vec![e])]
            }
            (Form::ResPair, IrType::Result(ok, _)) => pair("err", ok, self)?,
            (Form::OptPair, IrType::Option(inner))
                if self.extern_converts(inner) || native_inner.is_some() =>
            {
                pair("ok", inner, self)?
            }
            // A record of a native module comes as `(T, bool)`.
            (Form::OptPtr, IrType::Option(inner)) if native_inner.is_some() => {
                let nt = native_inner.clone().unwrap_or(NativeTy::Unit);
                let v = self.go_value(id("v"), &nt, inner)?;
                vec![
                    Stmt::Define(vec!["v".to_string(), "ok".to_string()], vec![c]),
                    Stmt::If(
                        None,
                        not(id("ok")),
                        vec![Stmt::Return(vec![lit("nil")])],
                        None,
                    ),
                    Stmt::Return(vec![addr(v)]),
                ]
            }
            // A pointer to a record: copied unless it is `nil`.
            (Form::OptPtr, IrType::Option(inner)) if self.record(inner).is_some() => {
                let f = self.extern_in_fn(inner, false)?;
                vec![Stmt::Return(vec![self.rt_call(
                    "externMap",
                    "ConvertPtr",
                    vec![c, f],
                )])]
            }
            // A record is read field by field: the call is made once.
            (Form::Data, t) if self.record(t).is_some() => {
                let v = match native {
                    Some(nt) => self.go_value(id("v"), nt, t)?,
                    None => self.extern_value(id("v"), t)?,
                };
                vec![
                    Stmt::Define(vec!["v".to_string()], vec![c]),
                    Stmt::Return(vec![v]),
                ]
            }
            (Form::Data, t) => {
                let v = match native {
                    Some(nt) => self.go_value(c, nt, t)?,
                    None => self.extern_value(c, t)?,
                };
                vec![Stmt::Return(vec![v])]
            }
            _ => vec![Stmt::Return(vec![c])],
        })
    }

    /// Whether a value of type `t` crosses the extern boundary in another Go type: a `Map`, a
    /// record, or a list of these.
    fn extern_needs(&self, t: &IrType) -> bool {
        match t {
            IrType::Map(..) => true,
            IrType::List(inner) => self.extern_needs(inner),
            _ => self.record(t).is_some(),
        }
    }

    /// Whether a value of type `t` returned by extern code is converted.
    fn extern_converts(&self, t: &IrType) -> bool {
        matches!(t, IrType::Int) || self.extern_needs(t)
    }

    /// The struct of a record bound to a Go struct type (spec chapter 7, section 8.4a), and that
    /// type.
    fn record(&self, t: &IrType) -> Option<(&'p ir::Struct, String)> {
        let IrType::Named(d, _) = t else {
            return None;
        };
        let s = *self.structs.get(d)?;
        let path = s.extern_paths.get("go")?;
        Some((s, self.extern_path(path)))
    }

    /// The Go struct type of a record on the side of extern code.
    fn record_ty(&mut self, path: &str) -> Result<Type, Error> {
        let Some(GoTarget::Func(import, name)) = go_target(path) else {
            return Err(format!("the Go type path `{path}` of a record"));
        };
        self.file.imports.insert(import.clone());
        Ok(Type::Name(Some(import), name, Vec::new()))
    }

    /// A record built field by field, each field converted: for extern code (`out`) or from it.
    fn record_value(
        &mut self,
        e: Expr,
        t: &IrType,
        s: &'p ir::Struct,
        path: &str,
        out: bool,
    ) -> Result<Expr, Error> {
        // A record of a native module: the Go names and types the reader found.
        let native = s.extern_fields.get("go");
        let mut elems = Vec::new();
        for (i, f) in s.fields.iter().enumerate() {
            let found = native.and_then(|n| n.get(i));
            let ours = value_name(&f.name, f.public);
            let theirs = match found {
                Some((name, _)) => name.clone(),
                None => value_name(&f.name, true),
            };
            let (from, to) = if out { (ours, theirs) } else { (theirs, ours) };
            let field = sel(e.clone(), &from);
            let v = match (found, out) {
                (Some((_, nt)), true) => self.go_arg(field, nt, &f.ty)?,
                (Some((_, nt)), false) => self.go_value(field, nt, &f.ty)?,
                (None, true) => self.extern_arg(field, &f.ty)?,
                (None, false) => self.extern_value(field, &f.ty)?,
            };
            elems.push((Some(to), v));
        }
        let ty = if out {
            self.record_ty(path)?
        } else {
            self.ty(t)?
        };
        Ok(Expr::Composite(ty, elems))
    }

    /// A wolfpack value passed to extern code: a `Map` becomes a Go map (spec chapter 8,
    /// section 8).
    fn extern_arg(&mut self, e: Expr, t: &IrType) -> Result<Expr, Error> {
        Ok(match t {
            IrType::Map(k, v) => {
                let key = self.extern_out_fn(k)?;
                let value = self.extern_out_fn(v)?;
                self.rt_call("externMap", "ToExternMap", vec![e, key, value])
            }
            IrType::List(inner) if self.extern_needs(inner) => {
                let f = self.extern_out_fn(inner)?;
                self.rt_call("externMap", "ConvertList", vec![e, f])
            }
            _ => match self.record(t) {
                Some((s, path)) => self.record_value(e, t, s, &path, true)?,
                None => e,
            },
        })
    }

    /// A wolfpack value of type `t` passed to a function of a native module, which takes the Go
    /// type `nt` the reader found (spec chapter 7, section 8.7, rule 9).
    fn go_arg(&mut self, e: Expr, nt: &NativeTy, t: &IrType) -> Result<Expr, Error> {
        Ok(match (nt, t) {
            (NativeTy::Int(n), _) if n != "int64" => {
                let f = self.rt("extern", "ToTargetInt");
                call(Expr::Inst(Box::new(f), vec![Type::name(n)]), vec![e])
            }
            (NativeTy::Float(n), _) if n == "float32" => {
                self.rt_call("extern", "ToFloat32", vec![e])
            }
            (NativeTy::Vec(inner), IrType::List(it)) if !go_exact(inner) => {
                let f = self.go_arg_fn(inner, it)?;
                self.rt_call("externMap", "ConvertList", vec![e, f])
            }
            (NativeTy::Map { key, value, .. }, IrType::Map(kt, vt)) => {
                let key = self.go_arg_fn(key, kt)?;
                let value = self.go_arg_fn(value, vt)?;
                self.rt_call("externMap", "ToExternMap", vec![e, key, value])
            }
            (NativeTy::Record { .. }, t) => match self.record(t) {
                Some((s, path)) => self.record_value(e, t, s, &path, true)?,
                None => return Err(format!("the record {t:?} of a native module")),
            },
            _ => e,
        })
    }

    /// The function that converts a wolfpack value of type `t` for the Go type `nt`.
    fn go_arg_fn(&mut self, nt: &NativeTy, t: &IrType) -> Result<Expr, Error> {
        let from = self.ty(t)?;
        if go_exact(nt) {
            let same = self.rt("externMap", "Same");
            return Ok(Expr::Inst(Box::new(same), vec![from]));
        }
        let to = self.go_native(nt, t)?;
        let body = self.go_arg(id("v"), nt, t)?;
        Ok(Expr::FuncLit(
            vec![("v".to_string(), from)],
            vec![to],
            vec![Stmt::Return(vec![body])],
        ))
    }

    /// A value of the Go type `nt` returned by a function of a native module, as type `t`.
    fn go_value(&mut self, e: Expr, nt: &NativeTy, t: &IrType) -> Result<Expr, Error> {
        Ok(match (nt, t) {
            (NativeTy::Int(n), _) if n != "int64" => {
                self.rt_call("extern", "FromTargetInt", vec![e])
            }
            (NativeTy::Float(n), _) if n == "float32" => {
                self.rt_call("extern", "FromFloat32", vec![e])
            }
            (NativeTy::Vec(inner), IrType::List(it)) if !go_exact(inner) => {
                let f = self.go_value_fn(inner, it)?;
                self.rt_call("externMap", "ConvertList", vec![e, f])
            }
            (NativeTy::Map { key, value, .. }, IrType::Map(kt, vt)) => {
                let compare = match key.as_ref() {
                    NativeTy::Int(_) => {
                        let f = self.rt("externMap", "CompareKey");
                        Expr::Inst(Box::new(f), vec![int64()])
                    }
                    NativeTy::String => {
                        let f = self.rt("externMap", "CompareKey");
                        Expr::Inst(Box::new(f), vec![Type::name("string")])
                    }
                    _ => self.rt("externMap", "CompareBool"),
                };
                let key = self.go_value_fn(key, kt)?;
                let value = self.go_value_fn(value, vt)?;
                self.rt_call("externMap", "FromExternMap", vec![e, key, value, compare])
            }
            (NativeTy::Record { .. }, t) => match self.record(t) {
                Some((s, path)) => self.record_value(e, t, s, &path, false)?,
                None => return Err(format!("the record {t:?} of a native module")),
            },
            _ => e,
        })
    }

    /// The function that converts a value of the Go type `nt` to its wolfpack type `t`.
    fn go_value_fn(&mut self, nt: &NativeTy, t: &IrType) -> Result<Expr, Error> {
        let from = self.go_native(nt, t)?;
        if go_exact(nt) {
            let same = self.rt("externMap", "Same");
            return Ok(Expr::Inst(Box::new(same), vec![from]));
        }
        let to = self.ty(t)?;
        let body = self.go_value(id("v"), nt, t)?;
        Ok(Expr::FuncLit(
            vec![("v".to_string(), from)],
            vec![to],
            vec![Stmt::Return(vec![body])],
        ))
    }

    /// The Go type `nt` of a native module, for a value of type `t` inside a list or a map.
    fn go_native(&mut self, nt: &NativeTy, t: &IrType) -> Result<Type, Error> {
        Ok(match (nt, t) {
            (NativeTy::Int(n) | NativeTy::Float(n), _) => Type::name(n),
            (NativeTy::Bool, _) => Type::name("bool"),
            (NativeTy::String, _) => Type::name("string"),
            (NativeTy::Vec(inner), IrType::List(it)) => {
                Type::Slice(Box::new(self.go_native(inner, it)?))
            }
            (NativeTy::Map { key, value, .. }, IrType::Map(kt, vt)) => Type::Map(
                Box::new(self.go_native(key, kt)?),
                Box::new(self.go_native(value, vt)?),
            ),
            (NativeTy::Record { .. }, t) => self.extern_ty(t)?,
            (other, _) => return Err(format!("the Go type {other:?} inside a list or a map")),
        })
    }

    /// The function that converts a value of type `t` for extern code.
    fn extern_out_fn(&mut self, t: &IrType) -> Result<Expr, Error> {
        let from = self.ty(t)?;
        if !self.extern_needs(t) {
            let same = self.rt("externMap", "Same");
            return Ok(Expr::Inst(Box::new(same), vec![from]));
        }
        let to = self.extern_ty(t)?;
        let body = self.extern_arg(id("v"), t)?;
        Ok(Expr::FuncLit(
            vec![("v".to_string(), from)],
            vec![to],
            vec![Stmt::Return(vec![body])],
        ))
    }

    /// A value returned by extern code: an `Int` of any Go integer type, a Go map sorted by key
    /// (spec chapter 7, section 8.4, rule 6).
    fn extern_value(&mut self, e: Expr, t: &IrType) -> Result<Expr, Error> {
        Ok(match t {
            IrType::Int => self.rt_call("extern", "FromTargetInt", vec![e]),
            IrType::Map(k, v) => {
                let key = self.extern_in_fn(k, true)?;
                let value = self.extern_in_fn(v, true)?;
                let compare = match **k {
                    IrType::Int => {
                        let f = self.rt("externMap", "CompareKey");
                        Expr::Inst(Box::new(f), vec![int64()])
                    }
                    IrType::String => {
                        let f = self.rt("externMap", "CompareKey");
                        Expr::Inst(Box::new(f), vec![Type::name("string")])
                    }
                    _ => self.rt("externMap", "CompareBool"),
                };
                self.rt_call("externMap", "FromExternMap", vec![e, key, value, compare])
            }
            IrType::List(inner) if self.extern_needs(inner) => {
                let f = self.extern_in_fn(inner, false)?;
                self.rt_call("externMap", "ConvertList", vec![e, f])
            }
            _ => match self.record(t) {
                Some((s, path)) => self.record_value(e, t, s, &path, false)?,
                None => e,
            },
        })
    }

    /// The function that converts a value of type `t` from extern code. With `any_int`, an
    /// `Int` may be of any Go integer type; Go infers it from the map.
    fn extern_in_fn(&mut self, t: &IrType, any_int: bool) -> Result<Expr, Error> {
        if matches!(t, IrType::Int) && any_int {
            return Ok(self.rt("extern", "FromTargetInt"));
        }
        if !self.extern_needs(t) {
            return Ok(self.rt("externMap", "Same"));
        }
        let from = self.extern_ty(t)?;
        let to = self.ty(t)?;
        let body = self.extern_value(id("v"), t)?;
        Ok(Expr::FuncLit(
            vec![("v".to_string(), from)],
            vec![to],
            vec![Stmt::Return(vec![body])],
        ))
    }

    /// The Go type of a value of type `t` on the side of extern code.
    fn extern_ty(&mut self, t: &IrType) -> Result<Type, Error> {
        Ok(match t {
            IrType::Map(k, v) => {
                Type::Map(Box::new(self.extern_ty(k)?), Box::new(self.extern_ty(v)?))
            }
            IrType::List(inner) => Type::Slice(Box::new(self.extern_ty(inner)?)),
            _ => match self.record(t) {
                Some((_, path)) => self.record_ty(&path)?,
                None => self.ty(t)?,
            },
        })
    }

    /// The Go path of an extern path string: a package of `native/go/` is a package of the
    /// generated module (spec chapter 8, section 8a), `native:*search.Index` becomes
    /// `*<module>/native/search.Index`.
    fn extern_path(&self, path: &str) -> String {
        let Some(rest) = path.strip_prefix(NATIVE) else {
            return path.to_string();
        };
        let (open, rest) = match rest.strip_prefix('(') {
            Some(r) => ("(", r),
            None => ("", rest),
        };
        let (ptr, rest) = match rest.strip_prefix('*') {
            Some(r) => ("*", r),
            None => ("", rest),
        };
        format!("{open}{ptr}{}/native/{rest}", self.module_path)
    }

    /// `type pattern = *regexp.Regexp` for an opaque type.
    fn extern_type(&mut self, x: &'p ir::ExternType, out: &mut Vec<Decl>) -> Result<(), Error> {
        let Some(path) = x.paths.get("go") else {
            return Ok(());
        };
        let path = &self.extern_path(path);
        let (ptr, rest) = match path.strip_prefix('*') {
            Some(rest) => (true, rest),
            None => (false, path.as_str()),
        };
        let Some(GoTarget::Func(import, name)) = go_target(rest) else {
            return Err(format!("the Go type path `{path}` of `{}`", x.name));
        };
        self.file.imports.insert(import.clone());
        let mut ty = Type::Name(Some(import), name, Vec::new());
        if ptr {
            ty = Type::Ptr(Box::new(ty));
        }
        out.push(Decl::TypeDef {
            docs: Vec::new(),
            name: self.name_of(x.def),
            tparams: Vec::new(),
            ty,
            alias: true,
        });
        Ok(())
    }

    // ----- functions --------------------------------------------------------------------------

    /// A function or method. `recv` is the receiver of a Go method; `owner` and `self_ty` are
    /// set for functions of a type that are emitted as functions of the package (their type
    /// parameters include the type's, and `self` becomes the first parameter).
    fn function(
        &mut self,
        f: &'p ir::Function,
        name: String,
        recv: Option<(String, Type, Local)>,
        owner: &[ir::Generic],
        self_ty: Option<&IrType>,
    ) -> Result<Func, Error> {
        self.f = FnState::default();
        let mut scan = Scan::default();
        if let Some(b) = &f.body {
            scan.block(b);
        }
        self.f.reads = scan.reads;
        self.f.assigned = scan.assigned;
        let form = self.ret_form(&f.ret);
        self.f.rets.push((f.ret.clone(), form));
        // Type parameters: the type's (for functions of the package) and the function's own.
        let owner_generics: Vec<ir::Generic> = if owner.is_empty() {
            Vec::new()
        } else {
            owner
                .iter()
                .enumerate()
                .map(|(i, g)| ir::Generic {
                    name: g.name.clone(),
                    bounds: f
                        .impl_bounds
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| g.bounds.clone()),
                })
                .collect()
        };
        let mut all = owner_generics.clone();
        all.extend(f.generics.iter().cloned());
        let tparams = if recv.is_some() {
            self.tparams(&f.generics)?
        } else {
            self.tparams(&all)?
        };
        // Dictionaries: for a Go method, the impl block's bounds and the function's own.
        let mut params = Vec::new();
        if let Some((r, _, local)) = &recv {
            self.f.used.insert(r.clone());
            self.f.self_local = Some(local.clone());
            let gens: Vec<ir::Generic> = self
                .fns
                .get(&f.def)
                .map(|i| i.owner_generics.clone())
                .unwrap_or_default();
            params.extend(self.dict_params(&gens)?);
            params.extend(self.dict_params(&f.generics)?);
        } else {
            params.extend(self.dict_params(&all)?);
        }
        if let (Some(mode), Some(st)) = (f.self_mode, self_ty) {
            // In a default the receiver is the interface value itself, also for `mut self`: the
            // value it holds is the one that the methods change.
            let mode = if self.self_iface.is_some() {
                Mode::Read
            } else {
                mode
            };
            let t = self.param_ty(mode, st)?;
            let n = self.fresh("self");
            self.f.self_local = Some(if mode == Mode::Mut {
                Local::Ptr(n.clone())
            } else {
                Local::Val(n.clone())
            });
            params.insert(0, (n, t));
        }
        for p in &f.params {
            let n = self.local_name(p.local);
            let t = self.param_ty(p.mode, &p.ty)?;
            let l = if p.mode == Mode::Mut {
                Local::Ptr(n.clone())
            } else {
                Local::Val(n.clone())
            };
            self.f.locals.insert(p.local, l);
            params.push((n, t));
        }
        let results = self.results(&f.ret)?;
        let mut body = Vec::new();
        if let Some(b) = &f.body {
            let target = if results.is_empty() {
                Target::Discard
            } else {
                Target::Return
            };
            self.block(b, target, &mut body)?;
        }
        if !results.is_empty() && !terminates(&body) {
            body.push(Stmt::Expr(call(
                id("panic"),
                vec![Expr::Str("unreachable".to_string())],
            )));
        }
        self.f = FnState::default();
        Ok(Func {
            docs: f.docs.clone(),
            recv: recv.map(|(n, t, _)| (n, t)),
            name,
            tparams,
            params,
            results,
            body,
        })
    }

    // ----- statements -------------------------------------------------------------------------

    fn block(&mut self, b: &'p Block, target: Target, out: &mut Vec<Stmt>) -> Result<(), Error> {
        for s in &b.stmts {
            self.stmt(s, out)?;
        }
        match &b.tail {
            Some(tail) => self.tail(tail, target, out)?,
            None => {
                if let Target::Assign(place) = target {
                    // A block of type `Void` assigned to a temporary.
                    out.push(Stmt::Assign(vec![place], "=", vec![unit()]));
                }
            }
        }
        Ok(())
    }

    fn ret_form_now(&self) -> (IrType, Form) {
        self.f
            .rets
            .last()
            .cloned()
            .unwrap_or((IrType::Void, Form::Data))
    }

    /// The final expression of a block, sent to its target.
    fn tail(&mut self, e: &'p IrExpr, target: Target, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match &e.kind {
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond, out)?;
                let mut t = Vec::new();
                self.block(then, target.clone(), &mut t)?;
                let o = match otherwise {
                    Some(o) => {
                        let mut v = Vec::new();
                        self.block(o, target, &mut v)?;
                        Some(v)
                    }
                    None => None,
                };
                out.push(Stmt::If(None, c, t, o.filter(|v| !v.is_empty())));
                Ok(())
            }
            ExprKind::Match {
                scrutinee,
                mode,
                arms,
            } => self.match_(scrutinee, *mode, arms, target, out),
            ExprKind::Block(b) => self.block(b, target, out),
            ExprKind::Loop { label, body } => {
                let to = match target {
                    Target::Discard => None,
                    t => Some(t),
                };
                let (b, l) = self.loop_body(label, to, body)?;
                out.push(labeled(Stmt::For(None, None, None, b), l));
                Ok(())
            }
            ExprKind::Return(_) | ExprKind::Break { .. } | ExprKind::Continue { .. } => {
                self.control(e, out)
            }
            ExprKind::Panic { message, location } => {
                let m = self.expr(message, out)?;
                let v = self.rt_call("Panic", "PanicAt", vec![m, Expr::Str(location.clone())]);
                out.push(Stmt::Expr(call(id("panic"), vec![v])));
                Ok(())
            }
            _ => {
                match target {
                    Target::Discard => {
                        let v = self.discard(e, out)?;
                        if let Some(v) = v {
                            out.push(v);
                        }
                    }
                    Target::Return => {
                        if matches!(e.ty, IrType::Never) {
                            let v = self.discard(e, out)?;
                            out.extend(v);
                            return Ok(());
                        }
                        let vs = self.ret_values(e, out)?;
                        out.push(Stmt::Return(vs));
                    }
                    Target::Assign(place) => {
                        if matches!(e.ty, IrType::Never) {
                            let v = self.discard(e, out)?;
                            out.extend(v);
                        } else {
                            let v = self.expr(e, out)?;
                            out.push(Stmt::Assign(vec![place], "=", vec![v]));
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// An expression whose value is discarded, as a statement (if it does anything).
    fn discard(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Option<Stmt>, Error> {
        let v = if let ExprKind::Call { callee, args } = &e.kind {
            self.call(callee, args, &e.ty, out)?.0
        } else {
            self.expr(e, out)?
        };
        if pure(&v) {
            return Ok(None);
        }
        // Calls of functions may discard their results; other expressions are assigned to `_`.
        Ok(Some(if statement_call(&v) {
            Stmt::Expr(v)
        } else {
            Stmt::Assign(vec![id("_")], "=", vec![v])
        }))
    }

    /// `return`, `break` and `continue` as statements.
    fn control(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match &e.kind {
            ExprKind::Return(value) => match value {
                Some(v) => {
                    let (rt, _) = self.ret_form_now();
                    if matches!(rt, IrType::Void) {
                        let s = self.discard(v, out)?;
                        out.extend(s);
                        out.push(Stmt::Return(Vec::new()));
                    } else {
                        let vs = self.ret_values(v, out)?;
                        out.push(Stmt::Return(vs));
                    }
                }
                None => out.push(Stmt::Return(Vec::new())),
            },
            ExprKind::Break { label: None, value } => {
                if let Some(v) = value {
                    // `break value` leaves the innermost loop, a `loop` (chapter 3, rule 4.5).
                    match self.f.loops.last().and_then(|l| l.target.clone()) {
                        Some(Target::Return) => return self.control_return(v, out),
                        Some(t) => self.tail(v, t, out)?,
                        None => self.tail(v, Target::Discard, out)?,
                    }
                }
                let label = self.break_label();
                out.push(Stmt::Break(label));
            }
            ExprKind::Break {
                label: Some(user), ..
            } => {
                let l = self.user_label(user)?;
                out.push(Stmt::Break(Some(l)));
            }
            ExprKind::Continue { label } => {
                let l = match label {
                    Some(user) => Some(self.user_label(user)?),
                    None => None,
                };
                out.push(Stmt::Continue(l));
            }
            _ => {}
        }
        Ok(())
    }

    /// `return v` for the value of a `loop` in the position of the result.
    fn control_return(&mut self, v: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<(), Error> {
        let (rt, _) = self.ret_form_now();
        if matches!(rt, IrType::Void) {
            let s = self.discard(v, out)?;
            out.extend(s);
            out.push(Stmt::Return(Vec::new()));
        } else {
            let vs = self.ret_values(v, out)?;
            out.push(Stmt::Return(vs));
        }
        Ok(())
    }

    /// The Go label of the enclosing loop with the label `user` of the program.
    fn user_label(&mut self, user: &str) -> Result<String, Error> {
        let index = self
            .f
            .loops
            .iter()
            .rposition(|l| l.user.as_deref() == Some(user))
            .ok_or_else(|| format!("unknown label `{user}`"))?;
        if let Some(l) = self.f.loops.get(index).and_then(|l| l.label.clone()) {
            return Ok(l);
        }
        let name = self.fresh(&local_name_of(user));
        if let Some(l) = self.f.loops.get_mut(index) {
            l.label = Some(name.clone());
        }
        Ok(name)
    }

    /// The label a `break` needs: one when a Go `switch` stands between it and its loop.
    fn break_label(&mut self) -> Option<String> {
        let switches = self.f.switches;
        let needs = self.f.loops.last().is_some_and(|l| l.switches < switches);
        if !needs {
            return None;
        }
        if let Some(l) = self.f.loops.last().and_then(|l| l.label.clone()) {
            return Some(l);
        }
        let base = self
            .f
            .loops
            .last()
            .and_then(|l| l.user.as_deref())
            .map_or_else(|| "loop".to_string(), local_name_of);
        let name = self.fresh(&base);
        if let Some(l) = self.f.loops.last_mut() {
            l.label = Some(name.clone());
        }
        Some(name)
    }

    /// The values of `return e` in the form of the current function's result.
    fn ret_values(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Vec<Expr>, Error> {
        let (rt, form) = self.ret_form_now();
        if form == Form::Data {
            return Ok(vec![self.expr(e, out)?]);
        }
        // Constructors become the native values directly.
        match (&e.kind, form) {
            (ExprKind::Some(v), Form::OptPair) => return Ok(vec![self.expr(v, out)?, lit("true")]),
            (ExprKind::Some(v), Form::OptPtr) => {
                let x = self.expr(v, out)?;
                return Ok(vec![self.ptr_to(x)]);
            }
            (ExprKind::None, Form::OptPair) => {
                let IrType::Option(inner) = &rt else {
                    return Err("an `Option` result".to_string());
                };
                return Ok(vec![self.zero(inner)?, lit("false")]);
            }
            (ExprKind::None, Form::OptPtr) => return Ok(vec![lit("nil")]),
            (ExprKind::Ok(v), Form::ResPair) => return Ok(vec![self.expr(v, out)?, lit("nil")]),
            (ExprKind::Ok(v), Form::ResErr) => {
                let s = self.discard(v, out)?;
                out.extend(s);
                return Ok(vec![lit("nil")]);
            }
            (ExprKind::Err(v), Form::ResPair) => {
                let IrType::Result(ok, _) = &rt else {
                    return Err("a `Result` result".to_string());
                };
                let x = self.expr(v, out)?;
                return Ok(vec![self.zero(ok)?, x]);
            }
            (ExprKind::Err(v), Form::ResErr) => return Ok(vec![self.expr(v, out)?]),
            _ => {}
        }
        if let ExprKind::Call { callee, args } = &e.kind {
            let (v, from) = self.call(callee, args, &e.ty, out)?;
            return Ok(vec![self.convert(v, from, form, &e.ty)?]);
        }
        let v = self.expr(e, out)?;
        Ok(vec![self.convert(v, Form::Data, form, &e.ty)?])
    }

    /// `&x` for a value: the address of a composite literal or a variable, else a copy.
    fn ptr_to(&mut self, x: Expr) -> Expr {
        match x {
            Expr::Composite(..) | Expr::Ident(_) => addr(x),
            Expr::Unary("&", _) => {
                // A variant is already a pointer; `*T` of an interface needs a variable.
                self.rt_call("Option", "Ptr", vec![x])
            }
            other => self.rt_call("Option", "Ptr", vec![other]),
        }
    }

    /// A value of form `from` converted to form `to` (`Data` is the data form).
    fn convert(&mut self, v: Expr, from: Form, to: Form, ty: &IrType) -> Result<Expr, Error> {
        if from == to {
            return Ok(v);
        }
        Ok(match (from, to) {
            (Form::OptPair, Form::OptPtr) => self.rt_call("Option", "PtrOf", vec![v]),
            (Form::OptPtr, Form::OptPair) => self.rt_call("Option", "Deref", vec![v]),
            (_, Form::Data) => self.data_of(v, from, ty)?,
            (Form::Data, Form::OptPair | Form::ResPair) => call(sel(v, "Get"), Vec::new()),
            (Form::Data, Form::OptPtr) => call(sel(v, "Ptr"), Vec::new()),
            (Form::Data, Form::ResErr) => call(sel(v, "AsError"), Vec::new()),
            (from, to) => {
                let d = self.data_of(v, from, ty)?;
                return self.convert(d, Form::Data, to, ty);
            }
        })
    }

    /// A native result as data.
    fn data_of(&mut self, v: Expr, from: Form, ty: &IrType) -> Result<Expr, Error> {
        Ok(match from {
            Form::Data => v,
            Form::OptPair => self.rt_call("Option", "OptionOf", vec![v]),
            Form::OptPtr => self.rt_call("Option", "OptionOfPtr", vec![v]),
            Form::ResPair => {
                let IrType::Result(a, b) = ty else {
                    return Err("a `Result` result".to_string());
                };
                let targs = vec![self.ty(a)?, self.ty(b)?];
                let f = self.rt("Result", "ResultOf");
                callt(f, targs, vec![v])
            }
            Form::ResErr => {
                let IrType::Result(_, b) = ty else {
                    return Err("a `Result` result".to_string());
                };
                let targs = vec![self.ty(b)?];
                let f = self.rt("Result", "ResultOfErr");
                callt(f, targs, vec![v])
            }
        })
    }

    fn stmt(&mut self, s: &'p IrStmt, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match s {
            IrStmt::Let { local, value, .. } => self.let_(*local, value, out),
            IrStmt::LetPattern { pattern, value, .. } => {
                // `v := value; x := v.X; ...` (the pattern always matches).
                let v = self.expr(value, out)?;
                let subject = self.stable(v, "v", out);
                let mut conds = Vec::new();
                let mut bindings = Vec::new();
                self.test(pattern, subject, &value.ty, &mut conds, &mut bindings)?;
                self.bind(&bindings, Mode::Own, out)
            }
            IrStmt::Assign { target, op, value } => self.assign(target, *op, value, out),
            IrStmt::While { label, cond, body } => {
                let mut pre = Vec::new();
                let c = self.expr(cond, &mut pre)?;
                let b = self.loop_body(label, None, body)?;
                let (stmt, label) = if pre.is_empty() {
                    let cond = if c == lit("true") { None } else { Some(c) };
                    (Stmt::For(None, cond, None, b.0), b.1)
                } else {
                    pre.push(Stmt::If(None, not(c), vec![Stmt::Break(None)], None));
                    pre.extend(b.0);
                    (Stmt::For(None, None, None, pre), b.1)
                };
                out.push(labeled(stmt, label));
                Ok(())
            }
            IrStmt::ForList {
                label,
                var,
                mode,
                list,
                body,
            } => {
                if *mode == Mode::Mut {
                    let place = self.place_expr(list, out)?;
                    let items = if pure(&place) {
                        place
                    } else {
                        let t = self.fresh("items");
                        out.push(Stmt::Define(vec![t.clone()], vec![addr(place)]));
                        star(id(&t))
                    };
                    let i = self.fresh("i");
                    let name = self.local_name(*var);
                    self.f.locals.insert(*var, Local::Ptr(name.clone()));
                    let (mut b, label) = self.loop_body(label, None, body)?;
                    b.insert(
                        0,
                        Stmt::Define(
                            vec![name],
                            vec![addr(Expr::Index(Box::new(items.clone()), Box::new(id(&i))))],
                        ),
                    );
                    out.push(labeled(Stmt::Range(Some(i), None, items, b), label));
                } else {
                    let l = self.expr(list, out)?;
                    let name = if self.is_read(*var) {
                        let n = self.local_name(*var);
                        self.f.locals.insert(*var, Local::Val(n.clone()));
                        Some(n)
                    } else {
                        None
                    };
                    let (b, label) = self.loop_body(label, None, body)?;
                    let stmt = match name {
                        Some(n) => Stmt::Range(None, Some(n), l, b),
                        None => Stmt::Range(None, None, l, b),
                    };
                    out.push(labeled(stmt, label));
                }
                Ok(())
            }
            IrStmt::ForRange {
                label,
                var,
                start,
                end,
                inclusive,
                body,
            } => {
                let s = self.expr(start, out)?;
                let s = if untyped_int(&s) {
                    call(Expr::Type(int64()), vec![s])
                } else {
                    s
                };
                let e = self.expr(end, out)?;
                let e = self.stable(e, "end", out);
                let name = self.local_name(*var);
                self.f.locals.insert(*var, Local::Val(name.clone()));
                let (b, label) = self.loop_body(label, None, body)?;
                let stmt = Stmt::For(
                    Some(Box::new(Stmt::Define(vec![name.clone()], vec![s]))),
                    Some(bin(id(&name), if *inclusive { "<=" } else { "<" }, e)),
                    Some(Box::new(Stmt::Inc(id(&name), "++"))),
                    b,
                );
                out.push(labeled(stmt, label));
                Ok(())
            }
            IrStmt::Expr(e) => self.tail(e, Target::Discard, out),
        }
    }

    /// The body of a loop, with the label that `break`s inside `switch`es need.
    fn loop_body(
        &mut self,
        user: &Option<String>,
        target: Option<Target>,
        body: &'p Block,
    ) -> Result<(Vec<Stmt>, Option<String>), Error> {
        self.f.loops.push(Loop {
            label: None,
            user: user.clone(),
            switches: self.f.switches,
            target,
        });
        let mut b = Vec::new();
        let r = self.block(body, Target::Discard, &mut b);
        let l = self.f.loops.pop().and_then(|l| l.label);
        r?;
        Ok((b, l))
    }

    fn let_(
        &mut self,
        local: LocalRef,
        value: &'p IrExpr,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let ty = self.local_ty(local);
        let read = self.is_read(local);
        let assigned = self.f.assigned.contains(&local);
        if !read && !assigned {
            // Nothing reads the variable: only the value's effects remain.
            if matches!(
                value.kind,
                ExprKind::If { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Block(_)
                    | ExprKind::Loop { .. }
            ) {
                return self.tail(value, Target::Discard, out);
            }
            let s = self.discard(value, out)?;
            out.extend(s);
            return Ok(());
        }
        if matches!(
            value.kind,
            ExprKind::If { .. }
                | ExprKind::Match { .. }
                | ExprKind::Block(_)
                | ExprKind::Loop { .. }
        ) && needs_stmts(value)
        {
            let t = self.ty(&ty)?;
            let name = self.local_name(local);
            out.push(Stmt::Var(name.clone(), t, None));
            self.tail(value, Target::Assign(id(&name)), out)?;
            self.f.locals.insert(local, Local::Val(name.clone()));
            if !read {
                out.push(Stmt::Assign(vec![id("_")], "=", vec![id(&name)]));
            }
            return Ok(());
        }
        // `let x = f()?`: the result goes into the variable directly.
        if let ExprKind::Try {
            value: inner,
            coerce_error,
        } = &value.kind
        {
            if let ExprKind::Call { callee, args } = &inner.kind {
                let (v, form) = self.call(callee, args, &inner.ty, out)?;
                if matches!(form, Form::OptPair | Form::ResPair) {
                    let name = self.local_name(local);
                    self.try_native(
                        v,
                        form,
                        &inner.ty,
                        coerce_error.as_ref(),
                        Some(name.clone()),
                        out,
                    )?;
                    self.f.locals.insert(local, Local::Val(name.clone()));
                    if !read {
                        out.push(Stmt::Assign(vec![id("_")], "=", vec![id(&name)]));
                    }
                    return Ok(());
                }
                let x = self.try_native(v, form, &inner.ty, coerce_error.as_ref(), None, out)?;
                return self.bind_value(local, &ty, x, false, read, out);
            }
        }
        let v = self.expr(value, out)?;
        let coerced = matches!(value.kind, ExprKind::Coerce { .. });
        self.bind_value(local, &ty, v, coerced, read, out)
    }

    fn bind_value(
        &mut self,
        local: LocalRef,
        ty: &IrType,
        v: Expr,
        coerced: bool,
        read: bool,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let name = self.local_name(local);
        let iface = matches!(ty, IrType::Named(d, _) if self.ifaces.contains_key(d))
            || self.payload_enum(ty);
        if coerced || untyped_int(&v) || v == lit("nil") || (iface && !matches!(v, Expr::Call(..)))
        {
            let t = self.ty(ty)?;
            out.push(Stmt::Var(name.clone(), t, Some(v)));
        } else {
            out.push(Stmt::Define(vec![name.clone()], vec![v]));
        }
        self.f.locals.insert(local, Local::Val(name.clone()));
        if !read {
            out.push(Stmt::Assign(vec![id("_")], "=", vec![id(&name)]));
        }
        Ok(())
    }

    /// The expression itself if it may be evaluated twice, or a temporary holding it.
    fn stable(&mut self, e: Expr, base: &str, out: &mut Vec<Stmt>) -> Expr {
        if pure(&e) {
            return e;
        }
        let t = self.fresh(base);
        out.push(Stmt::Define(vec![t.clone()], vec![e]));
        id(&t)
    }

    // ----- places -----------------------------------------------------------------------------

    /// A Go expression for a place that can be assigned and whose address can be taken.
    fn place_expr(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Expr, Error> {
        Ok(match &e.kind {
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Val(n)) => id(&n),
                Some(Local::Ptr(n)) => star(id(&n)),
                Some(Local::Alias(p)) => p,
                None => return Err(format!("unknown local {l:?}")),
            },
            ExprKind::SelfValue => match self.f.self_local.clone() {
                Some(Local::Val(n)) => id(&n),
                Some(Local::Ptr(n)) => star(id(&n)),
                Some(Local::Alias(p)) => p,
                None => return Err("`self` outside a method".to_string()),
            },
            ExprKind::Field { base, name } => {
                let b = self.place_expr(base, out)?;
                let f = self.field_name(&base.ty, name);
                if self.field_boxed(&base.ty, name) {
                    star(sel(unstar(b), &f))
                } else {
                    sel(unstar(b), &f)
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.place_expr(base, out)?;
                let b = self.stable(b, "base", out);
                let i = self.expr(index, out)?;
                let i = self.stable(i, "key", out);
                match &base.ty {
                    IrType::Map(..) => star(call(sel(b, "Ref"), vec![i])),
                    _ => star(self.rt_call("index", "RefAt", vec![b, i])),
                }
            }
            _ => {
                // A temporary value used as a receiver.
                let v = self.expr(e, out)?;
                let t = self.fresh("tmp");
                out.push(Stmt::Define(vec![t.clone()], vec![v]));
                id(&t)
            }
        })
    }

    /// The Go name of a field of a struct or of a variant payload.
    fn field_name(&self, base: &IrType, name: &str) -> String {
        if let IrType::Named(def, _) = base {
            if let Some(s) = self.structs.get(def) {
                if let Some(f) = s.fields.iter().find(|f| f.name == name) {
                    return value_name(&f.name, f.public);
                }
            }
        }
        value_name(name, false)
    }

    fn assign(
        &mut self,
        target: &'p IrExpr,
        op: Option<BinaryOp>,
        value: &'p IrExpr,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        // `list[i] = v` and `map[k] = v` check the index or the key.
        if let ExprKind::Index { base, index } = &target.kind {
            let b = self.place_expr(base, out)?;
            let b = self.stable(b, "base", out);
            let i = self.expr(index, out)?;
            let i = self.stable(i, "key", out);
            let is_map = matches!(base.ty, IrType::Map(..));
            let v = self.expr(value, out)?;
            let v = match op {
                None => v,
                Some(op) => {
                    let current = if is_map {
                        call(sel(b.clone(), "Get"), vec![i.clone()])
                    } else {
                        self.rt_call("index", "At", vec![b.clone(), i.clone()])
                    };
                    self.binary_values(op, current, v, &target.ty)?
                }
            };
            let s = if is_map {
                call(sel(b, "Set"), vec![i, v])
            } else {
                self.rt_call("index", "SetAt", vec![b, i, v])
            };
            out.push(Stmt::Expr(s));
            return Ok(());
        }
        let place = self.place_expr(target, out)?;
        let v = self.expr(value, out)?;
        match op {
            None => out.push(Stmt::Assign(vec![place], "=", vec![v])),
            Some(op) => {
                let native = matches!(
                    (op, &target.ty),
                    (
                        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div,
                        IrType::Float
                    ) | (BinaryOp::Add, IrType::String)
                        | (
                            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor,
                            IrType::Int
                        )
                );
                if native {
                    let sym = match op {
                        BinaryOp::Add => "+=",
                        BinaryOp::Sub => "-=",
                        BinaryOp::Mul => "*=",
                        BinaryOp::Div => "/=",
                        BinaryOp::BitAnd => "&=",
                        BinaryOp::BitOr => "|=",
                        _ => "^=",
                    };
                    out.push(Stmt::Assign(vec![place], sym, vec![v]));
                } else {
                    let place = self.stable_place(place, out);
                    let nv = self.binary_values(op, place.clone(), v, &target.ty)?;
                    out.push(Stmt::Assign(vec![place], "=", vec![nv]));
                }
            }
        }
        Ok(())
    }

    /// A place that can be read and then written without evaluating it twice.
    fn stable_place(&mut self, place: Expr, out: &mut Vec<Stmt>) -> Expr {
        if pure(&place) {
            return place;
        }
        let t = self.fresh("p");
        out.push(Stmt::Define(vec![t.clone()], vec![addr(place)]));
        star(id(&t))
    }

    /// The argument for a `mut` parameter: a pointer to the place.
    fn mut_arg(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Expr, Error> {
        if let ExprKind::Index { base, index } = &e.kind {
            let b = self.place_expr(base, out)?;
            let i = self.expr(index, out)?;
            return Ok(match &base.ty {
                IrType::Map(..) => call(sel(b, "Ref"), vec![i]),
                _ => self.rt_call("index", "RefAt", vec![b, i]),
            });
        }
        let p = self.place_expr(e, out)?;
        Ok(addr(p))
    }

    // ----- expressions ------------------------------------------------------------------------

    /// An expression in the data form.
    fn expr(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Expr, Error> {
        let (v, form) = self.native(e, out)?;
        self.data_of(v, form, &e.ty)
    }

    /// Several subexpressions in order: if a later one needs statements, earlier ones are kept
    /// in temporaries so that they are still evaluated first.
    fn seq(&mut self, items: &[&'p IrExpr], out: &mut Vec<Stmt>) -> Result<Vec<Expr>, Error> {
        let mut parts: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        for e in items {
            let mut pre = Vec::new();
            let v = self.expr(e, &mut pre)?;
            parts.push((v, pre));
        }
        let last = parts.iter().rposition(|(_, pre)| !pre.is_empty());
        let mut values = Vec::new();
        for (i, (v, pre)) in parts.into_iter().enumerate() {
            out.extend(pre);
            if last.is_some_and(|l| l > i) && !pure(&v) {
                let t = self.fresh("v");
                out.push(Stmt::Define(vec![t.clone()], vec![v]));
                values.push(id(&t));
            } else {
                values.push(v);
            }
        }
        Ok(values)
    }

    /// An expression with the form of its value.
    fn native(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<(Expr, Form), Error> {
        let data = |x: Expr| Ok((x, Form::Data));
        match &e.kind {
            ExprKind::Int(v) => data(lit(&v.to_string())),
            ExprKind::Float(v) => data(float_lit(*v)?),
            ExprKind::Str(s) => data(Expr::Str(s.clone())),
            ExprKind::Bool(b) => data(lit(if *b { "true" } else { "false" })),
            ExprKind::Unit => data(unit()),
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Val(n)) => data(id(&n)),
                Some(Local::Ptr(n)) => data(star(id(&n))),
                Some(Local::Alias(p)) => data(p),
                None => Err(format!("unknown local {l:?}")),
            },
            ExprKind::SelfValue => match self.f.self_local.clone() {
                Some(Local::Val(n)) => data(id(&n)),
                Some(Local::Ptr(n)) => data(star(id(&n))),
                Some(Local::Alias(p)) => data(p),
                None => Err("`self` outside a method".to_string()),
            },
            ExprKind::FnValue(def) => data(self.fn_value(*def, &e.ty)?),
            ExprKind::Const(def) => data(self.def_expr(*def)),
            ExprKind::Call { callee, args } => self.call(callee, args, &e.ty, out),
            ExprKind::Field { base, name } => {
                let b = self.expr(base, out)?;
                let f = self.field_name(&base.ty, name);
                if self.field_boxed(&base.ty, name) {
                    data(star(sel(unstar(b), &f)))
                } else {
                    data(sel(unstar(b), &f))
                }
            }
            ExprKind::Index { base, index } => {
                let v = self.seq(&[base, index], out)?;
                let mut it = v.into_iter();
                let (b, i) = (
                    it.next().unwrap_or(lit("nil")),
                    it.next().unwrap_or(lit("0")),
                );
                match &base.ty {
                    IrType::Map(..) => data(call(sel(b, "Get"), vec![i])),
                    _ => data(self.rt_call("index", "At", vec![b, i])),
                }
            }
            ExprKind::Slice {
                base,
                start,
                end,
                inclusive,
            } => {
                let b = self.expr(base, out)?;
                let s = match start {
                    Some(s) => self.expr(s, out)?,
                    None => lit("0"),
                };
                let x = match end {
                    Some(x) => {
                        let v = self.expr(x, out)?;
                        Some(if *inclusive {
                            self.rt_call("int", "Add", vec![v, lit("1")])
                        } else {
                            v
                        })
                    }
                    None => None,
                };
                if matches!(base.ty, IrType::String) {
                    data(match x {
                        Some(x) => self.rt_call("SliceStr", "SliceStr", vec![b, s, x]),
                        None => self.rt_call("SliceStr", "SliceStrFrom", vec![b, s]),
                    })
                } else {
                    let elem = match &base.ty {
                        IrType::List(t) => (**t).clone(),
                        _ => IrType::Void,
                    };
                    let c = self.copy_fn(&elem)?;
                    data(match x {
                        Some(x) => self.rt_call("SliceList", "SliceList", vec![b, s, x, c]),
                        None => self.rt_call("SliceList", "SliceListFrom", vec![b, s, c]),
                    })
                }
            }
            ExprKind::Struct { def, fields, base } => {
                let mut items: Vec<&IrExpr> = fields.iter().map(|(_, v)| v).collect();
                if let Some(b) = base {
                    items.push(b);
                }
                let mut values = self.seq(&items, out)?;
                // `..base`: the fields that are not written are read from the base.
                let base_value = match base {
                    Some(_) => {
                        let b = values.pop().unwrap_or(Expr::Lit("nil".to_string()));
                        Some(self.stable(b, "base", out))
                    }
                    None => None,
                };
                let mut elems = Vec::new();
                if let (Some(b), Some(st)) = (&base_value, self.structs.get(def).copied()) {
                    for f in &st.fields {
                        if fields.iter().any(|(n, _)| *n == f.name) {
                            continue;
                        }
                        let name = self.field_name(&e.ty, &f.name);
                        elems.push((Some(name.clone()), sel(b.clone(), &name)));
                    }
                }
                for ((n, _), v) in fields.iter().zip(values) {
                    let f = self.field_name(&e.ty, n);
                    let v = if self.field_boxed(&e.ty, n) {
                        self.ptr_to(v)
                    } else {
                        v
                    };
                    elems.push((Some(f), v));
                }
                let ty = self.ty(&e.ty)?;
                let _ = def;
                data(Expr::Composite(ty, elems))
            }
            ExprKind::Variant { def, values } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("a variant without an enum".to_string()),
                };
                let en = self.enums.get(&parent).copied().ok_or("unknown enum")?;
                if en.variants.iter().all(|v| v.payload.is_empty()) {
                    return data(self.def_expr(*def));
                }
                let variant = en
                    .variants
                    .iter()
                    .find(|v| v.def == *def)
                    .ok_or("unknown variant")?;
                let (names, _) = self.payload(variant);
                let items: Vec<&IrExpr> = values.iter().collect();
                let vs = self.seq(&items, out)?;
                let Type::Ptr(inner) = self.variant_ty(*def, &e.ty)? else {
                    return Err("variant type".to_string());
                };
                let elems = names
                    .into_iter()
                    .zip(vs)
                    .map(|(n, v)| (Some(n), v))
                    .collect();
                data(addr(Expr::Composite(*inner, elems)))
            }
            ExprKind::Some(v) => {
                let x = self.expr(v, out)?;
                let f = self.rt("Option", "Some");
                if untyped_int(&x) || self.holds_variant(v) {
                    let IrType::Option(inner) = &e.ty else {
                        return Err("Some".to_string());
                    };
                    let t = self.ty(inner)?;
                    data(callt(f, vec![t], vec![x]))
                } else {
                    data(call(f, vec![x]))
                }
            }
            ExprKind::None => {
                let IrType::Option(inner) = &e.ty else {
                    return Err("None".to_string());
                };
                let t = self.ty(inner)?;
                let f = self.rt("Option", "None");
                data(callt(f, vec![t], Vec::new()))
            }
            ExprKind::Ok(v) | ExprKind::Err(v) => {
                let IrType::Result(a, b) = &e.ty else {
                    return Err("Ok or Err".to_string());
                };
                let x = self.expr(v, out)?;
                let targs = vec![self.ty(a)?, self.ty(b)?];
                let name = if matches!(e.kind, ExprKind::Ok(_)) {
                    "Ok"
                } else {
                    "Err"
                };
                let f = self.rt("Result", name);
                data(callt(f, targs, vec![x]))
            }
            ExprKind::List(items) => {
                let list: Vec<&IrExpr> = items.iter().collect();
                let vs = self.seq(&list, out)?;
                let t = self.ty(&e.ty)?;
                data(Expr::Composite(
                    t,
                    vs.into_iter().map(|v| (None, v)).collect(),
                ))
            }
            ExprKind::Map(entries) => {
                let mut list = Vec::new();
                for (k, v) in entries {
                    list.push(k);
                    list.push(v);
                }
                let vs = self.seq(&list, out)?;
                let IrType::Map(kt, vt) = &e.ty else {
                    return Err("map literal".to_string());
                };
                let (k, v) = (self.ty(kt)?, self.ty(vt)?);
                if vs.is_empty() {
                    let t = self.rt_type("Map", "Map", vec![k, v]);
                    return data(Expr::Composite(t, Vec::new()));
                }
                let entry = self.rt_type("Entry", "Entry", vec![k, v]);
                let mut it = vs.into_iter();
                let mut elems = Vec::new();
                while let (Some(a), Some(b)) = (it.next(), it.next()) {
                    elems.push(Expr::Composite(
                        entry.clone(),
                        vec![(Some("Key".to_string()), a), (Some("Value".to_string()), b)],
                    ));
                }
                data(self.rt_call("Map", "MapOf", elems))
            }
            ExprKind::Unary { op, operand } => {
                let v = self.expr(operand, out)?;
                data(match op {
                    UnaryOp::Not => not(v),
                    UnaryOp::BitNot => Expr::Unary("^", Box::new(v)),
                    UnaryOp::Neg => {
                        if matches!(operand.ty, IrType::Int) {
                            match v {
                                Expr::Lit(t) if !t.starts_with('-') => lit(&format!("-{t}")),
                                v => self.rt_call("int", "Neg", vec![v]),
                            }
                        } else {
                            match v {
                                // Go constants have no negative zero.
                                Expr::Lit(t) if t.trim_start_matches(['0', '.']).is_empty() => {
                                    self.std_call("math", "Copysign", vec![lit("0"), lit("-1")])
                                }
                                Expr::Lit(t) if !t.starts_with('-') => lit(&format!("-{t}")),
                                v => Expr::Unary("-", Box::new(v)),
                            }
                        }
                    }
                })
            }
            ExprKind::Binary { op, lhs, rhs } => data(self.binary(*op, lhs, rhs, out)?),
            ExprKind::Closure { params, body } => {
                let ret = match &e.ty {
                    IrType::Fn(_, r) => (**r).clone(),
                    _ => IrType::Void,
                };
                data(self.closure(params, body, &ret)?)
            }
            ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::Block(_)
            | ExprKind::Loop { .. } => {
                if let ExprKind::Block(b) = &e.kind {
                    if b.stmts.is_empty() {
                        if let Some(t) = &b.tail {
                            return self.native(t, out);
                        }
                    }
                }
                if matches!(e.ty, IrType::Void | IrType::Never) {
                    self.tail(e, Target::Discard, out)?;
                    return data(unit());
                }
                let t = self.fresh("r");
                let ty = self.ty(&e.ty)?;
                out.push(Stmt::Var(t.clone(), ty, None));
                self.tail(e, Target::Assign(id(&t)), out)?;
                data(id(&t))
            }
            ExprKind::Return(_)
            | ExprKind::Break { .. }
            | ExprKind::Continue { .. }
            | ExprKind::Panic { .. } => {
                self.tail(e, Target::Discard, out)?;
                data(unit())
            }
            ExprKind::Interpolate(parts) => {
                // `"a " + x + " b"`: the parts are strings already.
                let items: Vec<&IrExpr> = parts.iter().collect();
                let values = self.seq(&items, out)?;
                let joined = values
                    .into_iter()
                    .filter(|v| !matches!(v, Expr::Str(s) if s.is_empty()))
                    .reduce(|a, b| bin(a, "+", b))
                    .unwrap_or(Expr::Str(String::new()));
                data(joined)
            }
            ExprKind::Try {
                value,
                coerce_error,
            } => {
                let (v, form) = self.native(value, out)?;
                let x = self.try_native(v, form, &value.ty, coerce_error.as_ref(), None, out)?;
                data(x)
            }
            ExprKind::Coerce { value, to } => {
                let v = self.expr(value, out)?;
                // A built-in value used as a `Display` value gets a type with `ToString`.
                if matches!(to, IrType::Named(d, _) if Some(*d) == self.display) {
                    let wrapper = match &value.ty {
                        IrType::Int => Some("ShowInt"),
                        IrType::Float => Some("ShowFloat"),
                        IrType::Bool => Some("ShowBool"),
                        IrType::String => Some("ShowString"),
                        _ => None,
                    };
                    if let Some(w) = wrapper {
                        let t = self.rt_type("Display", w, Vec::new());
                        return data(call(Expr::Type(t), vec![v]));
                    }
                    if matches!(
                        value.ty,
                        IrType::List(_) | IrType::Map(..) | IrType::Option(_) | IrType::Result(..)
                    ) {
                        let f = self.show_fn(&value.ty)?;
                        return data(self.rt_call("Text", "NewShown", vec![v, f]));
                    }
                }
                if let IrType::Named(d, _) = &value.ty {
                    if self.ptr_recv.contains(d) && self.structs.contains_key(d) {
                        return data(self.ptr_to(v));
                    }
                    // A plain enum is a copy type and may be a constant: a pointer to a copy.
                    if self.ptr_recv.contains(d) {
                        return data(self.rt_call("Option", "Ptr", vec![v]));
                    }
                }
                data(v)
            }
            ExprKind::Copy(v) => {
                let x = self.expr(v, out)?;
                data(self.copy_expr(x, &v.ty)?)
            }
            ExprKind::Assert {
                cond,
                text,
                location,
            } => {
                let c = self.expr(cond, out)?;
                data(self.rt_call(
                    "Panic",
                    "Assert",
                    vec![c, Expr::Str(text.clone()), Expr::Str(location.clone())],
                ))
            }
        }
    }

    /// A declared function used as a value: functions with an idiomatic result are wrapped so
    /// that they return data (spec chapter 8, 4.1.4).
    fn fn_value(&mut self, def: DefRef, ty: &IrType) -> Result<Expr, Error> {
        let d = self.p.def(def).ok_or("unknown function")?.clone();
        if d.intrinsic.is_some() {
            return Err(format!("the intrinsic `{}` used as a value", d.name));
        }
        let info = self.fns.get(&def).cloned().ok_or("unknown function")?;
        if !info.generics.is_empty() || !info.owner_generics.is_empty() {
            let has_dicts = info
                .generics
                .iter()
                .chain(&info.owner_generics)
                .any(|g| !self.dicts_of(&g.bounds).is_empty());
            if has_dicts {
                return Err(format!("the generic function `{}` used as a value", d.name));
            }
        }
        let mut f = self.def_expr(def);
        if !info.generics.is_empty() {
            let mut map = BTreeMap::new();
            let pattern = IrType::Fn(info.params.clone(), Box::new(info.ret.clone()));
            unify(&pattern, ty, &mut map);
            let mut targs = Vec::new();
            for g in &info.generics {
                let t = map.get(&g.name).cloned().unwrap_or(IrType::Void);
                targs.push(self.ty(&t)?);
            }
            f = Expr::Inst(Box::new(f), targs);
        }
        let form = self.ret_form(&info.ret);
        if form == Form::Data {
            return Ok(f);
        }
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (i, (m, t)) in info.params.iter().enumerate() {
            let n = format!("p{i}");
            params.push((n.clone(), self.param_ty(*m, t)?));
            args.push(id(&n));
        }
        let c = call(f, args);
        let v = self.data_of(c, form, &info.ret)?;
        let r = self.ty(&info.ret)?;
        Ok(Expr::FuncLit(params, vec![r], vec![Stmt::Return(vec![v])]))
    }

    fn closure(
        &mut self,
        params: &'p [ir::Param],
        body: &'p Block,
        ret: &IrType,
    ) -> Result<Expr, Error> {
        self.f.rets.push((ret.clone(), Form::Data));
        let loops = std::mem::take(&mut self.f.loops);
        let switches = std::mem::replace(&mut self.f.switches, 0);
        let mut ps = Vec::new();
        for p in params {
            let n = self.local_name(p.local);
            let t = self.param_ty(p.mode, &p.ty)?;
            let l = if p.mode == Mode::Mut {
                Local::Ptr(n.clone())
            } else {
                Local::Val(n.clone())
            };
            self.f.locals.insert(p.local, l);
            ps.push((n, t));
        }
        let results = match ret {
            IrType::Void | IrType::Never => Vec::new(),
            other => vec![self.ty(other)?],
        };
        let target = if results.is_empty() {
            Target::Discard
        } else {
            Target::Return
        };
        let mut stmts = Vec::new();
        let r = self.block(body, target, &mut stmts);
        self.f.rets.pop();
        self.f.loops = loops;
        self.f.switches = switches;
        r?;
        if !results.is_empty() && !terminates(&stmts) {
            stmts.push(Stmt::Expr(call(
                id("panic"),
                vec![Expr::Str("unreachable".to_string())],
            )));
        }
        Ok(Expr::FuncLit(ps, results, stmts))
    }

    /// `value?` for a value of form `form`: the early return, and the value on success.
    fn try_native(
        &mut self,
        v: Expr,
        form: Form,
        ty: &IrType,
        coerce: Option<&IrType>,
        name: Option<String>,
        out: &mut Vec<Stmt>,
    ) -> Result<Expr, Error> {
        let _ = coerce;
        match (form, ty) {
            (Form::ResPair, IrType::Result(_, e)) => {
                let n = name.unwrap_or_else(|| self.fresh("v"));
                let err = self.err_name();
                out.push(Stmt::Define(vec![n.clone(), err.clone()], vec![v]));
                let ret = self.err_return(id(&err), e, true)?;
                out.push(Stmt::If(
                    None,
                    bin(id(&err), "!=", lit("nil")),
                    vec![Stmt::Return(ret)],
                    None,
                ));
                Ok(id(&n))
            }
            (Form::ResErr, IrType::Result(_, e)) => {
                let err = self.err_name();
                let ret = self.err_return(id(&err), e, true)?;
                out.push(Stmt::If(
                    Some(Box::new(Stmt::Define(vec![err.clone()], vec![v]))),
                    bin(id(&err), "!=", lit("nil")),
                    vec![Stmt::Return(ret)],
                    None,
                ));
                Ok(unit())
            }
            (Form::OptPair, IrType::Option(_)) => {
                let n = name.unwrap_or_else(|| self.fresh("v"));
                let ok = self.shared("ok");
                out.push(Stmt::Define(vec![n.clone(), ok.clone()], vec![v]));
                let ret = self.none_return()?;
                out.push(Stmt::If(None, not(id(&ok)), vec![Stmt::Return(ret)], None));
                Ok(id(&n))
            }
            (Form::OptPtr, IrType::Option(_)) => {
                let p = self.fresh("p");
                out.push(Stmt::Define(vec![p.clone()], vec![v]));
                let ret = self.none_return()?;
                out.push(Stmt::If(
                    None,
                    bin(id(&p), "==", lit("nil")),
                    vec![Stmt::Return(ret)],
                    None,
                ));
                Ok(star(id(&p)))
            }
            (Form::Data, IrType::Result(_, e)) => {
                let t = self.stable(v, "r", out);
                let ret = self.err_return(sel(t.clone(), "Err"), e, false)?;
                out.push(Stmt::If(
                    None,
                    not(sel(t.clone(), "Ok")),
                    vec![Stmt::Return(ret)],
                    None,
                ));
                Ok(sel(t, "Value"))
            }
            (Form::Data, IrType::Option(_)) => {
                let t = self.stable(v, "o", out);
                let ret = self.none_return()?;
                out.push(Stmt::If(
                    None,
                    not(sel(t.clone(), "Some")),
                    vec![Stmt::Return(ret)],
                    None,
                ));
                Ok(sel(t, "Value"))
            }
            _ => Err("`?` on a value that is neither `Result` nor `Option`".to_string()),
        }
    }

    /// The name of an `err` temporary. Go accepts `v, err := f()` again in the same scope as
    /// long as `v` is new, so `err` is shared unless a local of the program has that name.
    fn err_name(&mut self) -> String {
        self.shared("err")
    }

    fn shared(&mut self, name: &str) -> String {
        if self.f.user.contains(name) {
            return self.fresh(name);
        }
        self.f.used.insert(name.to_string());
        self.file.idents.insert(name.to_string());
        name.to_string()
    }

    /// `return ..., err` for `?` on an error: `err` is a Go `error` (`native`) or a value of
    /// type `e`.
    fn err_return(&mut self, err: Expr, e: &IrType, native: bool) -> Result<Vec<Expr>, Error> {
        let (rt, form) = self.ret_form_now();
        let IrType::Result(ok, target_e) = &rt else {
            return Err("`?` on a `Result` in a function that does not return one".to_string());
        };
        Ok(match form {
            Form::ResPair => vec![self.zero(ok)?, err],
            Form::ResErr => vec![err],
            _ => {
                let x = if native {
                    let t = self.ty(e)?;
                    Expr::Assert(Box::new(err), t)
                } else {
                    err
                };
                let targs = vec![self.ty(ok)?, self.ty(target_e)?];
                let f = self.rt("Result", "Err");
                vec![callt(f, targs, vec![x])]
            }
        })
    }

    fn none_return(&mut self) -> Result<Vec<Expr>, Error> {
        let (rt, form) = self.ret_form_now();
        let IrType::Option(inner) = &rt else {
            return Err("`?` on an `Option` in a function that does not return one".to_string());
        };
        Ok(match form {
            Form::OptPair => vec![self.zero(inner)?, lit("false")],
            Form::OptPtr => vec![lit("nil")],
            _ => {
                let t = self.ty(inner)?;
                let f = self.rt("Option", "None");
                vec![callt(f, vec![t], Vec::new())]
            }
        })
    }

    // ----- operators --------------------------------------------------------------------------

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &'p IrExpr,
        rhs: &'p IrExpr,
        out: &mut Vec<Stmt>,
    ) -> Result<Expr, Error> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            let l = self.expr(lhs, out)?;
            let mut pre = Vec::new();
            let r = self.expr(rhs, &mut pre)?;
            let sym = if op == BinaryOp::And { "&&" } else { "||" };
            if pre.is_empty() {
                return Ok(bin(l, sym, r));
            }
            let t = self.fresh("b");
            out.push(Stmt::Define(vec![t.clone()], vec![l]));
            pre.push(Stmt::Assign(vec![id(&t)], "=", vec![r]));
            let cond = if op == BinaryOp::And {
                id(&t)
            } else {
                not(id(&t))
            };
            out.push(Stmt::If(None, cond, pre, None));
            return Ok(id(&t));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            // `x == Status.Open` for a variant without payload: test the type.
            for (a, b) in [(lhs, rhs), (rhs, lhs)] {
                if let ExprKind::Variant { def, values } = &b.kind {
                    if values.is_empty() && self.payload_enum(&b.ty) {
                        let x = self.expr(a, out)?;
                        let t = self.variant_ty(*def, &b.ty)?;
                        let f = self.rt("Is", "Is");
                        let c = callt(f, vec![t], vec![x]);
                        return Ok(if op == BinaryOp::Eq { c } else { not(c) });
                    }
                }
            }
        }
        let v = self.seq(&[lhs, rhs], out)?;
        let mut it = v.into_iter();
        let (l, r) = (it.next().unwrap_or(lit("0")), it.next().unwrap_or(lit("0")));
        self.binary_values(op, l, r, &lhs.ty)
    }

    fn binary_values(
        &mut self,
        op: BinaryOp,
        l: Expr,
        r: Expr,
        ty: &IrType,
    ) -> Result<Expr, Error> {
        use BinaryOp as B;
        let int = matches!(ty, IrType::Int);
        Ok(match op {
            B::Add if int => self.rt_call("int", "Add", vec![l, r]),
            B::Sub if int => self.rt_call("int", "Sub", vec![l, r]),
            B::Mul if int => self.rt_call("int", "Mul", vec![l, r]),
            B::Div if int => self.rt_call("int", "Div", vec![l, r]),
            B::Rem if int => self.rt_call("int", "Rem", vec![l, r]),
            B::Shl => self.rt_call("int", "Shl", vec![l, r]),
            B::Shr => self.rt_call("int", "Shr", vec![l, r]),
            // Go evaluates constant expressions exactly (and rejects a division by zero), not in
            // the arithmetic of `float64`.
            B::Add | B::Sub | B::Mul | B::Div
                if matches!(ty, IrType::Float) && constant(&l) && constant(&r) =>
            {
                let f = match op {
                    B::Add => "FloatAdd",
                    B::Sub => "FloatSub",
                    B::Mul => "FloatMul",
                    _ => "FloatDiv",
                };
                self.rt_call("float", f, vec![l, r])
            }
            B::Add => bin(l, "+", r),
            B::Sub => bin(l, "-", r),
            B::Mul => bin(l, "*", r),
            B::Div => bin(l, "/", r),
            B::Rem => self.std_call("math", "Mod", vec![l, r]),
            B::BitAnd => bin(l, "&", r),
            B::BitOr => bin(l, "|", r),
            B::BitXor => bin(l, "^", r),
            B::Eq => self.eq_expr(l, r, ty)?,
            B::NotEq => not(self.eq_expr(l, r, ty)?),
            B::Lt => bin(l, "<", r),
            B::Le => bin(l, "<=", r),
            B::Gt => bin(l, ">", r),
            B::Ge => bin(l, ">=", r),
            B::And => bin(l, "&&", r),
            B::Or => bin(l, "||", r),
        })
    }

    // ----- calls ------------------------------------------------------------------------------

    /// Type arguments of a call of a generic function: from the parameter types against the
    /// argument types, and from the result type.
    fn infer(
        &self,
        info: &FnInfo,
        recv: Option<&IrExpr>,
        args: &[ir::Arg],
        ret: &IrType,
    ) -> BTreeMap<String, IrType> {
        let mut map = BTreeMap::new();
        if let (Some(r), Some(owner)) = (recv, info.owner) {
            if let IrType::Named(d, targs) = &r.ty {
                if *d == owner {
                    for (g, t) in info.owner_generics.iter().zip(targs) {
                        map.insert(g.name.clone(), t.clone());
                    }
                }
            }
        }
        for ((_, p), a) in info.params.iter().zip(args) {
            unify(p, &a.value.ty, &mut map);
        }
        unify(&info.ret, ret, &mut map);
        map
    }

    fn call(
        &mut self,
        callee: &'p Callee,
        args: &'p [ir::Arg],
        ty: &IrType,
        out: &mut Vec<Stmt>,
    ) -> Result<(Expr, Form), Error> {
        let (def, recv) = match callee {
            Callee::Fn(def) => (Some(*def), None),
            Callee::Method { method, recv } => (Some(*method), Some(recv.as_ref())),
            Callee::Interface { recv, .. } | Callee::Default { recv, .. } => {
                (None, Some(recv.as_ref()))
            }
            Callee::Value(_) => (None, None),
        };
        if let Some(def) = def {
            let d = self.p.def(def).ok_or("unknown function")?;
            if let Some(name) = d.intrinsic.clone() {
                return self.intrinsic(&name, recv, args, ty, out);
            }
        }
        let info = match callee {
            Callee::Fn(def) | Callee::Method { method: def, .. } => self.fns.get(def).cloned(),
            Callee::Interface {
                interface, name, ..
            }
            | Callee::Default {
                interface, name, ..
            } => self.iface_fns.get(&(*interface, name.clone())).cloned(),
            Callee::Value(f) => match &f.ty {
                IrType::Fn(params, ret) => Some(FnInfo {
                    params: params.clone(),
                    ret: (**ret).clone(),
                    generics: Vec::new(),
                    owner_generics: Vec::new(),
                    owner: None,
                    self_mode: None,
                    interface: None,
                }),
                _ => None,
            },
        }
        .ok_or("a call of an unknown function")?;
        // `compare` on a type parameter goes through its dictionary.
        if let Callee::Interface {
            interface, recv, ..
        } = callee
        {
            if Some(*interface) == self.display && !matches!(recv.ty, IrType::Named(..)) {
                let f = self.show_fn(&recv.ty)?;
                let v = self.expr(recv, out)?;
                return Ok((call(f, vec![v]), Form::Data));
            }
            if Some(*interface) == self.comparable {
                let f = self.cmp_fn(&recv.ty)?;
                let mut items = vec![recv.as_ref()];
                items.extend(args.iter().map(|a| &a.value));
                let vs = self.seq(&items, out)?;
                return Ok((call(f, vs), Form::Data));
            }
        }
        // The receiver.
        let recv_expr = match (callee, recv) {
            (_, Some(r)) => {
                let mode = info.self_mode.unwrap_or(Mode::Read);
                let payload = self.payload_enum(&r.ty);
                if mode == Mode::Mut && payload {
                    Some(self.mut_arg(r, out)?)
                } else if mode == Mode::Mut
                    || matches!(&r.ty, IrType::Named(d, _) if self.ptr_recv.contains(d))
                {
                    Some(self.place_expr(r, out)?)
                } else {
                    Some(self.expr(r, out)?)
                }
            }
            _ => None,
        };
        // Arguments in order.
        let mut values = Vec::new();
        let mut later: Vec<(usize, &'p IrExpr)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let mode = info.params.get(i).map(|(m, _)| *m).unwrap_or(Mode::Read);
            if mode == Mode::Mut {
                values.push(Some(self.mut_arg(&a.value, out)?));
            } else {
                values.push(None);
                later.push((i, &a.value));
            }
        }
        let evaluated = {
            let list: Vec<&'p IrExpr> = later.iter().map(|(_, e)| *e).collect();
            self.seq(&list, out)?
        };
        let mut evaluated = evaluated.into_iter();
        let mut argv: Vec<Expr> = values
            .into_iter()
            .map(|v| v.unwrap_or_else(|| evaluated.next().unwrap_or(lit("nil"))))
            .collect();
        // Type arguments and dictionaries.
        let map = self.infer(&info, recv, args, ty);
        let is_go_method = match callee {
            Callee::Method { method, .. } => self.is_go_method(*method),
            Callee::Interface { .. } => true,
            _ => false,
        };
        let mut dicts = Vec::new();
        let gens: Vec<ir::Generic> = if is_go_method {
            info.owner_generics
                .iter()
                .chain(&info.generics)
                .cloned()
                .collect()
        } else if info.owner.is_some() {
            let owner = info.owner_generics.clone();
            owner
                .into_iter()
                .chain(info.generics.iter().cloned())
                .collect()
        } else {
            info.generics.clone()
        };
        for g in &gens {
            for d in self.dicts_of(&g.bounds) {
                let t = map.get(&g.name).cloned().unwrap_or(IrType::Void);
                dicts.push(self.dict(d, &t)?);
            }
        }
        let type_params: Vec<ir::Generic> = if is_go_method {
            Vec::new()
        } else {
            gens.clone()
        };
        let explicit = type_params
            .iter()
            .any(|g| !info.params.iter().any(|(_, p)| mentions(p, &g.name)))
            || argv
                .iter()
                .zip(&info.params)
                .any(|(a, (_, p))| untyped_int(a) && matches!(p, IrType::Param(_)))
            || args.iter().zip(&info.params).any(|(a, (_, p))| {
                type_params.iter().any(|g| mentions(p, &g.name)) && self.holds_variant(&a.value)
            });
        let mut targs = Vec::new();
        if explicit {
            for g in &type_params {
                let t = map.get(&g.name).cloned().unwrap_or(IrType::Void);
                targs.push(self.ty(&t)?);
            }
        }
        let form = match callee {
            Callee::Value(_) => Form::Data,
            _ => self.ret_form(&info.ret),
        };
        let mut all = dicts;
        all.append(&mut argv);
        let e = match callee {
            Callee::Default {
                interface,
                name,
                recv,
                ..
            } => {
                let r = recv_expr.ok_or("a default without a receiver")?;
                let r = self.as_iface(r, &recv.ty);
                let targs = self.iface_targs(*interface, &recv.ty)?;
                let fname = self.default_fn_name(*interface, name);
                let callee = match self.qualifier(*interface) {
                    Some(path) => Expr::Qual(path, fname),
                    None => id(&fname),
                };
                let mut a = vec![r];
                a.extend(all);
                callt(callee, targs, a)
            }
            Callee::Fn(def) => {
                let f = self.def_expr(*def);
                callt(f, targs, all)
            }
            Callee::Method { method, .. } => {
                let r = recv_expr.ok_or("a method without a receiver")?;
                if is_go_method {
                    let name = self.name_of(*method);
                    call(sel(unstar(r), &name), all)
                } else if info.interface.is_some() && self.p.def(*method).is_some() && {
                    let owner = info.owner;
                    owner.is_some_and(|o| self.enums.contains_key(&o))
                } {
                    // An interface method of an enum with payloads: call the interface method.
                    let d = self.p.def(*method).ok_or("method")?;
                    let name = value_name(&d.name, true);
                    call(sel(r, &name), all)
                } else {
                    let f = self.def_expr(*method);
                    let mut a = vec![r];
                    a.extend(all);
                    callt(f, targs, a)
                }
            }
            Callee::Interface { name, .. } => {
                let r = recv_expr.ok_or("a method without a receiver")?;
                call(sel(r, &value_name(name, true)), all)
            }
            Callee::Value(f) => {
                let fv = self.expr(f, out)?;
                call(fv, all)
            }
        };
        Ok((e, form))
    }

    /// A value of type `t` as a value of an interface it implements, as a coercion makes it.
    fn as_iface(&mut self, v: Expr, t: &IrType) -> Expr {
        if let IrType::Named(d, _) = t {
            if self.ptr_recv.contains(d) && self.structs.contains_key(d) {
                return self.ptr_to(v);
            }
            if self.ptr_recv.contains(d) && !self.ifaces.contains_key(d) {
                return self.rt_call("Option", "Ptr", vec![v]);
            }
        }
        v
    }

    /// The type arguments of the interface `iface` for a value of type `t`: those of its
    /// implementation, of the interface value, or of the bound.
    fn iface_targs(&mut self, iface: DefRef, t: &IrType) -> Result<Vec<Type>, Error> {
        let args: Vec<IrType> = match t {
            IrType::Named(d, args) if *d == iface => args.clone(),
            IrType::Named(d, targs) => {
                let (generics, implements) = match (self.structs.get(d), self.enums.get(d)) {
                    (Some(s), _) => (&s.generics, &s.implements),
                    (_, Some(e)) => (&e.generics, &e.implements),
                    _ => return Ok(Vec::new()),
                };
                let mut map = BTreeMap::new();
                for (g, a) in generics.iter().zip(targs) {
                    map.insert(g.name.clone(), a.clone());
                }
                implements
                    .iter()
                    .find_map(|i| match i {
                        IrType::Named(x, a) if *x == iface => {
                            Some(a.iter().map(|t| subst(t, &map)).collect())
                        }
                        _ => None,
                    })
                    .unwrap_or_default()
            }
            IrType::SelfType => self
                .ifaces
                .get(&iface)
                .map(|i| {
                    i.generics
                        .iter()
                        .map(|g| IrType::Param(g.name.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for a in &args {
            out.push(self.ty(a)?);
        }
        Ok(out)
    }

    /// Whether a method is a Go method (and not a function of the package).
    fn is_go_method(&self, method: DefRef) -> bool {
        let Some(info) = self.fns.get(&method) else {
            return false;
        };
        let Some(owner) = info.owner else {
            return false;
        };
        if info.self_mode.is_none() || !info.generics.is_empty() {
            return false;
        }
        self.structs.contains_key(&owner)
            || self
                .enums
                .get(&owner)
                .is_some_and(|e| e.variants.iter().all(|v| v.payload.is_empty()))
    }

    // ----- intrinsics -------------------------------------------------------------------------

    /// A call of an intrinsic of std.
    fn intrinsic(
        &mut self,
        name: &str,
        recv: Option<&'p IrExpr>,
        args: &'p [ir::Arg],
        ty: &IrType,
        out: &mut Vec<Stmt>,
    ) -> Result<(Expr, Form), Error> {
        let short = name.strip_prefix("std.").unwrap_or(name);
        let mut items: Vec<&'p IrExpr> = Vec::new();
        if let Some(r) = recv {
            items.push(r);
        }
        items.extend(args.iter().map(|a| &a.value));
        let first_ty = items.first().map(|e| e.ty.clone()).unwrap_or(IrType::Void);
        let elem = match &first_ty {
            IrType::List(t) => (**t).clone(),
            IrType::Map(_, v) => (**v).clone(),
            _ => IrType::Void,
        };
        // Intrinsics that change their first argument take it as a place.
        let mutating = matches!(
            short,
            "list.List.push"
                | "list.List.pop"
                | "list.List.insert"
                | "list.List.remove"
                | "list.List.clear"
                | "list.List.append"
                | "map.Map.insert"
                | "map.Map.remove"
                | "map.Map.clear"
                | "prelude.replace"
                | "list.List.sort"
                | "list.sort_by"
        );
        let mut a: Vec<Expr> = if mutating {
            let first = items
                .first()
                .copied()
                .ok_or("an intrinsic without arguments")?;
            let p = self.place_expr(first, out)?;
            let mut rest = self.seq(items.get(1..).unwrap_or(&[]), out)?;
            rest.insert(0, p);
            rest
        } else {
            self.seq(&items, out)?
        };
        let mut arg = || {
            if a.is_empty() {
                lit("nil")
            } else {
                a.remove(0)
            }
        };
        let data = |e: Expr| Ok((e, Form::Data));
        let pair = |e: Expr| Ok((e, Form::OptPair));
        match short {
            "io.print" => data(self.std_call("fmt", "Println", vec![arg()])),
            "list.List.to_string"
            | "map.Map.to_string"
            | "prelude.Option.to_string"
            | "prelude.Result.to_string" => {
                let x = arg();
                data(self.text_expr(x, &first_ty)?)
            }
            "io.write" => data(self.std_call("fmt", "Print", vec![arg()])),
            "io.eprint" => data(self.rt_call("print", "Eprint", vec![arg()])),
            "io.read_line" => pair(self.rt_call("ReadLine", "ReadLine", Vec::new())),
            "io.args" => data(self.rt_call("process", "Args", Vec::new())),
            "process.exit" => data(self.rt_call("Exit", "Exit", vec![arg()])),
            "process.env" => pair(self.rt_call("process", "Env", vec![arg()])),
            "prelude.replace" => {
                let p = arg();
                let v = arg();
                data(self.rt_call("Replace", "Replace", vec![addr(p), v]))
            }
            "prelude.Bool.to_string" => data(self.std_call("strconv", "FormatBool", vec![arg()])),
            "prelude.error" => data(self.rt_call("TextError", "NewTextError", vec![arg()])),
            "string.String.to_string" => data(arg()),
            "math.Int.to_string" => {
                let x = arg();
                data(self.std_call("strconv", "FormatInt", vec![x, lit("10")]))
            }
            "math.Int.to_float" => data(call(Expr::Type(Type::name("float64")), vec![arg()])),
            "math.Int.abs" => data(self.rt_call("int", "IntAbs", vec![arg()])),
            "math.Int.pow" => {
                let x = arg();
                data(self.rt_call("int", "IntPow", vec![x, arg()]))
            }
            "math.Int.min" | "math.Int.max" => {
                let f = if short.ends_with("min") { "min" } else { "max" };
                let x = arg();
                data(call(id(f), vec![x, arg()]))
            }
            "math.Int.compare" => {
                let x = arg();
                data(self.rt_call("Ordering", "CompareInt", vec![x, arg()]))
            }
            "string.String.compare" => {
                let x = arg();
                data(self.rt_call("Ordering", "CompareString", vec![x, arg()]))
            }
            "math.Float.to_string" => data(self.rt_call("FloatString", "FloatString", vec![arg()])),
            "math.Float.to_int" => data(self.rt_call("float", "FloatToInt", vec![arg()])),
            "math.Float.abs" => data(self.std_call("math", "Abs", vec![arg()])),
            "math.Float.min" => {
                let x = arg();
                data(self.rt_call("float", "FloatMin", vec![x, arg()]))
            }
            "math.Float.max" => {
                let x = arg();
                data(self.rt_call("float", "FloatMax", vec![x, arg()]))
            }
            "math.Float.pow" => {
                let x = arg();
                data(self.std_call("math", "Pow", vec![x, arg()]))
            }
            "math.Float.sqrt" => data(self.std_call("math", "Sqrt", vec![arg()])),
            "math.Float.floor" => data(self.std_call("math", "Floor", vec![arg()])),
            "math.Float.ceil" => data(self.std_call("math", "Ceil", vec![arg()])),
            "math.Float.round" => data(self.std_call("math", "Round", vec![arg()])),
            "math.Float.is_nan" => data(self.std_call("math", "IsNaN", vec![arg()])),
            "string.String.len" => data(self.rt_call("StrLen", "StrLen", vec![arg()])),
            "string.String.is_empty" => data(bin(arg(), "==", Expr::Str(String::new()))),
            "string.String.contains" => {
                let s = arg();
                data(self.std_call("strings", "Contains", vec![s, arg()]))
            }
            "string.String.starts_with" => {
                let s = arg();
                data(self.std_call("strings", "HasPrefix", vec![s, arg()]))
            }
            "string.String.ends_with" => {
                let s = arg();
                data(self.std_call("strings", "HasSuffix", vec![s, arg()]))
            }
            "string.String.find" => {
                let s = arg();
                pair(self.rt_call("strings", "StrFind", vec![s, arg()]))
            }
            "string.String.split" => {
                let s = arg();
                data(self.rt_call("strings", "StrSplit", vec![s, arg()]))
            }
            "string.String.lines" => data(self.rt_call("strings", "StrLines", vec![arg()])),
            "string.String.chars" => data(self.rt_call("strings", "StrChars", vec![arg()])),
            "string.String.trim" => data(self.rt_call("strings", "StrTrim", vec![arg()])),
            "string.String.replace" => {
                let s = arg();
                let f = arg();
                data(self.rt_call("strings", "StrReplace", vec![s, f, arg()]))
            }
            "string.String.repeat" => {
                let s = arg();
                data(self.rt_call("strings", "StrRepeat", vec![s, arg()]))
            }
            "string.String.to_upper_ascii" => {
                data(self.rt_call("strings", "StrUpper", vec![arg()]))
            }
            "string.String.to_lower_ascii" => {
                data(self.rt_call("strings", "StrLower", vec![arg()]))
            }
            "string.String.parse_int" => Ok((
                self.rt_call("parse", "StringParseInt", vec![arg()]),
                Form::ResPair,
            )),
            "string.String.parse_float" => Ok((
                self.rt_call("parse", "StringParseFloat", vec![arg()]),
                Form::ResPair,
            )),
            "string.join" => {
                let parts = arg();
                data(self.std_call("strings", "Join", vec![parts, arg()]))
            }
            "string.fixed" => {
                let x = arg();
                data(self.rt_call("Format", "Fixed", vec![x, arg()]))
            }
            "string.pad" => {
                let text = arg();
                let rest = vec![text, arg(), arg(), arg(), arg()];
                data(self.rt_call("Format", "Pad", rest))
            }
            "list.List.new" => {
                let t = self.ty(ty)?;
                data(Expr::Composite(t, Vec::new()))
            }
            "list.List.len" => data(call(
                Expr::Type(int64()),
                vec![call(id("len"), vec![arg()])],
            )),
            "list.List.is_empty" => data(bin(call(id("len"), vec![arg()]), "==", lit("0"))),
            "list.List.push" => {
                let p = arg();
                let v = arg();
                let p = self.stable_place(p, out);
                out.push(Stmt::Assign(
                    vec![p.clone()],
                    "=",
                    vec![call(id("append"), vec![p, v])],
                ));
                data(unit())
            }
            "list.List.append" => {
                let p = arg();
                let v = arg();
                let p = self.stable_place(p, out);
                out.push(Stmt::Assign(
                    vec![p.clone()],
                    "=",
                    vec![Expr::Call(
                        Box::new(id("append")),
                        Vec::new(),
                        vec![p, v],
                        true,
                    )],
                ));
                data(unit())
            }
            "list.List.clear" => {
                let p = arg();
                let t = self.ty(&first_ty)?;
                out.push(Stmt::Assign(
                    vec![p],
                    "=",
                    vec![Expr::Composite(t, Vec::new())],
                ));
                data(unit())
            }
            "list.List.pop" => pair(self.rt_call("list", "Pop", vec![addr(arg())])),
            "list.List.insert" => {
                let p = arg();
                let i = arg();
                data(self.rt_call("list", "ListInsert", vec![addr(p), i, arg()]))
            }
            "list.List.remove" => {
                let p = arg();
                data(self.rt_call("list", "ListRemove", vec![addr(p), arg()]))
            }
            "list.List.swap" => {
                let p = arg();
                let i = arg();
                data(self.rt_call("list", "ListSwap", vec![p, i, arg()]))
            }
            "list.List.reverse" => data(self.std_call("slices", "Reverse", vec![arg()])),
            "list.List.map" => {
                let l = arg();
                data(self.rt_call("list", "ListMap", vec![l, arg()]))
            }
            "list.List.filter" => {
                let l = arg();
                data(self.rt_call("list", "ListFilter", vec![l, arg()]))
            }
            "list.List.any" => {
                let l = arg();
                data(self.rt_call("list", "ListAny", vec![l, arg()]))
            }
            "list.List.all" => {
                let l = arg();
                data(self.rt_call("list", "ListAll", vec![l, arg()]))
            }
            "list.List.count" => {
                let l = arg();
                data(self.rt_call("list", "ListCount", vec![l, arg()]))
            }
            "list.List.contains" => {
                let l = arg();
                let x = arg();
                let eq = self.eq_fn(&elem)?;
                data(self.rt_call("list", "ListContains", vec![l, x, eq]))
            }
            "list.List.index_of" => {
                let l = arg();
                let x = arg();
                let eq = self.eq_fn(&elem)?;
                pair(self.rt_call("list", "ListIndexOf", vec![l, x, eq]))
            }
            "list.List.get" => {
                let l = arg();
                let i = arg();
                let c = self.copy_fn(&elem)?;
                pair(self.rt_call("list", "ListGet", vec![l, i, c]))
            }
            "list.List.sort" => {
                let l = arg();
                let c = self.cmp_fn(&elem)?;
                data(self.rt_call("sort", "ListSort", vec![l, c]))
            }
            "list.sort_by" => {
                let l = arg();
                data(self.rt_call("sort", "ListSort", vec![l, arg()]))
            }
            "list.List.min" | "list.List.max" => {
                let l = arg();
                let c = self.cmp_fn(&elem)?;
                let k = self.copy_fn(&elem)?;
                let f = if short.ends_with("min") {
                    "ListMin"
                } else {
                    "ListMax"
                };
                pair(self.rt_call("sort", f, vec![l, c, k]))
            }
            "list.sum" => data(self.rt_call("list", "ListSum", vec![arg()])),
            "list.range" => {
                let x = arg();
                data(self.rt_call("list", "ListRange", vec![x, arg()]))
            }
            "map.Map.new" => {
                let t = self.ty(ty)?;
                data(Expr::Composite(t, Vec::new()))
            }
            "map.Map.len" => data(call(sel(arg(), "Len"), Vec::new())),
            "map.Map.is_empty" => data(call(sel(arg(), "IsEmpty"), Vec::new())),
            "map.Map.contains_key" => {
                let m = arg();
                data(call(sel(m, "ContainsKey"), vec![arg()]))
            }
            "map.Map.insert" => {
                let m = arg();
                let k = arg();
                pair(call(sel(m, "Insert"), vec![k, arg()]))
            }
            "map.Map.remove" => {
                let m = arg();
                pair(call(sel(m, "Remove"), vec![arg()]))
            }
            "map.Map.keys" => data(call(sel(arg(), "Keys"), Vec::new())),
            "map.Map.entries" => data(call(sel(arg(), "Entries"), Vec::new())),
            "map.Map.clear" => data(call(sel(arg(), "Clear"), Vec::new())),
            "map.get" => {
                let m = arg();
                let k = arg();
                let c = self.copy_fn(&elem)?;
                pair(self.rt_call("Map", "MapGet", vec![m, k, c]))
            }
            "map.values" => {
                let m = arg();
                let c = self.copy_fn(&elem)?;
                data(self.rt_call("Map", "MapValues", vec![m, c]))
            }
            "fs.read_text" => Ok((self.rt_call("fs", "FsReadText", vec![arg()]), Form::ResPair)),
            "fs.write_text" => {
                let p = arg();
                Ok((
                    self.rt_call("fs", "FsWriteText", vec![p, arg()]),
                    Form::ResErr,
                ))
            }
            "fs.exists" => data(self.rt_call("fs", "FsExists", vec![arg()])),
            other => Err(format!("the intrinsic `{other}` for Go")),
        }
    }

    // ----- match ------------------------------------------------------------------------------

    fn match_(
        &mut self,
        scrutinee: &'p IrExpr,
        mode: Mode,
        arms: &'p [Arm],
        target: Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        // `match f() { Some(x) => ..., None => ... }` on an idiomatic result.
        if let ExprKind::Call { callee, args } = &scrutinee.kind {
            if arms
                .iter()
                .all(|a| a.guard.is_none() && simple_arm(&a.pattern))
                && arms.len() <= 2
            {
                let mut pre = Vec::new();
                let (v, form) = self.call(callee, args, &scrutinee.ty, &mut pre)?;
                if form != Form::Data {
                    out.extend(pre);
                    return self.match_native(v, form, &scrutinee.ty, arms, target, out);
                }
                // Not idiomatic: fall through with the value already computed.
                out.extend(pre);
                return self.match_value(v, &scrutinee.ty, mode, arms, target, out);
            }
        }
        let is_place = matches!(
            scrutinee.kind,
            ExprKind::Local(_)
                | ExprKind::Field { .. }
                | ExprKind::SelfValue
                | ExprKind::Index { .. }
        );
        let subject = if mode == Mode::Mut && is_place {
            let p = self.place_expr(scrutinee, out)?;
            if pure(&p) {
                p
            } else {
                let t = self.fresh("m");
                out.push(Stmt::Define(vec![t.clone()], vec![addr(p)]));
                star(id(&t))
            }
        } else {
            self.expr(scrutinee, out)?
        };
        self.match_value(subject, &scrutinee.ty, mode, arms, target, out)
    }

    /// A `match` on an idiomatic result: `if v, ok := f(); ok { ... } else { ... }`.
    fn match_native(
        &mut self,
        v: Expr,
        form: Form,
        ty: &IrType,
        arms: &'p [Arm],
        target: Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let positive = |p: &Pattern| matches!(p, Pattern::Some(_) | Pattern::Ok(_));
        let negative = |p: &Pattern| matches!(p, Pattern::None | Pattern::Err(_));
        let yes = arms.iter().find(|a| positive(&a.pattern));
        let no = arms.iter().find(|a| negative(&a.pattern));
        let other = arms.iter().find(|a| irrefutable(&a.pattern));
        let inner = |a: Option<&'p Arm>| -> Option<&'p Pattern> {
            match a.map(|a| &a.pattern) {
                Some(Pattern::Some(p) | Pattern::Ok(p) | Pattern::Err(p)) => Some(p),
                _ => None,
            }
        };
        let bound = |p: Option<&Pattern>, this: &Self| match p {
            Some(Pattern::Bind(l)) if this.is_read(*l) || this.f.assigned.contains(l) => Some(*l),
            _ => None,
        };
        let yes_bind = bound(inner(yes), self);
        let no_bind = bound(inner(no), self);
        let yes_arm = yes.or(other);
        let no_arm = no.or(other);
        if let Some(a) = other {
            if let Pattern::Bind(l) = a.pattern {
                if self.is_read(l) {
                    return Err("a binding of a whole idiomatic result".to_string());
                }
            }
        }
        let mut then = Vec::new();
        let mut otherwise = Vec::new();
        let (init, cond) = match form {
            Form::OptPair => {
                let n = match yes_bind {
                    Some(l) => {
                        let n = self.local_name(l);
                        self.f.locals.insert(l, Local::Val(n.clone()));
                        n
                    }
                    None => "_".to_string(),
                };
                let ok = self.shared("ok");
                (Stmt::Define(vec![n, ok.clone()], vec![v]), id(&ok))
            }
            Form::OptPtr => {
                let p = self.fresh("p");
                if let Some(l) = yes_bind {
                    let n = self.local_name(l);
                    then.push(Stmt::Define(vec![n.clone()], vec![star(id(&p))]));
                    self.f.locals.insert(l, Local::Val(n));
                }
                (
                    Stmt::Define(vec![p.clone()], vec![v]),
                    bin(id(&p), "!=", lit("nil")),
                )
            }
            Form::ResPair | Form::ResErr => {
                let n = match yes_bind {
                    Some(l) if form == Form::ResPair => {
                        let n = self.local_name(l);
                        self.f.locals.insert(l, Local::Val(n.clone()));
                        n
                    }
                    _ => "_".to_string(),
                };
                let err = self.err_name();
                if let (Some(l), IrType::Result(_, e)) = (no_bind, ty) {
                    let n = self.local_name(l);
                    let t = self.ty(e)?;
                    otherwise.push(Stmt::Define(
                        vec![n.clone()],
                        vec![Expr::Assert(Box::new(id(&err)), t)],
                    ));
                    self.f.locals.insert(l, Local::Val(n));
                }
                let names = if form == Form::ResPair {
                    vec![n, err.clone()]
                } else {
                    vec![err.clone()]
                };
                (
                    Stmt::Define(names, vec![v]),
                    bin(id(&err), "==", lit("nil")),
                )
            }
            Form::Data => return Err("a native match on data".to_string()),
        };
        if let Some(a) = yes_arm {
            self.block(&a.body, target.clone(), &mut then)?;
        }
        if let Some(a) = no_arm {
            self.block(&a.body, target, &mut otherwise)?;
        }
        let otherwise = if otherwise.is_empty() {
            None
        } else {
            Some(otherwise)
        };
        out.push(Stmt::If(Some(Box::new(init)), cond, then, otherwise));
        Ok(())
    }

    /// A `match` on a value in the data form.
    fn match_value(
        &mut self,
        subject: Expr,
        ty: &IrType,
        mode: Mode,
        arms: &'p [Arm],
        target: Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let guarded = arms.iter().any(|a| a.guard.is_some());
        // A type switch for enums with payloads.
        if self.payload_enum(ty) && !guarded && arms.iter().all(|a| top_only(&a.pattern)) {
            return self.type_switch(subject, ty, mode, arms, target, out);
        }
        // A value switch for integers, strings and enums without payloads.
        let switchable = matches!(ty, IrType::Int | IrType::String) || self.plain_enum(ty);
        if switchable && !guarded && arms.iter().all(|a| value_pattern(&a.pattern)) {
            return self.value_switch(subject, ty, arms, target, out);
        }
        // An `if` chain.
        let subject = self.stable(subject, "m", out);
        let mut branches = Vec::new();
        for arm in arms {
            let alts: Vec<&Pattern> = match &arm.pattern {
                Pattern::Or(alts) if alts.iter().any(binds) => alts.iter().collect(),
                p => vec![p],
            };
            for alt in alts {
                let mut conds = Vec::new();
                let mut bindings = Vec::new();
                self.test(alt, subject.clone(), ty, &mut conds, &mut bindings)?;
                branches.push((conds, bindings, arm));
            }
        }
        let last_unguarded = arms.last().is_some_and(|a| a.guard.is_none());
        let mut chain: Option<Vec<Stmt>> = None;
        let mut first = true;
        for (conds, bindings, arm) in branches.into_iter().rev() {
            let mut conds = if first && last_unguarded {
                Vec::new()
            } else {
                conds
            };
            if let Some(g) = &arm.guard {
                // The guard sees the bindings as the places they name.
                let saved: Vec<(LocalRef, Option<Local>)> = bindings
                    .iter()
                    .map(|(l, p)| {
                        let old = self.f.locals.insert(*l, Local::Alias(p.clone()));
                        (*l, old)
                    })
                    .collect();
                let mut pre = Vec::new();
                let gv = self.expr(g, &mut pre)?;
                for (l, old) in saved {
                    match old {
                        Some(o) => self.f.locals.insert(l, o),
                        None => self.f.locals.remove(&l),
                    };
                }
                if !pre.is_empty() {
                    return Err("a guard that needs statements".to_string());
                }
                conds.push(gv);
            }
            // The body declares only the bindings it uses.
            let mut scan = Scan::default();
            scan.block(&arm.body);
            let used: Vec<(LocalRef, Expr)> = bindings
                .into_iter()
                .filter(|(l, _)| scan.reads.contains(l) || scan.assigned.contains(l))
                .collect();
            let mut b = Vec::new();
            self.bind(&used, mode, &mut b)?;
            self.block(&arm.body, target.clone(), &mut b)?;
            match and_all(conds) {
                None => chain = Some(b),
                Some(c) => {
                    let otherwise = match chain.take() {
                        Some(rest) if rest.is_empty() => None,
                        Some(rest) => Some(rest),
                        None if first && matches!(target, Target::Return) => {
                            Some(vec![Stmt::Expr(call(
                                id("panic"),
                                vec![Expr::Str("unreachable".to_string())],
                            ))])
                        }
                        None => None,
                    };
                    chain = Some(vec![Stmt::If(None, c, b, otherwise)]);
                }
            }
            first = false;
        }
        if let Some(stmts) = chain {
            out.extend(stmts);
        }
        Ok(())
    }

    fn type_switch(
        &mut self,
        subject: Expr,
        ty: &IrType,
        mode: Mode,
        arms: &'p [Arm],
        target: Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let IrType::Named(edef, _) = ty else {
            return Err("type switch".to_string());
        };
        let en = self.enums.get(edef).copied().ok_or("enum")?;
        let needs_var = arms.iter().any(|a| match &a.pattern {
            Pattern::Variant { fields, .. } => fields.iter().any(|f| match f {
                Pattern::Bind(l) => self.is_read(*l) || self.f.assigned.contains(l),
                _ => false,
            }),
            _ => false,
        });
        let var = if needs_var {
            Some(self.fresh("v"))
        } else {
            None
        };
        let mut cases = Vec::new();
        let mut has_default = false;
        self.f.switches += 1;
        let result = (|| -> Result<(), Error> {
            for arm in arms {
                let mut body = Vec::new();
                let labels = match &arm.pattern {
                    Pattern::Wild => {
                        has_default = true;
                        None
                    }
                    Pattern::Bind(l) => {
                        has_default = true;
                        self.bind(&[(*l, subject.clone())], mode, &mut body)?;
                        None
                    }
                    Pattern::Variant { def, fields } => {
                        let variant = en
                            .variants
                            .iter()
                            .find(|v| v.def == *def)
                            .ok_or("variant")?;
                        let (names, _) = self.payload(variant);
                        let mut bindings = Vec::new();
                        for (p, n) in fields.iter().zip(&names) {
                            if let (Pattern::Bind(l), Some(v)) = (p, &var) {
                                bindings.push((*l, sel(id(v), n)));
                            }
                        }
                        self.bind(&bindings, mode, &mut body)?;
                        Some(vec![self.variant_ty(*def, ty)?])
                    }
                    Pattern::Or(alts) => {
                        let mut list = Vec::new();
                        for a in alts {
                            if let Pattern::Variant { def, .. } = a {
                                list.push(self.variant_ty(*def, ty)?);
                            }
                        }
                        Some(list)
                    }
                    _ => return Err("a pattern in a type switch".to_string()),
                };
                self.block(&arm.body, target.clone(), &mut body)?;
                if labels.is_some() || !body.is_empty() {
                    cases.push((labels, body));
                }
            }
            Ok(())
        })();
        self.f.switches -= 1;
        result?;
        if !has_default && matches!(target, Target::Return) {
            cases.push((
                None,
                vec![Stmt::Expr(call(
                    id("panic"),
                    vec![Expr::Str("unreachable".to_string())],
                ))],
            ));
        }
        out.push(Stmt::TypeSwitch(var, subject, cases));
        Ok(())
    }

    fn value_switch(
        &mut self,
        subject: Expr,
        ty: &IrType,
        arms: &'p [Arm],
        target: Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let mut cases = Vec::new();
        let mut has_default = false;
        self.f.switches += 1;
        let result = (|| -> Result<(), Error> {
            for arm in arms {
                let mut body = Vec::new();
                let labels = match &arm.pattern {
                    Pattern::Wild => {
                        has_default = true;
                        None
                    }
                    Pattern::Bind(l) => {
                        has_default = true;
                        self.bind(&[(*l, subject.clone())], Mode::Read, &mut body)?;
                        None
                    }
                    p => {
                        let mut list = Vec::new();
                        self.case_values(p, &mut list)?;
                        Some(list)
                    }
                };
                self.block(&arm.body, target.clone(), &mut body)?;
                if labels.is_some() || !body.is_empty() {
                    cases.push((labels, body));
                }
            }
            Ok(())
        })();
        self.f.switches -= 1;
        result?;
        let _ = ty;
        if !has_default && matches!(target, Target::Return) {
            cases.push((
                None,
                vec![Stmt::Expr(call(
                    id("panic"),
                    vec![Expr::Str("unreachable".to_string())],
                ))],
            ));
        }
        out.push(Stmt::Switch(Some(subject), cases));
        Ok(())
    }

    fn case_values(&mut self, p: &Pattern, out: &mut Vec<Expr>) -> Result<(), Error> {
        match p {
            Pattern::Int(v) => out.push(lit(&v.to_string())),
            Pattern::Str(s) => out.push(Expr::Str(s.clone())),
            Pattern::Const(d) | Pattern::Variant { def: d, .. } => out.push(self.def_expr(*d)),
            Pattern::Or(alts) => {
                for a in alts {
                    self.case_values(a, out)?;
                }
            }
            _ => return Err("a pattern in a value switch".to_string()),
        }
        Ok(())
    }

    fn bind(
        &mut self,
        bindings: &[(LocalRef, Expr)],
        mode: Mode,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        for (local, path) in bindings {
            let read = self.is_read(*local);
            let assigned = self.f.assigned.contains(local);
            if mode == Mode::Mut {
                self.f.locals.insert(*local, Local::Alias(path.clone()));
            } else if read || assigned {
                let name = self.local_name(*local);
                out.push(Stmt::Define(vec![name.clone()], vec![path.clone()]));
                if !read {
                    out.push(Stmt::Assign(vec![id("_")], "=", vec![id(&name)]));
                }
                self.f.locals.insert(*local, Local::Val(name));
            }
        }
        Ok(())
    }

    /// The conditions under which `subject` matches `p`, and the bindings it makes.
    fn test(
        &mut self,
        p: &Pattern,
        subject: Expr,
        ty: &IrType,
        conds: &mut Vec<Expr>,
        bindings: &mut Vec<(LocalRef, Expr)>,
    ) -> Result<(), Error> {
        match p {
            Pattern::Wild => {}
            Pattern::Bind(l) => bindings.push((*l, subject)),
            Pattern::Int(v) => conds.push(bin(subject, "==", lit(&v.to_string()))),
            Pattern::Str(s) => conds.push(bin(subject, "==", Expr::Str(s.clone()))),
            Pattern::Bool(true) => conds.push(subject),
            Pattern::Bool(false) => conds.push(not(subject)),
            Pattern::Const(def) => {
                let c = self.def_expr(*def);
                conds.push(bin(subject, "==", c));
            }
            Pattern::None => conds.push(not(sel(subject, "Some"))),
            Pattern::Some(inner) => {
                let t = match ty {
                    IrType::Option(t) => (**t).clone(),
                    _ => IrType::Void,
                };
                conds.push(sel(subject.clone(), "Some"));
                self.test(inner, sel(subject, "Value"), &t, conds, bindings)?;
            }
            Pattern::Ok(inner) | Pattern::Err(inner) => {
                let (a, b) = match ty {
                    IrType::Result(a, b) => ((**a).clone(), (**b).clone()),
                    _ => (IrType::Void, IrType::Void),
                };
                let ok = matches!(p, Pattern::Ok(_));
                conds.push(if ok {
                    sel(subject.clone(), "Ok")
                } else {
                    not(sel(subject.clone(), "Ok"))
                });
                let (sub, t) = if ok {
                    (sel(subject, "Value"), a)
                } else {
                    (sel(subject, "Err"), b)
                };
                self.test(inner, sub, &t, conds, bindings)?;
            }
            Pattern::Variant { def, fields } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("a pattern of an unknown variant".to_string()),
                };
                let en = self.enums.get(&parent).copied().ok_or("unknown enum")?;
                if en.variants.iter().all(|v| v.payload.is_empty()) {
                    let c = self.def_expr(*def);
                    conds.push(bin(subject, "==", c));
                    return Ok(());
                }
                let vty = self.variant_ty(*def, ty)?;
                let f = self.rt("Is", "Is");
                conds.push(callt(f, vec![vty.clone()], vec![subject.clone()]));
                let variant = en
                    .variants
                    .iter()
                    .find(|v| v.def == *def)
                    .ok_or("variant")?;
                let map: BTreeMap<String, IrType> = match ty {
                    IrType::Named(_, args) => en
                        .generics
                        .iter()
                        .map(|g| g.name.clone())
                        .zip(args.iter().cloned())
                        .collect(),
                    _ => BTreeMap::new(),
                };
                let (names, types) = self.payload(variant);
                let asserted = Expr::Assert(Box::new(subject), vty);
                for ((sub, n), t) in fields.iter().zip(&names).zip(&types) {
                    let t = subst(t, &map);
                    self.test(sub, sel(asserted.clone(), n), &t, conds, bindings)?;
                }
            }
            Pattern::Struct { def, fields } => {
                let s = self.structs.get(def).copied().ok_or("unknown struct")?;
                let map: BTreeMap<String, IrType> = match ty {
                    IrType::Named(_, args) => s
                        .generics
                        .iter()
                        .map(|g| g.name.clone())
                        .zip(args.iter().cloned())
                        .collect(),
                    _ => BTreeMap::new(),
                };
                for (sub, f) in fields.iter().zip(&s.fields) {
                    let t = subst(&f.ty, &map);
                    let n = value_name(&f.name, f.public);
                    self.test(sub, sel(subject.clone(), &n), &t, conds, bindings)?;
                }
            }
            Pattern::Or(alts) => {
                let mut any = Vec::new();
                for alt in alts {
                    let mut c = Vec::new();
                    let mut b = Vec::new();
                    self.test(alt, subject.clone(), ty, &mut c, &mut b)?;
                    match and_all(c) {
                        Some(c) => any.push(c),
                        None => return Ok(()),
                    }
                }
                let mut it = any.into_iter();
                if let Some(first) = it.next() {
                    conds.push(it.fold(first, |acc, c| bin(acc, "||", c)));
                }
            }
        }
        Ok(())
    }
}

/// A labeled statement if the label was used.
fn labeled(s: Stmt, label: Option<String>) -> Stmt {
    match label {
        Some(l) => Stmt::Labeled(l, Box::new(s)),
        None => s,
    }
}

/// `*p` used as the base of a field: Go dereferences pointers to structs itself.
fn unstar(e: Expr) -> Expr {
    match e {
        Expr::Star(inner) if matches!(*inner, Expr::Ident(_)) => *inner,
        other => other,
    }
}

/// Whether an expression is a call that Go accepts as a statement: not a conversion and not a
/// built-in function such as `len`.
fn statement_call(e: &Expr) -> bool {
    let Expr::Call(f, _, _, _) = e else {
        return false;
    };
    match f.as_ref() {
        Expr::Type(_) => false,
        Expr::Ident(n) => !matches!(
            n.as_str(),
            "len" | "cap" | "min" | "max" | "append" | "new" | "make" | "complex" | "real" | "imag"
        ),
        _ => true,
    }
}

/// Patterns of a `match` on an idiomatic result: `Some(x)`, `None`, `Ok(x)`, `Err(x)` or `_`.
fn simple_arm(p: &Pattern) -> bool {
    match p {
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => irrefutable(i),
        Pattern::None | Pattern::Wild => true,
        _ => false,
    }
}

/// Patterns of a value `switch`.
fn value_pattern(p: &Pattern) -> bool {
    match p {
        Pattern::Int(_)
        | Pattern::Str(_)
        | Pattern::Const(_)
        | Pattern::Wild
        | Pattern::Bind(_) => true,
        Pattern::Variant { fields, .. } => fields.is_empty(),
        Pattern::Or(alts) => alts.iter().all(|a| value_pattern(a) && !irrefutable(a)),
        _ => false,
    }
}

impl Emitter<'_> {
    /// The receiver name of a type's methods: its first letter, or `self` if a parameter or a
    /// local of one of its methods has that name.
    fn receiver_name(&self, type_name: &str, methods: &[ir::Function]) -> String {
        let first: String = type_name
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase().to_string())
            .unwrap_or_else(|| "x".to_string());
        let mut names = BTreeSet::new();
        for m in methods {
            for p in &m.params {
                names.insert(local_name_of(&p.name));
            }
            if let Some(b) = &m.body {
                let mut scan = Scan::default();
                scan.block(b);
                for l in scan.reads.iter().chain(&scan.assigned) {
                    if let Some(info) = self.p.locals.get(l) {
                        names.insert(local_name_of(&info.name));
                    }
                }
            }
        }
        if names.contains(&first) || self.taken.contains(&first) {
            "self".to_string()
        } else {
            first
        }
    }
}

/// What an extern path of Go names.
/// The prefix of a path to code of the project in `native/go/` (spec chapter 7, section 8.2a).
pub(crate) const NATIVE: &str = "native:";

/// Whether a value of the Go type `nt` of a native module is already the Go form of its wolfpack
/// type.
fn go_exact(nt: &NativeTy) -> bool {
    match nt {
        NativeTy::Int(n) => n == "int64",
        NativeTy::Float(n) => n == "float64",
        NativeTy::Vec(inner) => go_exact(inner),
        NativeTy::Map { .. } | NativeTy::Record { .. } => false,
        _ => true,
    }
}

enum GoTarget {
    /// A function or type of a package: import path and name.
    Func(String, String),
    /// A method, called on the first parameter.
    Method(String),
}

/// Splits a Go path: the package is everything before the first `.` after the last `/`.
fn go_target(path: &str) -> Option<GoTarget> {
    if let Some(rest) = path.strip_prefix('(') {
        let (_, method) = rest.split_once(").")?;
        return Some(GoTarget::Method(method.to_string()));
    }
    let start = path.rfind('/').map_or(0, |i| i + 1);
    let dot = path.get(start..)?.find('.')? + start;
    let (import, rest) = (path.get(..dot)?, path.get(dot + 1..)?);
    match rest.split_once('.') {
        Some((_, method)) => Some(GoTarget::Method(method.to_string())),
        None => Some(GoTarget::Func(import.to_string(), rest.to_string())),
    }
}
