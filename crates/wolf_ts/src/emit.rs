//! IR to the TypeScript tree (spec chapter 8).
//!
//! Main decisions, all [INTERNAL] in the spec:
//!
//! - `if` and `match` are statements in TypeScript. Where they are values, the backend emits
//!   them with a *target*: the branches return, assign a temporary, or discard their values.
//! - `Option` uses the hybrid representation of ADR 0002: `T | undefined` as the result of a
//!   function whose declared result is an `Option` of anything but `Option` or `Void`, and the
//!   tagged `Option<T>` of the runtime everywhere else. Each expression has a *representation*,
//!   and conversions are inserted where they differ.
//! - A `mut` parameter of a type that JavaScript passes by value (numbers, strings, enums, `Option`
//!   ...) receives a `Ref`, an object with a `v` property; lists, maps and structs are passed as
//!   they are, because changes through the reference are visible to the caller.
//! - Bindings of `match mut` and the variable of `for x in mut` are aliases of the place they
//!   name, so that `left += days` changes the payload itself.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ir::{
    self as ir, Arm, BinaryOp, Block, Callee, CondItem, DefKind, DefRef, Expr as IrExpr, ExprKind,
    LocalRef, Mode, ModuleRole, NativeTy, Pattern, Payload, Program, Stmt as IrStmt,
    Type as IrType, UnaryOp,
};

use crate::names::{camel, type_name};
use crate::runtime;
use crate::ts::{self, ArrowBody, Expr, Item, Method, Stmt, Type};

/// A construct the backend does not support yet.
pub type Error = String;

/// How an `Option` value is represented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Repr {
    /// The runtime's `Option<T>`.
    Tagged,
    /// `T | undefined`.
    Undef,
}

/// Where the value of a block goes.
#[derive(Clone)]
enum Target {
    Discard,
    Return,
    Assign(Expr, Repr),
}

/// How a local is reached.
#[derive(Clone)]
enum Local {
    Plain(String),
    /// A `Ref`: the value is `name.v`.
    Boxed(String),
    /// Another place: a binding of `match mut` or the variable of `for x in mut`.
    Alias(Place),
}

/// A place that can be read and written.
#[derive(Clone)]
enum Place {
    /// An assignable expression: a name or a property.
    Expr(Expr),
    /// `list[i]` with a JavaScript index that is known to be in range.
    Elem(Expr, Expr),
}

/// A dictionary: the function a bound of a type parameter stands for. A payload-less enum is a
/// string at run time, so only the caller knows which `to_string` or `compare` to use, and which
/// class gives the string the methods of an interface.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Dict {
    /// `to_string` of `Display`.
    Show,
    /// `compare` of `Comparable`.
    Cmp,
    /// A user interface (see [`Emitter::view_iface`]): the value as a value of the interface, the
    /// value itself or, for a payload-less enum, its wrapper class.
    View(DefRef),
}

/// The signature facts calls need.
#[derive(Clone)]
struct FnInfo {
    params: Vec<(Mode, IrType)>,
    ret: IrType,
    /// The dictionaries it takes after the receiver, by type parameter: those of the `impl`
    /// block's bounds, then those of its own.
    dicts: Vec<(String, Dict)>,
    /// For a method: the type and its type parameters.
    owner: Option<(DefRef, Vec<String>)>,
    /// The mode of `self`, for a method.
    self_mode: Option<Mode>,
    /// The bounds of its own type parameters, by type parameter.
    bounds: Vec<(String, IrType)>,
}

/// Facts about the function being emitted.
#[derive(Default)]
struct FnState {
    locals: BTreeMap<LocalRef, Local>,
    used: BTreeSet<String>,
    temps: u32,
    /// Result types of the function and the closures around the current point, innermost last,
    /// with whether an `Option` result may be `T | undefined` (only for declared functions).
    rets: Vec<(IrType, bool)>,
    /// `mut` parameters passed by reference (lists, maps, structs).
    by_reference: BTreeSet<LocalRef>,
    /// The dictionaries in scope, by type parameter: parameters, or fields of `self` in an
    /// implementation of an interface.
    dicts: BTreeMap<(String, Dict), Expr>,
    /// The user interface bounds of the type parameters in scope, by type parameter and
    /// interface: the types of the dictionaries, and of a cast where there is no dictionary.
    bounds: BTreeMap<(String, DefRef), IrType>,
    self_expr: Option<Expr>,
    /// `self` is a `Ref` (a `mut self` method of a payload-less enum): `self = v` sets `self.v`.
    self_by_ref: bool,
    /// The loops around the current point, innermost last: the label and, for a `loop` whose
    /// value is used, where `break value` sends the value.
    loops: Vec<(Option<String>, Option<Target>)>,
}

pub(crate) struct Emitter<'p> {
    p: &'p Program,
    /// A test build: tests and test helpers are emitted, `main` is not run.
    tests: bool,
    /// Files that register the tests of a module with `node:test`.
    pub(crate) test_files: Vec<(String, String)>,
    fns: BTreeMap<DefRef, FnInfo>,
    iface_fns: BTreeMap<(DefRef, String), FnInfo>,
    /// Every interface.
    ifaces: BTreeMap<DefRef, &'p ir::Interface>,
    /// The defaults imported into the current module, by interface and name.
    imported_defaults: BTreeMap<(DefRef, String), String>,
    /// The interface whose default is being emitted: there `Self` is that interface.
    self_iface: Option<IrType>,
    /// In the declaration and the defaults of an interface that mentions `Self` beyond the
    /// receiver (see [`Emitter::self_typed`]), `Self` is the type parameter `Self`.
    self_param: bool,
    enums: BTreeMap<DefRef, &'p ir::Enum>,
    structs: BTreeMap<DefRef, &'p ir::Struct>,
    names: BTreeMap<DefRef, String>,
    /// The output path of each module, if it is emitted.
    paths: Vec<Option<String>>,
    /// Interfaces of std that need special handling.
    comparable: Option<DefRef>,
    display: Option<DefRef>,
    error: Option<DefRef>,
    markers: BTreeSet<DefRef>,
    /// Runtime pieces used.
    pub(crate) rt: BTreeSet<&'static str>,
    /// Std modules referenced, to emit.
    pub(crate) wanted: BTreeSet<usize>,
    /// Snippets of std module files that were used, by module.
    snippets: BTreeMap<usize, BTreeSet<&'static str>>,
    // State of the module being emitted.
    module: usize,
    imports: BTreeMap<usize, BTreeSet<(String, String)>>,
    imported: BTreeMap<DefRef, String>,
    taken: BTreeSet<String>,
    /// Payload-less enums used as values of an interface in the current module, with the
    /// interface type: each gets a class `Enum$Interface` that holds the value.
    wrappers: BTreeMap<(DefRef, DefRef), (IrType, IrType)>,
    externs: Vec<(String, String)>,
    f: FnState,
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

/// Whether a function result of this type is `T | undefined` (ADR 0002).
/// Whether a type mentions a type parameter.
fn generic(t: &IrType) -> bool {
    match t {
        IrType::Param(_) => true,
        IrType::List(a) | IrType::Option(a) => generic(a),
        IrType::Map(a, b) | IrType::Result(a, b) => generic(a) || generic(b),
        IrType::Named(_, args) => args.iter().any(generic),
        IrType::Fn(ps, r) => ps.iter().any(|(_, t)| generic(t)) || generic(r),
        _ => false,
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

/// Whether a type mentions `Self`.
fn mentions_self(t: &IrType) -> bool {
    match t {
        IrType::SelfType => true,
        IrType::List(a) | IrType::Option(a) => mentions_self(a),
        IrType::Map(a, b) | IrType::Result(a, b) => mentions_self(a) || mentions_self(b),
        IrType::Named(_, args) => args.iter().any(mentions_self),
        IrType::Fn(ps, r) => ps.iter().any(|(_, t)| mentions_self(t)) || mentions_self(r),
        _ => false,
    }
}

/// `t` with `Self` replaced by `with`.
fn replace_self(t: &IrType, with: &IrType) -> IrType {
    match t {
        IrType::SelfType => with.clone(),
        IrType::List(a) => IrType::List(Box::new(replace_self(a, with))),
        IrType::Option(a) => IrType::Option(Box::new(replace_self(a, with))),
        IrType::Map(a, b) => IrType::Map(
            Box::new(replace_self(a, with)),
            Box::new(replace_self(b, with)),
        ),
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

fn undef_result(t: &IrType) -> bool {
    matches!(t, IrType::Option(inner) if !matches!(**inner, IrType::Option(_) | IrType::Void))
}

/// The type inside a list or an `Option` of a native module.
fn inner_native(nt: Option<&NativeTy>) -> Option<&NativeTy> {
    match nt {
        Some(NativeTy::Vec(x) | NativeTy::Slice(x) | NativeTy::Option(x) | NativeTy::Result(x)) => {
            Some(x)
        }
        _ => None,
    }
}

fn ident(name: &str) -> Expr {
    Expr::Ident(name.to_string())
}

fn member(base: Expr, name: &str) -> Expr {
    Expr::Member(Box::new(base), name.to_string())
}

fn call(callee: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call(Box::new(callee), Vec::new(), args)
}

fn bin(a: Expr, op: &'static str, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), op, Box::new(b))
}

fn not(e: Expr) -> Expr {
    match e {
        Expr::Unary("!", inner) => *inner,
        other => Expr::Unary("!", Box::new(other)),
    }
}

/// Expressions that may be evaluated twice or out of order.
/// Whether an expression reads `this`, which the getter and setter of a `Ref` built from it
/// would not see: there `this` is the `Ref`.
fn uses_this(e: &Expr) -> bool {
    match e {
        Expr::This => true,
        Expr::Member(base, _) => uses_this(base),
        Expr::Index(a, b) => uses_this(a) || uses_this(b),
        _ => false,
    }
}

fn pure(e: &Expr) -> bool {
    match e {
        Expr::Ident(_)
        | Expr::BigInt(_)
        | Expr::Num(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Undefined
        | Expr::This => true,
        Expr::Member(base, _) => pure(base),
        _ => false,
    }
}

/// Whether emitting the expression may need statements before it.
fn needs_stmts(e: &IrExpr) -> bool {
    match &e.kind {
        ExprKind::If { .. }
        | ExprKind::IfChain { .. }
        | ExprKind::Match { .. }
        | ExprKind::Block(_)
        | ExprKind::Try { .. }
        | ExprKind::Return(_)
        | ExprKind::Loop { .. }
        | ExprKind::Break { .. }
        | ExprKind::Continue { .. } => true,
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
        ExprKind::Panic { message, .. } => needs_stmts(message),
        ExprKind::Assert { cond, .. } => needs_stmts(cond),
        _ => false,
    }
}

/// A label of the program as a TypeScript label.
fn label_name(label: &str) -> String {
    crate::names::safe(camel(label))
}

/// A loop statement with its label, if it has one.
fn labeled_loop(label: &Option<String>, stmt: Stmt) -> Stmt {
    match label {
        Some(l) => Stmt::Labeled(label_name(l), Box::new(stmt)),
        None => stmt,
    }
}

/// For a `to_string` call on an `Int`, a `Bool` or a `String`, the receiver: a template literal
/// prints it the same way.
fn plain_text<'e>(p: &Program, e: &'e IrExpr) -> Option<&'e IrExpr> {
    match &e.kind {
        ExprKind::Call {
            callee: Callee::Method { recv, method },
            args,
        } if args.is_empty()
            && matches!(recv.ty, IrType::Int | IrType::Bool | IrType::String)
            && p.def(*method).is_some_and(|d| d.name == "to_string") =>
        {
            Some(recv)
        }
        _ => None,
    }
}

/// Types JavaScript passes by reference, so that a `mut` parameter needs no `Ref`.
fn by_reference(t: &IrType, structs: &BTreeMap<DefRef, &ir::Struct>) -> bool {
    match t {
        IrType::List(_) | IrType::Map(..) => true,
        IrType::Named(d, _) => structs.contains_key(d),
        _ => false,
    }
}

impl<'p> Emitter<'p> {
    pub(crate) fn new(p: &'p Program, tests: bool) -> Emitter<'p> {
        let mut e = Emitter {
            p,
            tests,
            test_files: Vec::new(),
            fns: BTreeMap::new(),
            iface_fns: BTreeMap::new(),
            ifaces: BTreeMap::new(),
            imported_defaults: BTreeMap::new(),
            self_iface: None,
            self_param: false,
            enums: BTreeMap::new(),
            structs: BTreeMap::new(),
            names: BTreeMap::new(),
            paths: Vec::new(),
            comparable: None,
            display: None,
            error: None,
            markers: BTreeSet::new(),
            rt: BTreeSet::new(),
            wanted: BTreeSet::new(),
            snippets: BTreeMap::new(),
            module: 0,
            imports: BTreeMap::new(),
            imported: BTreeMap::new(),
            taken: BTreeSet::new(),
            wrappers: BTreeMap::new(),
            externs: Vec::new(),
            f: FnState::default(),
        };
        e.tables();
        e
    }

    fn tables(&mut self) {
        let p = self.p;
        let info = |f: &ir::Function| FnInfo {
            params: f.params.iter().map(|x| (x.mode, x.ty.clone())).collect(),
            ret: f.ret.clone(),
            dicts: Vec::new(),
            owner: None,
            self_mode: f.self_mode,
            bounds: f
                .generics
                .iter()
                .flat_map(|g| g.bounds.iter().map(|b| (g.name.clone(), b.clone())))
                .collect(),
        };
        let mut type_names: BTreeSet<String> = BTreeSet::new();
        for m in &p.modules {
            for item in &m.items {
                match item {
                    ir::Item::Struct(s) => {
                        type_names.insert(s.name.clone());
                    }
                    ir::Item::Enum(e) => {
                        type_names.insert(e.name.clone());
                    }
                    ir::Item::Interface(i) => {
                        type_names.insert(i.name.clone());
                    }
                    _ => {}
                }
            }
        }
        for m in &p.modules {
            let path = match m.role {
                ModuleRole::Std => m.path.last().map(|n| format!("src/std/{n}.ts")),
                ModuleRole::Entry | ModuleRole::Src => Some(format!("src/{}.ts", m.path.join("/"))),
                ModuleRole::Example => Some(format!("{}.ts", m.path.join("/"))),
                ModuleRole::Test if self.tests => {
                    let rest: Vec<&str> = m.path.iter().skip(1).map(String::as_str).collect();
                    Some(format!("test/{}.test.ts", rest.join("/")))
                }
                ModuleRole::Test => None,
                // `native.slug` is written next to the native code: `src/native/wolf/slug.ts`.
                ModuleRole::Native => {
                    let rest: Vec<&str> = m.path.iter().skip(1).map(String::as_str).collect();
                    Some(format!("src/native/wolf/{}.ts", rest.join("/")))
                }
            };
            self.paths.push(path);
            for item in &m.items {
                match item {
                    ir::Item::Struct(s) => {
                        self.names.insert(s.def, type_name(&s.name));
                        self.structs.insert(s.def, s);
                        for f in &s.methods {
                            self.fns.insert(f.def, info(f));
                            self.names.insert(f.def, camel(&f.name));
                        }
                    }
                    ir::Item::Enum(e) => {
                        self.names.insert(e.def, type_name(&e.name));
                        self.enums.insert(e.def, e);
                        for v in &e.variants {
                            let plain = format!("{}{}", e.name, v.name);
                            let class = if type_names.contains(&plain) {
                                format!("{}${}", e.name, v.name)
                            } else {
                                plain
                            };
                            self.names.insert(v.def, class);
                        }
                        for f in &e.methods {
                            self.fns.insert(f.def, info(f));
                            self.names
                                .insert(f.def, format!("{}${}", e.name, camel(&f.name)));
                        }
                    }
                    ir::Item::Interface(i) => {
                        self.names.insert(i.def, type_name(&i.name));
                        self.ifaces.insert(i.def, i);
                        for f in &i.methods {
                            self.iface_fns.insert((i.def, f.name.clone()), info(f));
                            if f.body.is_some() {
                                self.fns.insert(f.def, info(f));
                            }
                        }
                        if m.role == ModuleRole::Std {
                            match i.name.as_str() {
                                "Comparable" => self.comparable = Some(i.def),
                                "Display" => self.display = Some(i.def),
                                "Error" => self.error = Some(i.def),
                                "Equatable" | "Copyable" | "Hashable" => {
                                    self.markers.insert(i.def);
                                }
                                _ => {}
                            }
                        }
                    }
                    ir::Item::Fn(f) => {
                        self.fns.insert(f.def, info(f));
                        self.names.insert(f.def, camel(&f.name));
                    }
                    ir::Item::Const(c) => {
                        self.names.insert(c.def, camel(&c.name));
                    }
                    ir::Item::ExternFn(x) => {
                        self.fns.insert(
                            x.def,
                            FnInfo {
                                params: x.params.clone(),
                                ret: x.ret.clone(),
                                dicts: Vec::new(),
                                owner: None,
                                self_mode: None,
                                bounds: Vec::new(),
                            },
                        );
                        self.names.insert(x.def, camel(&x.name));
                    }
                    ir::Item::ExternType(x) => {
                        self.names.insert(x.def, type_name(&x.name));
                    }
                    ir::Item::Builtin(b) => {
                        for f in &b.methods {
                            self.fns.insert(f.def, info(f));
                        }
                    }
                    ir::Item::Test(_) => {}
                }
            }
        }
        // Dictionaries, now that `Display` and `Comparable` are known. An implementation of an
        // interface keeps the interface's signature and takes none.
        for m in &p.modules {
            for item in &m.items {
                let (owner, methods): (Option<(DefRef, &[ir::Generic])>, &[ir::Function]) =
                    match item {
                        ir::Item::Struct(s) => (Some((s.def, &s.generics)), &s.methods),
                        ir::Item::Enum(e) => (Some((e.def, &e.generics)), &e.methods),
                        ir::Item::Fn(f) => (None, std::slice::from_ref(f)),
                        // A default takes the dictionaries of its interface's bounds.
                        ir::Item::Interface(i) => {
                            let mut dicts = Vec::new();
                            if self.self_typed(i.def) {
                                dicts.push(("Self".to_string(), Dict::View(i.def)));
                            }
                            for g in &i.generics {
                                for d in self.dicts_of(&g.bounds) {
                                    dicts.push((g.name.clone(), d));
                                }
                            }
                            for f in i.methods.iter().filter(|f| f.body.is_some()) {
                                if let Some(info) = self.fns.get_mut(&f.def) {
                                    info.dicts = dicts.clone();
                                }
                            }
                            continue;
                        }
                        _ => continue,
                    };
                for f in methods {
                    let mut dicts = Vec::new();
                    if f.interface.is_none() && f.body.is_some() {
                        if let Some((_, gs)) = owner {
                            for (g, bounds) in gs.iter().zip(&f.impl_bounds) {
                                for d in self.dicts_of(bounds) {
                                    dicts.push((g.name.clone(), d));
                                }
                            }
                        }
                        for g in &f.generics {
                            for d in self.dicts_of(&g.bounds) {
                                dicts.push((g.name.clone(), d));
                            }
                        }
                    }
                    if let Some(info) = self.fns.get_mut(&f.def) {
                        info.dicts = dicts;
                        info.owner =
                            owner.map(|(d, gs)| (d, gs.iter().map(|g| g.name.clone()).collect()));
                    }
                }
            }
        }
        // Intrinsic functions have no items; their signatures come from their declarations.
        for (def, d) in &p.defs {
            if d.intrinsic.is_some() {
                self.names.entry(*def).or_insert_with(|| camel(&d.name));
            }
        }
    }

    /// The dictionaries of the bounds of a type parameter.
    fn dicts_of(&self, bounds: &[IrType]) -> Vec<Dict> {
        let mut out = Vec::new();
        for b in bounds {
            let IrType::Named(def, _) = b else { continue };
            if Some(*def) == self.display {
                out.push(Dict::Show);
            } else if Some(*def) == self.comparable {
                out.push(Dict::Cmp);
            } else if self.view_iface(*def) {
                out.push(Dict::View(*def));
            }
        }
        out
    }

    /// The name of the dictionary of the bound `d` of the type parameter `g`, as a parameter and
    /// as a field of an instance.
    fn dict_name(&self, g: &str, d: Dict) -> String {
        match d {
            Dict::Show => format!("$show{g}"),
            Dict::Cmp => format!("$cmp{g}"),
            Dict::View(i) => {
                let iface = self.p.def(i).map(|d| d.name.clone()).unwrap_or_default();
                format!("$as{iface}{g}")
            }
        }
    }

    /// The dictionaries that the instances of a generic struct or enum with payloads carry, by
    /// type parameter: those of the bounds of its declaration, when it implements an interface.
    /// An implementation keeps the interface's signature and so takes no dictionaries; it finds
    /// them in `self` (and so do the defaults it gets). The fields start with `$`, which `==`
    /// skips.
    fn type_dicts(&self, def: DefRef) -> Vec<(String, Dict)> {
        let (generics, implements) = match (self.structs.get(&def), self.enums.get(&def)) {
            (Some(s), _) => (&s.generics, &s.implements),
            (_, Some(e)) if !e.variants.iter().all(|v| v.payload.is_empty()) => {
                (&e.generics, &e.implements)
            }
            _ => return Vec::new(),
        };
        let implements_one = implements
            .iter()
            .any(|t| !matches!(t, IrType::Named(d, _) if self.markers.contains(d)));
        if !implements_one {
            return Vec::new();
        }
        let mut out = Vec::new();
        for g in generics {
            for d in self.dicts_of(&g.bounds) {
                out.push((g.name.clone(), d));
            }
        }
        out
    }

    /// The dictionaries that the implementation of `iface` by the type `def` takes from the head
    /// of its block beyond those of the type's declaration (`Bag<T: Named>: Named` with
    /// `struct Bag<T>`): not every value of the type has them, so a value gets them where its
    /// type is known, as a value of the interface ([`Self::with_head_dicts`]).
    fn head_dicts(&self, def: DefRef, iface: DefRef) -> Vec<(String, Dict)> {
        let (generics, methods) = match (self.structs.get(&def), self.enums.get(&def)) {
            (Some(s), _) => (&s.generics, &s.methods),
            (_, Some(e)) if !e.variants.iter().all(|v| v.payload.is_empty()) => {
                (&e.generics, &e.methods)
            }
            _ => return Vec::new(),
        };
        let Some(m) = methods.iter().find(|f| f.interface == Some(iface)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (g, bounds) in generics.iter().zip(&m.impl_bounds) {
            let added: Vec<IrType> = bounds
                .iter()
                .filter(|b| !g.bounds.contains(b))
                .cloned()
                .collect();
            for d in self.dicts_of(&added) {
                if !out.contains(&(g.name.clone(), d)) {
                    out.push((g.name.clone(), d));
                }
            }
        }
        out
    }

    /// The fields of [`Self::head_dicts`] of every interface the type implements, with their
    /// types: `declare`d, since they are set from outside the class.
    fn head_dict_fields(
        &mut self,
        def: DefRef,
        generics: &[ir::Generic],
        methods: &[ir::Function],
        implements: &[IrType],
    ) -> Vec<(String, Type)> {
        let mut dicts = Vec::new();
        for t in implements {
            if let IrType::Named(i, _) = t {
                for d in self.head_dicts(def, *i) {
                    if !dicts.contains(&d) {
                        dicts.push(d);
                    }
                }
            }
        }
        if dicts.is_empty() {
            return Vec::new();
        }
        // The bounds of the heads, for the types of the dictionaries of generic interfaces.
        for f in methods.iter().filter(|f| f.interface.is_some()) {
            for (g, bounds) in generics.iter().zip(&f.impl_bounds) {
                for b in bounds {
                    if let IrType::Named(i, _) = b {
                        if self.view_iface(*i) {
                            self.f.bounds.insert((g.name.clone(), *i), b.clone());
                        }
                    }
                }
            }
        }
        let declared = self.type_dicts(def);
        let mut out = Vec::new();
        for (g, d) in dicts.into_iter().filter(|x| !declared.contains(x)) {
            let ty = self.dict_ty(d, &IrType::Param(g.clone()));
            out.push((format!("declare {}", self.dict_name(&g, d)), ty));
        }
        self.f.bounds.clear();
        out
    }

    /// `v`, a value of type `t`, with the dictionaries of [`Self::head_dicts`] of its
    /// implementation of `iface`.
    fn with_head_dicts(&mut self, v: Expr, t: &IrType, iface: DefRef) -> Result<Expr, Error> {
        let IrType::Named(def, targs) = t else {
            return Ok(v);
        };
        let dicts = self.head_dicts(*def, iface);
        if dicts.is_empty() {
            return Ok(v);
        }
        let names: Vec<String> = match (self.structs.get(def), self.enums.get(def)) {
            (Some(s), _) => s.generics.iter().map(|g| g.name.clone()).collect(),
            (_, Some(e)) => e.generics.iter().map(|g| g.name.clone()).collect(),
            _ => Vec::new(),
        };
        let mut entries = Vec::new();
        for (g, d) in dicts {
            let a = names
                .iter()
                .position(|n| *n == g)
                .and_then(|i| targs.get(i))
                .cloned()
                .unwrap_or(IrType::Void);
            entries.push((self.dict_name(&g, d), self.dict(d, &a)?));
        }
        Ok(self.rt_call("withDicts", vec![v, Expr::Object(entries)]))
    }

    /// The fields of [`Self::type_dicts`] with their types, and the bounds of the type's
    /// parameters set in `self.f.bounds` for the types of the dictionaries.
    fn type_dict_fields(&mut self, def: DefRef, generics: &[ir::Generic]) -> Vec<(String, Type)> {
        for g in generics {
            for b in &g.bounds {
                if let IrType::Named(i, _) = b {
                    if self.view_iface(*i) {
                        self.f.bounds.insert((g.name.clone(), *i), b.clone());
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (g, d) in self.type_dicts(def) {
            let ty = self.dict_ty(d, &IrType::Param(g.clone()));
            out.push((self.dict_name(&g, d), ty));
        }
        self.f.bounds.clear();
        out
    }

    /// The dictionaries an instance of `t` (a struct or a variant of its enum) is built with.
    fn instance_dicts(&mut self, def: DefRef, t: &IrType) -> Result<Vec<Expr>, Error> {
        let IrType::Named(_, targs) = t else {
            return Ok(Vec::new());
        };
        let names: Vec<String> = match (self.structs.get(&def), self.enums.get(&def)) {
            (Some(s), _) => s.generics.iter().map(|g| g.name.clone()).collect(),
            (_, Some(e)) => e.generics.iter().map(|g| g.name.clone()).collect(),
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::new();
        for (g, d) in self.type_dicts(def) {
            let t = names
                .iter()
                .position(|n| *n == g)
                .and_then(|i| targs.get(i))
                .cloned()
                .unwrap_or(IrType::Void);
            out.push(self.dict(d, &t)?);
        }
        Ok(out)
    }

    /// Whether a bound gets a [`Dict::View`]: a user interface, whose methods a wrapper class
    /// gives a payload-less enum. Such a bound is no constraint of the TypeScript type parameter,
    /// which a string would not meet.
    fn view_iface(&self, def: DefRef) -> bool {
        Some(def) != self.display
            && Some(def) != self.comparable
            && !self.markers.contains(&def)
            && self.ifaces.contains_key(&def)
    }

    /// Whether a user interface mentions `Self` beyond the receiver (spec chapter 4, section 8.2,
    /// rule 3). It is a bound only (section 8.5, rule 1); its TypeScript interface takes the
    /// implementing type as its last type parameter `Self` (`class Dog implements Same<Dog>`),
    /// and its defaults are generic over `Self` and take its dictionary, so that a value of
    /// `Self` is the value itself, not its wrapper.
    fn self_typed(&self, def: DefRef) -> bool {
        self.view_iface(def)
            && self.ifaces.get(&def).is_some_and(|i| {
                i.methods
                    .iter()
                    .any(|f| f.params.iter().any(|p| mentions_self(&p.ty)) || mentions_self(&f.ret))
            })
    }

    /// The TypeScript type of the interface type `t` implemented by `self_ty`: an interface that
    /// mentions `Self` takes it as its last type argument.
    fn iface_type(&mut self, t: &IrType, self_ty: &IrType) -> Type {
        let ty = self.ty(t);
        match (t, ty) {
            (IrType::Named(d, _), Type::Ref(name, mut args)) if self.self_typed(*d) => {
                args.push(self.ty(self_ty));
                Type::Ref(name, args)
            }
            (_, ty) => ty,
        }
    }

    /// `t` with `Self` as the type parameter `Self` in a default of an interface that mentions it.
    fn norm(&self, t: &IrType) -> IrType {
        if self.self_param && mentions_self(t) {
            replace_self(t, &IrType::Param("Self".to_string()))
        } else {
            t.clone()
        }
    }

    /// The type of a struct or an enum with its own type parameters: `Pair<T>`.
    fn owner_ty(&self, owner: DefRef) -> IrType {
        let generics = match (self.structs.get(&owner), self.enums.get(&owner)) {
            (Some(s), _) => &s.generics,
            (_, Some(e)) => &e.generics,
            _ => return IrType::Named(owner, Vec::new()),
        };
        IrType::Named(
            owner,
            generics
                .iter()
                .map(|g| IrType::Param(g.name.clone()))
                .collect(),
        )
    }

    /// Whether the dictionary of a [`Dict::View`] takes the place of the value (a `Ref`) instead
    /// of the value: the interface has a `mut self` method, whose change must reach the place
    /// also when the value is a payload-less enum (its wrapper then holds the place, see
    /// [`Self::plain_view`]).
    fn place_view(&self, iface: DefRef) -> bool {
        self.ifaces
            .get(&iface)
            .is_some_and(|i| i.methods.iter().any(|f| f.self_mode == Some(Mode::Mut)))
    }

    /// The type of a dictionary for the type `t`.
    fn dict_ty(&mut self, d: Dict, t: &IrType) -> Type {
        let bound = match (d, t) {
            (Dict::View(i), IrType::Param(n)) => self.f.bounds.get(&(n.clone(), i)).cloned(),
            _ => None,
        };
        let of = t;
        let t = self.ty(t);
        match d {
            Dict::View(i) => {
                let to = bound.unwrap_or(IrType::Named(i, Vec::new()));
                let to = self.iface_type(&to, of);
                if self.place_view(i) {
                    self.rt.insert("Ref");
                    let r = Type::Ref("$rt.Ref".to_string(), vec![t]);
                    Type::Fn(vec![("r".to_string(), r)], Box::new(to))
                } else {
                    Type::Fn(vec![("x".to_string(), t)], Box::new(to))
                }
            }
            Dict::Show => Type::Fn(vec![("x".to_string(), t)], Box::new(Type::name("string"))),
            Dict::Cmp => {
                let ordering = self
                    .comparable
                    .and_then(|c| self.iface_fns.get(&(c, "compare".to_string())))
                    .map(|f| f.ret.clone())
                    .unwrap_or(IrType::Void);
                let o = self.ty(&ordering);
                Type::Fn(
                    vec![("a".to_string(), t.clone()), ("b".to_string(), t)],
                    Box::new(o),
                )
            }
        }
    }

    /// The dictionary for the type `t`, or `None` where the runtime's `display` or `compare`
    /// does the same: it calls the method of a class and handles the built-in types.
    fn dict_for(&mut self, d: Dict, t: &IrType) -> Option<Expr> {
        let t = &self.norm(t);
        match t {
            IrType::Param(n) => self.f.dicts.get(&(n.clone(), d)).cloned(),
            IrType::Named(def, _) => {
                let iface = match d {
                    Dict::Show => self.display,
                    Dict::Cmp => self.comparable,
                    Dict::View(_) => return None,
                };
                // The implementation takes dictionaries from the head of its block: the value
                // gets them first.
                if let Some(i) = iface.filter(|i| !self.head_dicts(*def, *i).is_empty()) {
                    let ty = self.ty(t);
                    let x = self.with_head_dicts(ident("x"), t, i).ok()?;
                    let (params, body) = match d {
                        Dict::Show => (
                            vec![("x".to_string(), Some(ty))],
                            self.rt_call("display", vec![x]),
                        ),
                        _ => (
                            vec![
                                ("x".to_string(), Some(ty.clone())),
                                ("y".to_string(), Some(ty)),
                            ],
                            self.rt_call("compare", vec![x, ident("y")]),
                        ),
                    };
                    return Some(Expr::Arrow(params, None, ArrowBody::Expr(Box::new(body))));
                }
                let e = self.enums.get(def)?;
                let method = e
                    .methods
                    .iter()
                    .find(|f| f.interface.is_some() && f.interface == iface)?
                    .def;
                Some(ident(&self.def_name_local(method, *def)))
            }
            _ => None,
        }
    }

    /// The dictionary for the type `t`, always.
    fn dict(&mut self, d: Dict, t: &IrType) -> Result<Expr, Error> {
        Ok(match d {
            Dict::Show => self.show_fn(t),
            Dict::Cmp => self.dict_for(d, t).unwrap_or_else(|| self.rt("compare")),
            Dict::View(i) => self.view(i, t)?,
        })
    }

    /// The dictionary of the user interface `iface` for values of type `t`: that of a type
    /// parameter, or a function that makes the interface value as a coercion does.
    fn view(&mut self, iface: DefRef, t: &IrType) -> Result<Expr, Error> {
        let t = &self.norm(t);
        if let IrType::Param(n) = t {
            if let Some(d) = self.f.dicts.get(&(n.clone(), Dict::View(iface))) {
                return Ok(d.clone());
            }
        }
        let to = match t {
            IrType::Param(n) => self.f.bounds.get(&(n.clone(), iface)).cloned(),
            _ => None,
        }
        .unwrap_or_else(|| IrType::Named(iface, self.iface_args(iface, t)));
        let ty = self.ty(t);
        if self.place_view(iface) {
            self.rt.insert("Ref");
            let r = Type::Ref("$rt.Ref".to_string(), vec![ty]);
            let body = self.view_place(ident("r"), t, &to)?;
            return Ok(Expr::Arrow(
                vec![("r".to_string(), Some(r))],
                None,
                ArrowBody::Expr(Box::new(body)),
            ));
        }
        let body = self.coerce(ident("x"), t, &to)?;
        Ok(Expr::Arrow(
            vec![("x".to_string(), Some(ty))],
            None,
            ArrowBody::Expr(Box::new(body)),
        ))
    }

    /// The value of the interface `to` for the place `r` (a `Ref`) of a value of type `from`, for
    /// an interface with `mut self` methods (see [`Self::place_view`]): the change of such a
    /// method reaches the place. A class is changed in place anyway; a payload-less enum gets a
    /// wrapper that holds the place.
    fn view_place(&mut self, r: Expr, from: &IrType, to: &IrType) -> Result<Expr, Error> {
        let from = &self.norm(from);
        if let (IrType::Param(n), IrType::Named(iface, _)) = (from, to) {
            if let Some(d) = self.f.dicts.get(&(n.clone(), Dict::View(*iface))).cloned() {
                return Ok(call(d, vec![r]));
            }
        }
        if self.plain_enum(from) {
            return Ok(self.plain_view(r, from, to));
        }
        self.coerce(member(r, "v"), from, to)
    }

    /// `to_string` of `Display` for values of type `t`, as a function; for lists, maps, options
    /// and results it is composed from the text of their parts (spec chapter 7, section 4.1).
    fn show_fn(&mut self, t: &IrType) -> Expr {
        let (piece, parts) = match t {
            IrType::List(e) => ("listText", vec![self.part_fn(e)]),
            IrType::Map(k, v) => ("mapText", vec![self.part_fn(k), self.part_fn(v)]),
            IrType::Option(e) => ("optionText", vec![self.part_fn(e)]),
            IrType::Result(v, e) => ("resultText", vec![self.part_fn(v), self.err_fn(e)]),
            _ => {
                return self
                    .dict_for(Dict::Show, t)
                    .unwrap_or_else(|| self.rt("display"));
            }
        };
        let ty = self.ty(t);
        let mut args = vec![ident("x")];
        args.extend(parts);
        let body = self.rt_call(piece, args);
        Expr::Arrow(
            vec![("x".to_string(), Some(ty))],
            None,
            ArrowBody::Expr(Box::new(body)),
        )
    }

    /// The text of a part of a collection: a `String` is quoted; in generic code the runtime
    /// tells a `String` from the dictionary of `T`.
    fn part_fn(&mut self, t: &IrType) -> Expr {
        match t {
            IrType::String => self.rt("quote"),
            IrType::Param(_) => {
                let show = self.show_fn(t);
                self.rt_call("part", vec![show])
            }
            _ => self.show_fn(t),
        }
    }

    /// The text of the error of a `Result`: by `to_string` if its type is `Display`, else by
    /// `message()`.
    fn err_fn(&mut self, t: &IrType) -> Expr {
        match t {
            IrType::Param(n) if self.f.dicts.contains_key(&(n.clone(), Dict::Show)) => {
                self.part_fn(t)
            }
            // The dictionary of `Error` gives a payload-less enum its class.
            IrType::Param(n) => {
                let view = self
                    .error
                    .and_then(|e| self.f.dicts.get(&(n.clone(), Dict::View(e))).cloned());
                match view {
                    Some(d) => {
                        let ty = self.ty(t);
                        let body = self.rt_call("errText", vec![call(d, vec![ident("x")])]);
                        Expr::Arrow(
                            vec![("x".to_string(), Some(ty))],
                            None,
                            ArrowBody::Expr(Box::new(body)),
                        )
                    }
                    None => self.rt("errText"),
                }
            }
            IrType::Named(def, _) if self.ifaces_of(*def).contains(&self.display) => {
                self.show_fn(t)
            }
            IrType::Named(def, _) => {
                let message = self.enums.get(def).and_then(|e| {
                    e.methods
                        .iter()
                        .find(|f| f.interface.is_some() && f.name == "message")
                        .map(|f| f.def)
                });
                match message {
                    Some(m) => ident(&self.def_name_local(m, *def)),
                    None => self.rt("errText"),
                }
            }
            _ => self.part_fn(t),
        }
    }

    /// The interfaces that a struct or an enum implements.
    fn ifaces_of(&self, def: DefRef) -> Vec<Option<DefRef>> {
        let list = self
            .structs
            .get(&def)
            .map(|s| &s.implements)
            .or_else(|| self.enums.get(&def).map(|e| &e.implements));
        list.into_iter()
            .flatten()
            .map(|t| match t {
                IrType::Named(i, _) => Some(*i),
                _ => None,
            })
            .collect()
    }

    fn rt(&mut self, name: &'static str) -> Expr {
        self.rt.insert(name);
        member(ident("$rt"), name)
    }

    fn rt_call(&mut self, name: &'static str, args: Vec<Expr>) -> Expr {
        let f = self.rt(name);
        call(f, args)
    }

    // ----- names and imports ------------------------------------------------------------------

    fn def_module(&self, def: DefRef) -> Option<usize> {
        self.p.def(def).map(|d| d.module)
    }

    /// The name of a top-level declaration in the current module, importing it if needed.
    fn def_name(&mut self, def: DefRef) -> String {
        let name = self
            .names
            .get(&def)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let Some(module) = self.def_module(def) else {
            return name;
        };
        // Variant classes and enum functions live in the module of their enum.
        if module == self.module {
            return name;
        }
        if let Some(alias) = self.imported.get(&def) {
            return alias.clone();
        }
        // The name must not be hidden by a local of the function being emitted either.
        let mut alias = name.clone();
        let mut n = 1;
        while self.taken.contains(&alias) || self.f.used.contains(&alias) {
            alias = format!("{name}$m{n}");
            n += 1;
        }
        self.taken.insert(alias.clone());
        self.imported.insert(def, alias.clone());
        self.imports
            .entry(module)
            .or_default()
            .insert((name, alias.clone()));
        if self
            .p
            .modules
            .get(module)
            .is_some_and(|m| m.role == ModuleRole::Std)
        {
            self.wanted.insert(module);
        }
        alias
    }

    fn local_name(&mut self, local: LocalRef) -> String {
        let base = self
            .p
            .locals
            .get(&local)
            .map(|l| camel(&l.name))
            .unwrap_or_else(|| "x".to_string());
        let mut name = base.clone();
        let mut n = 2;
        while self.f.used.contains(&name) || self.taken.contains(&name) {
            name = format!("{base}${n}");
            n += 1;
        }
        self.f.used.insert(name.clone());
        name
    }

    fn temp(&mut self, base: &str) -> String {
        self.f.temps += 1;
        if self.f.temps == 1 {
            format!("${base}")
        } else {
            format!("${base}{}", self.f.temps)
        }
    }

    fn local_ty(&self, local: LocalRef) -> IrType {
        self.p
            .locals
            .get(&local)
            .map(|l| l.ty.clone())
            .unwrap_or(IrType::Void)
    }

    // ----- types ------------------------------------------------------------------------------

    /// A type in a data position (fields, parameters, locals, type arguments).
    fn ty(&mut self, t: &IrType) -> Type {
        match t {
            IrType::Int => Type::name("bigint"),
            IrType::Float => Type::name("number"),
            IrType::Bool => Type::name("boolean"),
            IrType::String => Type::name("string"),
            IrType::Void => Type::name("undefined"),
            IrType::Never => Type::name("never"),
            IrType::List(inner) => Type::Array(Box::new(self.ty(inner))),
            IrType::Map(k, v) => Type::Ref("Map".to_string(), vec![self.ty(k), self.ty(v)]),
            IrType::Option(inner) => {
                self.rt.insert("Option");
                Type::Ref("$rt.Option".to_string(), vec![self.ty(inner)])
            }
            IrType::Result(a, b) => {
                self.rt.insert("Result");
                Type::Ref("$rt.Result".to_string(), vec![self.ty(a), self.ty(b)])
            }
            IrType::Named(def, args) => {
                let name = self.def_name(*def);
                let args = args.iter().map(|a| self.ty(a)).collect();
                Type::Ref(name, args)
            }
            IrType::Fn(params, ret) => {
                let params = params
                    .iter()
                    .enumerate()
                    .map(|(i, (m, t))| (format!("p{i}"), self.param_ty(*m, t)))
                    .collect();
                let ret = match ret.as_ref() {
                    IrType::Void => Type::name("void"),
                    other => self.ty(other),
                };
                Type::Fn(params, Box::new(ret))
            }
            IrType::Param(n) => Type::name(n),
            IrType::SelfType if self.self_param => Type::name("Self"),
            IrType::SelfType => match self.self_iface.clone() {
                // In a default, `Self` is the interface.
                Some(t) => self.ty(&t),
                None => Type::name("this"),
            },
        }
    }

    /// A function result type.
    fn ret_ty(&mut self, t: &IrType) -> Type {
        match t {
            IrType::Void => Type::name("void"),
            IrType::Option(inner) if undef_result(t) => {
                Type::Union(vec![self.ty(inner), Type::name("undefined")])
            }
            _ => self.ty(t),
        }
    }

    fn boxed(&self, mode: Mode, t: &IrType) -> bool {
        mode == Mode::Mut && !by_reference(t, &self.structs)
    }

    fn param_ty(&mut self, mode: Mode, t: &IrType) -> Type {
        let inner = self.ty(t);
        if self.boxed(mode, t) {
            self.rt.insert("Ref");
            Type::Ref("$rt.Ref".to_string(), vec![inner])
        } else {
            inner
        }
    }

    fn generics(&mut self, generics: &[ir::Generic]) -> Vec<(String, Option<Type>)> {
        generics
            .iter()
            .map(|g| {
                let mut parts = Vec::new();
                for b in &g.bounds {
                    let IrType::Named(def, _) = b else {
                        continue;
                    };
                    if self.markers.contains(def) || self.view_iface(*def) {
                        continue;
                    }
                    if Some(*def) == self.comparable {
                        // `Int` and `String` are `Comparable` too.
                        let c = self.ty(b);
                        parts.push(Type::Union(vec![
                            Type::name("bigint"),
                            Type::name("string"),
                            c,
                        ]));
                    } else {
                        parts.push(self.ty(b));
                    }
                }
                let constraint = match parts.len() {
                    0 => None,
                    1 => parts.pop(),
                    _ => {
                        let text: Vec<String> =
                            parts.iter().map(|p| format!("({})", ts::ty(p))).collect();
                        Some(Type::name(&text.join(" & ")))
                    }
                };
                (g.name.clone(), constraint)
            })
            .collect()
    }

    /// Whether a value is built from a variant of an enum with payloads, whose TypeScript type is
    /// the variant's class.
    fn holds_variant(&self, e: &IrExpr) -> bool {
        match &e.kind {
            ExprKind::Variant { .. } => !self.plain_enum(&e.ty),
            ExprKind::Some(v) | ExprKind::Ok(v) | ExprKind::Err(v) => self.holds_variant(v),
            ExprKind::List(items) => items.iter().any(|v| self.holds_variant(v)),
            ExprKind::Map(entries) => entries
                .iter()
                .any(|(k, v)| self.holds_variant(k) || self.holds_variant(v)),
            _ => false,
        }
    }

    /// Whether `method` takes `mut self` of a payload-less enum: its receiver is a string in
    /// TypeScript, so the method takes a `Ref` to the place of the receiver.
    fn plain_mut_self(&self, method: DefRef) -> bool {
        let Some(DefKind::Method { owner, .. }) = self.p.def(method).map(|d| d.kind) else {
            return false;
        };
        self.enums.get(&owner).is_some_and(|e| {
            e.variants.iter().all(|v| v.payload.is_empty())
                && e.methods
                    .iter()
                    .any(|f| f.def == method && f.self_mode == Some(Mode::Mut))
        })
    }

    /// Whether a type is a payload-less enum (a union of string literals in TypeScript).
    fn plain_enum(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if self.enums.get(d).is_some_and(|e| e.variants.iter().all(|v| v.payload.is_empty())))
    }

    /// Types whose `==` is `===` and whose copy is the value itself.
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

    // ----- modules ----------------------------------------------------------------------------

    /// Emits one module as a file.
    pub(crate) fn module(&mut self, index: usize) -> Result<Option<(String, String)>, Error> {
        let p = self.p;
        let Some(m) = p.modules.get(index) else {
            return Ok(None);
        };
        let Some(path) = self.paths.get(index).cloned().flatten() else {
            return Ok(None);
        };
        self.module = index;
        self.imports.clear();
        self.imported.clear();
        self.imported_defaults.clear();
        self.externs.clear();
        self.wrappers.clear();
        self.taken = m
            .items
            .iter()
            .filter_map(|it| match it {
                ir::Item::Struct(s) => self.names.get(&s.def).cloned(),
                ir::Item::Enum(e) => self.names.get(&e.def).cloned(),
                ir::Item::Interface(i) => self.names.get(&i.def).cloned(),
                ir::Item::Fn(f) => self.names.get(&f.def).cloned(),
                ir::Item::Const(c) => self.names.get(&c.def).cloned(),
                ir::Item::ExternFn(x) => self.names.get(&x.def).cloned(),
                _ => None,
            })
            .collect();
        let mut items = Vec::new();
        for item in &m.items {
            self.item(m, item, &mut items)?;
        }
        for ((e, iface), (from, to)) in std::mem::take(&mut self.wrappers) {
            let class = self.plain_wrapper(e, iface, &from, &to)?;
            items.push(class);
        }
        if m.role == ModuleRole::Std {
            self.std_snippets(index, &mut items);
        }
        let tests: Vec<&ir::Test> = m
            .items
            .iter()
            .filter_map(|it| match it {
                ir::Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        if self.tests && !tests.is_empty() {
            items.push(self.tests_fn(m, &tests)?);
            if m.role == ModuleRole::Test {
                items.push(Item::Raw(REGISTER.to_string()));
            } else {
                let own = path.rsplit('/').next().unwrap_or("").replace(".ts", ".js");
                let test_path = path.replace(".ts", ".test.ts");
                let text = format!(
                    "// Generated by wolf from {}. Do not edit.\n\nimport {{ test }} from \"node:test\";\nimport {{ $tests }} from \"./{own}\";\n\n{REGISTER}\n",
                    m.file
                );
                self.test_files.push((test_path, text));
            }
        }
        // `export { ... } from` for the re-exports of `src/lib.wolf`.
        let mut reexports: BTreeMap<usize, Vec<(String, Option<String>)>> = BTreeMap::new();
        for r in m.reexports.iter().filter(|_| m.role != ModuleRole::Std) {
            if let (Some(module), Some(name)) = (self.def_module(r.def), self.names.get(&r.def)) {
                let alias = if *name == camel(&r.name) || *name == r.name {
                    None
                } else {
                    Some(camel(&r.name))
                };
                reexports
                    .entry(module)
                    .or_default()
                    .push((name.clone(), alias));
            }
        }
        for (module, names) in reexports {
            if let Some(target) = self.paths.get(module).cloned().flatten() {
                items.push(Item::ExportFrom(names, relative(&path, &target)));
            }
        }
        let is_main = m
            .items
            .iter()
            .any(|it| matches!(it, ir::Item::Fn(f) if f.name == "main"))
            && matches!(m.role, ModuleRole::Entry | ModuleRole::Example)
            && (m.role == ModuleRole::Example || self.p.kind == ir::ProgramKind::Cli)
            && !self.tests;
        if is_main {
            let result = m.items.iter().any(|it| {
                matches!(it, ir::Item::Fn(f) if f.name == "main" && matches!(f.ret, IrType::Result(..)))
            });
            let runner = if result { "runMainResult" } else { "runMain" };
            let run = self.rt_call(runner, vec![ident("main")]);
            items.push(Item::Stmt(Stmt::Expr(run)));
        }
        // Imports first: other modules, then the runtime.
        let mut head = Vec::new();
        for (module, names) in std::mem::take(&mut self.imports) {
            let Some(target) = self.paths.get(module).cloned().flatten() else {
                continue;
            };
            let list = names
                .into_iter()
                .map(|(n, a)| if n == a { (n, None) } else { (n, Some(a)) })
                .collect();
            head.push(Item::Import(list, relative(&path, &target)));
        }
        if self.tests && m.role == ModuleRole::Test && !tests.is_empty() {
            head.push(Item::Import(
                vec![("test".to_string(), None)],
                "node:test".to_string(),
            ));
        }
        for (name, from) in std::mem::take(&mut self.externs) {
            // Code of `native/ts/` is copied to `src/native/` (spec chapter 8, section 8a).
            let from = match from.strip_prefix("native:") {
                Some(file) => relative(&path, &format!("src/native/{file}.ts")),
                None => from,
            };
            head.push(Item::Import(vec![(name, None)], from));
        }
        let uses_rt = items_use_rt(&items);
        if uses_rt {
            head.push(Item::ImportAll(
                "$rt".to_string(),
                relative(&path, "src/wolf_rt.ts"),
            ));
        }
        head.extend(items);
        let header = format!("Generated by wolf from {}. Do not edit.", m.file);
        let text = ts::print(&ts::File {
            header,
            items: head,
        });
        Ok(Some((path, text)))
    }

    fn item(
        &mut self,
        m: &'p ir::Module,
        item: &'p ir::Item,
        out: &mut Vec<Item>,
    ) -> Result<(), Error> {
        match item {
            ir::Item::Struct(s) => self.struct_item(s, out),
            ir::Item::Enum(e) => self.enum_item(e, out),
            ir::Item::Interface(i) => {
                if self.markers.contains(&i.def) {
                    return Ok(());
                }
                let self_typed = self.self_typed(i.def);
                let mut generics = self.generics(&i.generics);
                if self_typed {
                    generics.push(("Self".to_string(), None));
                }
                let outer_param = std::mem::replace(&mut self.self_param, self_typed);
                let mut methods = Vec::new();
                for f in &i.methods {
                    let params = f
                        .params
                        .iter()
                        .map(|p| (camel(&p.name), self.param_ty(p.mode, &p.ty)))
                        .collect();
                    let ret = self.ret_ty(&f.ret);
                    methods.push((f.docs.clone(), camel(&f.name), params, ret));
                }
                // Super-interfaces are extended (spec chapter 8, section 5.3), so that a value of
                // the interface has their methods. `Comparable` has no interface of its own.
                let mut extends = Vec::new();
                for s in &i.supers {
                    if !matches!(s, IrType::Named(d, _) if Some(*d) == self.comparable) {
                        extends.push(self.ty(s));
                    }
                }
                out.push(Item::Interface {
                    docs: i.docs.clone(),
                    export: true,
                    name: type_name(&i.name),
                    generics: generics.clone(),
                    extends,
                    methods,
                });
                // Each default is a function that takes the interface value; a class that gets
                // the default has a method that calls it (spec chapter 4, section 8.2, rule 4).
                let iface_ty = IrType::Named(
                    i.def,
                    i.generics
                        .iter()
                        .map(|g| IrType::Param(g.name.clone()))
                        .collect(),
                );
                for f in i.methods.iter().filter(|f| f.body.is_some()) {
                    // With `Self`, `self` is a value of the type parameter `Self`, and a `Ref` to
                    // it for `mut self`.
                    let by_ref = self_typed && f.self_mode == Some(Mode::Mut);
                    let this = if by_ref {
                        member(ident("self"), "v")
                    } else {
                        ident("self")
                    };
                    let outer = self.self_iface.replace(iface_ty.clone());
                    let r = self.function_with(f, Some(this), by_ref);
                    self.self_iface = outer;
                    let (mut g, mut params, ret, body) = match r {
                        Ok(r) => r,
                        Err(e) => {
                            self.self_param = outer_param;
                            return Err(e);
                        }
                    };
                    let t = if !self_typed {
                        self.ty(&iface_ty)
                    } else if by_ref {
                        self.rt.insert("Ref");
                        Type::Ref("$rt.Ref".to_string(), vec![Type::name("Self")])
                    } else {
                        Type::name("Self")
                    };
                    params.insert(0, ("self".to_string(), t));
                    let mut all = generics.clone();
                    all.append(&mut g);
                    out.push(Item::Function {
                        docs: f.docs.clone(),
                        export: true,
                        name: format!("{}${}", type_name(&i.name), camel(&f.name)),
                        generics: all,
                        params,
                        ret,
                        body,
                    });
                }
                self.self_param = outer_param;
                Ok(())
            }
            ir::Item::Fn(f) if f.in_tests && !self.tests => Ok(()),
            ir::Item::Fn(f) => {
                if f.body.is_none() {
                    return Ok(());
                }
                let name = camel(&f.name);
                let (generics, params, ret, body) = self.function(f, None)?;
                out.push(Item::Function {
                    docs: f.docs.clone(),
                    export: f.public && m.role != ModuleRole::Std
                        || (m.role == ModuleRole::Std && f.public),
                    name,
                    generics,
                    params,
                    ret,
                    body,
                });
                Ok(())
            }
            ir::Item::Const(c) => {
                let ty = self.ty(&c.ty);
                let value = match &c.value {
                    ir::ConstValue::Int(v) => Expr::BigInt(*v),
                    ir::ConstValue::Float(v) => float_lit(*v),
                    ir::ConstValue::Bool(v) => Expr::Bool(*v),
                    ir::ConstValue::Str(s) => Expr::Str(s.clone()),
                };
                out.push(Item::Const {
                    docs: c.docs.clone(),
                    export: c.public,
                    name: camel(&c.name),
                    ty,
                    value,
                });
                Ok(())
            }
            ir::Item::ExternFn(x) => self.extern_fn(x, out),
            ir::Item::ExternType(x) => {
                self.extern_type(x, out);
                Ok(())
            }
            ir::Item::Test(_) | ir::Item::Builtin(_) => Ok(()),
        }
    }

    /// `export function $tests()`: the tests of a module with their names and bodies.
    fn tests_fn(&mut self, m: &'p ir::Module, tests: &[&'p ir::Test]) -> Result<Item, Error> {
        let mut list = Vec::new();
        for t in tests {
            let saved = std::mem::take(&mut self.f);
            self.f.rets.push((IrType::Void, true));
            let mut body = Vec::new();
            self.block(&t.body, Target::Discard, &mut body)?;
            self.f = saved;
            let run = Expr::Arrow(Vec::new(), Some(Type::name("void")), ArrowBody::Block(body));
            list.push(Expr::Object(vec![
                (
                    "name".to_string(),
                    Expr::Str(format!("{}: {}", m.file, t.name)),
                ),
                ("run".to_string(), run),
            ]));
        }
        self.rt.insert("Test");
        Ok(Item::Function {
            docs: vec!["The tests of this module, for `node:test`.".to_string()],
            export: true,
            name: "$tests".to_string(),
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::Array(Box::new(Type::name("$rt.Test"))),
            body: vec![Stmt::Return(Some(Expr::Array(list)))],
        })
    }

    fn struct_item(&mut self, s: &'p ir::Struct, out: &mut Vec<Item>) -> Result<(), Error> {
        let generics = self.generics(&s.generics);
        let self_ty = self.owner_ty(s.def);
        let implements = self.implements(&s.implements, &self_ty);
        let fields: Vec<(String, Type, Option<Expr>)> = s
            .fields
            .iter()
            .map(|f| (camel(&f.name), self.ty(&f.ty), None))
            .collect();
        let object = Type::Object(
            fields
                .iter()
                .map(|(n, t, _)| (n.clone(), t.clone()))
                .collect(),
        );
        let mut body: Vec<Stmt> = fields
            .iter()
            .map(|(n, _, _)| Stmt::Assign(member(Expr::This, n), member(ident("fields"), n)))
            .collect();
        let mut fields = fields;
        let mut ctor = vec![("fields".to_string(), object)];
        for (n, t) in self.type_dict_fields(s.def, &s.generics) {
            body.push(Stmt::Assign(member(Expr::This, &n), ident(&n)));
            fields.push((n.clone(), t.clone(), None));
            ctor.push((n, t));
        }
        for (n, t) in self.head_dict_fields(s.def, &s.generics, &s.methods, &s.implements) {
            fields.push((n, t, None));
        }
        let mut methods = Vec::new();
        for f in &s.methods {
            if f.body.is_none() {
                continue;
            }
            let this = f.self_mode.map(|_| Expr::This);
            let (mut g, mut params, ret, body) = self.function(f, this)?;
            // A method of a block for one instantiation (spec chapter 4, section 8.3, rule 8)
            // says which instantiation `this` is: `sum(this: Stack<bigint>)`.
            if let (Some(args), Some(_)) = (&f.instance, f.self_mode) {
                let t = self.ty(&IrType::Named(s.def, args.clone()));
                params.insert(0, ("this".to_string(), t));
            }
            // Static members cannot use the class's type parameters: they get their own.
            if f.self_mode.is_none() {
                let mut all = instance_generics(&generics, &f.instance);
                all.append(&mut g);
                g = all;
            }
            methods.push(Method {
                docs: f.docs.clone(),
                is_static: f.self_mode.is_none(),
                name: camel(&f.name),
                generics: g
                    .into_iter()
                    .map(|(n, c)| match c {
                        Some(c) => format!("{n} extends {}", ts::ty(&c)),
                        None => n,
                    })
                    .collect(),
                params,
                ret,
                body,
            });
        }
        for (iface, args, f) in self.inherited(&s.implements, &s.methods) {
            methods.push(self.delegate(Expr::This, s.def, iface, &args, f)?);
        }
        out.push(Item::Class {
            docs: s.docs.clone(),
            export: true,
            name: type_name(&s.name),
            generics,
            implements,
            fields,
            constructor: Some((ctor, body)),
            methods,
        });
        Ok(())
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

    /// Binds a type parameter that only a bound of another one names, `T` in
    /// `fn show<T: Named, H: Holder<T>>(h: H)`, from the interface's arguments of the other.
    fn unify_bounds(&self, bounds: &[(String, IrType)], map: &mut BTreeMap<String, IrType>) {
        for _ in 0..bounds.len() {
            let before = map.len();
            for (g, b) in bounds {
                let (Some(actual), IrType::Named(i, bargs)) = (map.get(g).cloned(), b) else {
                    continue;
                };
                for (p, a) in bargs.iter().zip(self.iface_args(*i, &actual)) {
                    unify(p, &a, map);
                }
            }
            if map.len() == before {
                break;
            }
        }
    }

    /// The type arguments of the interface `iface` for a value of type `t`.
    fn iface_args(&self, iface: DefRef, t: &IrType) -> Vec<IrType> {
        match t {
            IrType::Named(d, args) if *d == iface => args.clone(),
            // A type parameter: the arguments of its bound.
            IrType::Param(n) => match self.f.bounds.get(&(n.clone(), iface)) {
                Some(IrType::Named(_, args)) => args.clone(),
                _ => Vec::new(),
            },
            // `self` in a default of the interface.
            IrType::SelfType => match &self.self_iface {
                Some(IrType::Named(d, args)) if *d == iface => args.clone(),
                _ => Vec::new(),
            },
            IrType::Named(d, targs) => {
                let (generics, implements) = match (self.structs.get(d), self.enums.get(d)) {
                    (Some(s), _) => (&s.generics, &s.implements),
                    (_, Some(e)) => (&e.generics, &e.implements),
                    _ => return Vec::new(),
                };
                let map: BTreeMap<String, IrType> = generics
                    .iter()
                    .map(|g| g.name.clone())
                    .zip(targs.iter().cloned())
                    .collect();
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
            _ => Vec::new(),
        }
    }

    /// The function of the default `name` of the interface `iface`, imported from the module
    /// of the interface if needed.
    fn default_fn(&mut self, iface: DefRef, name: &str) -> Expr {
        let base = self.names.get(&iface).cloned().unwrap_or_default();
        let local = format!("{base}${}", camel(name));
        let Some(module) = self.def_module(iface).filter(|m| *m != self.module) else {
            return ident(&local);
        };
        if let Some(alias) = self.imported_defaults.get(&(iface, name.to_string())) {
            return ident(alias);
        }
        let mut alias = local.clone();
        let mut n = 1;
        while self.taken.contains(&alias) || self.f.used.contains(&alias) {
            alias = format!("{local}$m{n}");
            n += 1;
        }
        self.taken.insert(alias.clone());
        self.imported_defaults
            .insert((iface, name.to_string()), alias.clone());
        self.imports
            .entry(module)
            .or_default()
            .insert((local, alias.clone()));
        ident(&alias)
    }

    /// A method that calls the default `f` of `iface` on `recv`, a value of the type `owner`.
    fn delegate(
        &mut self,
        recv: Expr,
        owner: DefRef,
        iface: DefRef,
        args: &[IrType],
        f: &ir::Function,
    ) -> Result<Method, Error> {
        let names: Vec<String> = self
            .ifaces
            .get(&iface)
            .map(|i| i.generics.iter().map(|g| g.name.clone()).collect())
            .unwrap_or_default();
        let mut map: BTreeMap<String, IrType> =
            names.into_iter().zip(args.iter().cloned()).collect();
        let self_ty = self.owner_ty(owner);
        let params: Vec<(String, Type)> = f
            .params
            .iter()
            .map(|p| {
                let t = replace_self(&subst(&p.ty, &map), &self_ty);
                (camel(&p.name), self.param_ty(p.mode, &t))
            })
            .collect();
        // The dictionaries of the type's bounds are fields of the receiver.
        let saved = std::mem::take(&mut self.f.dicts);
        let mut own = self.type_dicts(owner);
        own.extend(self.head_dicts(owner, iface));
        for (g, d) in own {
            let name = self.dict_name(&g, d);
            self.f.dicts.insert((g, d), member(recv.clone(), &name));
        }
        map.insert("Self".to_string(), self_ty.clone());
        let dicts = self.default_dicts(f, &map);
        self.f.dicts = saved;
        // The default of an interface that mentions `Self` takes the value itself: the string of a
        // payload-less enum (its wrapper is the place for `mut self`), the object of a class (in a
        // place of its own for `mut self`, whose new value the object then takes).
        let mut before = Vec::new();
        let mut after = Vec::new();
        let receiver = if !self.self_typed(iface) {
            recv
        } else if self.plain_enum(&self_ty) {
            if f.self_mode == Some(Mode::Mut) {
                recv
            } else {
                member(recv, "v")
            }
        } else if f.self_mode == Some(Mode::Mut) {
            let place = Expr::Object(vec![("v".to_string(), recv.clone())]);
            before.push(Stmt::Const("$self".to_string(), None, place));
            let back = self.rt_call("become", vec![recv, member(ident("$self"), "v")]);
            after.push(Stmt::Expr(back));
            ident("$self")
        } else {
            recv
        };
        let mut all = vec![receiver];
        all.extend(dicts?);
        all.extend(params.iter().map(|(n, _)| ident(n)));
        let result = call(self.default_fn(iface, &f.name), all);
        let ret = replace_self(&subst(&f.ret, &map), &self_ty);
        let mut body = before;
        if matches!(ret, IrType::Void) {
            body.push(Stmt::Expr(result));
            body.extend(after);
        } else if after.is_empty() {
            body.push(Stmt::Return(Some(result)));
        } else {
            body.push(Stmt::Const("$result".to_string(), None, result));
            body.extend(after);
            body.push(Stmt::Return(Some(ident("$result"))));
        }
        Ok(Method {
            docs: Vec::new(),
            is_static: false,
            name: camel(&f.name),
            generics: Vec::new(),
            params,
            ret: self.ret_ty(&ret),
            body,
        })
    }

    /// The dictionaries that the default `f` takes after the receiver (those of the bounds of
    /// its interface's type parameters), for the type arguments `map` of the interface.
    fn default_dicts(
        &mut self,
        f: &ir::Function,
        map: &BTreeMap<String, IrType>,
    ) -> Result<Vec<Expr>, Error> {
        let dicts = self
            .fns
            .get(&f.def)
            .map(|i| i.dicts.clone())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (g, d) in dicts {
            let t = map.get(&g).cloned().unwrap_or(IrType::Void);
            out.push(self.dict(d, &t)?);
        }
        Ok(out)
    }

    /// Implemented interfaces, without the derived ones.
    fn implements(&mut self, list: &[IrType], self_ty: &IrType) -> Vec<Type> {
        let kept: Vec<&IrType> = list
            .iter()
            .filter(|t| !matches!(t, IrType::Named(d, _) if self.markers.contains(d)))
            .collect();
        kept.into_iter()
            .map(|t| self.iface_type(t, self_ty))
            .collect()
    }

    fn enum_item(&mut self, e: &'p ir::Enum, out: &mut Vec<Item>) -> Result<(), Error> {
        let generics = self.generics(&e.generics);
        let gnames: Vec<String> = e.generics.iter().map(|g| g.name.clone()).collect();
        let gtypes: Vec<Type> = gnames.iter().map(|n| Type::name(n)).collect();
        let plain = e.variants.iter().all(|v| v.payload.is_empty());
        if plain {
            out.push(Item::TypeAlias {
                docs: e.docs.clone(),
                export: true,
                name: type_name(&e.name),
                generics: gnames.clone(),
                ty: Type::Union(
                    e.variants
                        .iter()
                        .map(|v| Type::Lit(v.name.clone()))
                        .collect(),
                ),
            });
        } else {
            let classes: Vec<Type> = e
                .variants
                .iter()
                .map(|v| {
                    Type::Ref(
                        self.names.get(&v.def).cloned().unwrap_or_default(),
                        gtypes.clone(),
                    )
                })
                .collect();
            out.push(Item::TypeAlias {
                docs: e.docs.clone(),
                export: true,
                name: type_name(&e.name),
                generics: gnames.clone(),
                ty: Type::Union(classes),
            });
            // The built-in implementations of std (`Display` of `Option`) have no body: their
            // text is composed where the type is known, and the classes do not carry them.
            let intrinsic: Vec<DefRef> = e
                .methods
                .iter()
                .filter(|f| f.body.is_none())
                .filter_map(|f| f.interface)
                .collect();
            let declared: Vec<IrType> = e
                .implements
                .iter()
                .filter(|t| !matches!(t, IrType::Named(i, _) if intrinsic.contains(i)))
                .cloned()
                .collect();
            let self_ty = self.owner_ty(e.def);
            let implements = self.implements(&declared, &self_ty);
            let enum_ty = Type::Ref(type_name(&e.name), gtypes.clone());
            for v in &e.variants {
                let class = self.names.get(&v.def).cloned().unwrap_or_default();
                let mut fields = vec![(
                    "kind".to_string(),
                    Type::Lit(v.name.clone()),
                    Some(Expr::Str(v.name.clone())),
                )];
                let mut params = Vec::new();
                let names: Vec<String> = match &v.payload {
                    Payload::None => Vec::new(),
                    Payload::Tuple(types) => (0..types.len()).map(|i| format!("_{i}")).collect(),
                    Payload::Struct(fs) => fs.iter().map(|f| camel(&f.name)).collect(),
                };
                let types: Vec<IrType> = match &v.payload {
                    Payload::None => Vec::new(),
                    Payload::Tuple(types) => types.clone(),
                    Payload::Struct(fs) => fs.iter().map(|f| f.ty.clone()).collect(),
                };
                for (n, t) in names.iter().zip(&types) {
                    let t = self.ty(t);
                    fields.push((n.clone(), t.clone(), None));
                    params.push((n.clone(), t));
                }
                let mut body: Vec<Stmt> = names
                    .iter()
                    .map(|n| Stmt::Assign(member(Expr::This, n), ident(n)))
                    .collect();
                for (n, t) in self.type_dict_fields(e.def, &e.generics) {
                    body.push(Stmt::Assign(member(Expr::This, &n), ident(&n)));
                    fields.push((n.clone(), t.clone(), None));
                    params.push((n, t));
                }
                for (n, t) in self.head_dict_fields(e.def, &e.generics, &e.methods, &e.implements) {
                    fields.push((n, t, None));
                }
                // Interface methods delegate to the enum's functions.
                let mut methods = Vec::new();
                for f in e
                    .methods
                    .iter()
                    .filter(|f| f.interface.is_some() && f.self_mode.is_some() && f.body.is_some())
                {
                    let fname = self.names.get(&f.def).cloned().unwrap_or_default();
                    let ps: Vec<(String, Type)> = f
                        .params
                        .iter()
                        .map(|p| (camel(&p.name), self.param_ty(p.mode, &p.ty)))
                        .collect();
                    let mut args = vec![Expr::This];
                    args.extend(ps.iter().map(|(n, _)| ident(n)));
                    let ret = self.ret_ty(&f.ret);
                    let value = call(ident(&fname), args);
                    methods.push(Method {
                        docs: Vec::new(),
                        is_static: false,
                        name: camel(&f.name),
                        generics: Vec::new(),
                        params: ps,
                        ret,
                        body: vec![Stmt::Return(Some(value))],
                    });
                }
                for (iface, args, f) in self.inherited(&e.implements, &e.methods) {
                    methods.push(self.delegate(Expr::This, e.def, iface, &args, f)?);
                }
                out.push(Item::Class {
                    docs: v.docs.clone(),
                    export: true,
                    name: class,
                    generics: generics.clone(),
                    implements: implements.clone(),
                    fields,
                    constructor: if params.is_empty() {
                        None
                    } else {
                        Some((params, body))
                    },
                    methods,
                });
            }
            let _ = enum_ty;
        }
        // The functions of the enum, as free functions taking `self`.
        for f in &e.methods {
            if f.body.is_none() {
                continue;
            }
            let name = self.names.get(&f.def).cloned().unwrap_or_default();
            // A method of a block for one instantiation takes that instantiation (spec chapter
            // 4, section 8.3, rule 8).
            let self_ty = IrType::Named(
                e.def,
                f.instance
                    .clone()
                    .unwrap_or_else(|| gnames.iter().map(|n| IrType::Param(n.clone())).collect()),
            );
            let by_ref = plain && f.self_mode == Some(Mode::Mut);
            let this = f.self_mode.map(|_| {
                if by_ref {
                    member(ident("self"), "v")
                } else {
                    ident("self")
                }
            });
            let (mut g, mut params, ret, body) = self.function_with(f, this, by_ref)?;
            if f.self_mode.is_some() {
                let mut t = self.ty(&self_ty);
                if by_ref {
                    self.rt.insert("Ref");
                    t = Type::Ref("$rt.Ref".to_string(), vec![t]);
                }
                params.insert(0, ("self".to_string(), t));
            }
            let mut all = instance_generics(&generics, &f.instance);
            all.append(&mut g);
            out.push(Item::Function {
                docs: f.docs.clone(),
                export: true,
                name,
                generics: all,
                params,
                ret,
                body,
            });
        }
        Ok(())
    }

    fn extern_fn(&mut self, x: &'p ir::ExternFn, out: &mut Vec<Item>) -> Result<(), Error> {
        let Some(path) = x.paths.get("ts") else {
            return Ok(());
        };
        let names: Vec<String> = (0..x.params.len()).map(|i| format!("p{i}")).collect();
        let params: Vec<(String, Type)> = names
            .iter()
            .zip(&x.params)
            .map(|(n, (m, t))| (n.clone(), self.param_ty(*m, t)))
            .collect();
        let mut args = Vec::new();
        // A function of a native module: the TypeScript types the reader found.
        let native = x.native.get("ts");
        for (i, (n, (_, t))) in names.iter().zip(&x.params).enumerate() {
            let nt = native.and_then(|s| s.params.get(i));
            args.push(self.extern_convert(ident(n), t, true, nt)?);
        }
        // A member of a class of a native module (spec chapter 7, section 8.7, rule 17): the class
        // is imported once under its own local name.
        let class_member = path
            .split_once('#')
            .and_then(|(module, export)| export.split_once('.').map(|(c, m)| (module, c, m)));
        let callee_call = if let Some((module, class, name)) = class_member {
            let local = format!("{class}$class");
            let import = (format!("{class} as {local}"), module.to_string());
            if !self.externs.contains(&import) {
                self.externs.push(import);
            }
            if let Some(method) = name.strip_prefix("prototype.") {
                let mut rest = args.into_iter();
                let recv = rest
                    .next()
                    .ok_or_else(|| format!("extern method `{}` has no receiver", x.name))?;
                call(member(recv, method), rest.collect())
            } else if name == "constructor" {
                Expr::New(local, args)
            } else {
                call(member(ident(&local), name), args)
            }
        } else if let Some((module, export)) = path.split_once('#') {
            // The wrapper has the wolfpack name; an export of the same name gets another one.
            let local = if export == "default" {
                camel(&x.name) + "$default"
            } else if export == camel(&x.name) {
                format!("{export}$extern")
            } else {
                export.to_string()
            };
            self.externs.push((
                if local == export {
                    export.to_string()
                } else {
                    format!("{export} as {local}")
                },
                module.to_string(),
            ));
            call(ident(&local), args)
        } else if let Some((_, method)) = path.split_once(".prototype.") {
            let mut rest = args.into_iter();
            let recv = rest
                .next()
                .ok_or_else(|| format!("extern method `{}` has no receiver", x.name))?;
            call(member(recv, method), rest.collect())
        } else {
            call(ident(path), args)
        };
        let ret = self.ret_ty(&x.ret);
        let body = self.extern_body(callee_call, &x.ret, native.map(|s| &s.ret))?;
        out.push(Item::Function {
            docs: Vec::new(),
            export: x.public,
            name: camel(&x.name),
            generics: Vec::new(),
            params,
            ret,
            body,
        });
        Ok(())
    }

    /// The body of an extern function: an exception becomes `Err` when the result is a
    /// `Result`, `null` and `undefined` become `None` (spec chapter 7, section 8.4).
    fn extern_body(
        &mut self,
        c: Expr,
        ret: &IrType,
        nt: Option<&NativeTy>,
    ) -> Result<Vec<Stmt>, Error> {
        Ok(match ret {
            IrType::Void => vec![Stmt::Expr(c)],
            IrType::Result(ok, _) => {
                let mut body = Vec::new();
                let value = if matches!(**ok, IrType::Void) {
                    body.push(Stmt::Expr(c));
                    Expr::Undefined
                } else {
                    self.extern_convert(c, ok, false, inner_native(nt))?
                };
                body.push(Stmt::Return(Some(self.rt_call("ok", vec![value]))));
                let error = self.rt_call("externError", vec![ident("e")]);
                let handler = vec![Stmt::Return(Some(self.rt_call("err", vec![error])))];
                vec![Stmt::Try(body, "e".to_string(), handler)]
            }
            IrType::Option(inner) if undef_result(ret) => {
                let nt = inner_native(nt);
                let v = if self.extern_converter(inner, false, nt)?.is_none() {
                    Expr::Binary(Box::new(c), "??", Box::new(Expr::Undefined))
                } else {
                    let x = self.extern_convert(ident("v"), inner, false, nt)?;
                    let f = Expr::Arrow(
                        vec![("v".to_string(), None)],
                        None,
                        ArrowBody::Expr(Box::new(x)),
                    );
                    self.rt_call("fromNullable", vec![c, f])
                };
                vec![Stmt::Return(Some(v))]
            }
            other => vec![Stmt::Return(Some(
                self.extern_convert(c, other, false, nt)?,
            ))],
        })
    }

    fn opaque(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if matches!(self.p.def(*d).map(|x| x.kind), Some(DefKind::ExternType)))
    }

    /// `type Pattern = RegExp`, with an import for a module export.
    fn extern_type(&mut self, x: &'p ir::ExternType, out: &mut Vec<Item>) {
        let Some(path) = x.paths.get("ts") else {
            return;
        };
        let name = self.def_name(x.def);
        let target = match path.split_once('#') {
            // The type is exported again under its wolfpack name, for the modules that import
            // this one; an export of the same name is imported under another one.
            Some((module, export)) if export == name => {
                self.externs.push((
                    format!("type {export} as {export}$extern"),
                    module.to_string(),
                ));
                format!("{export}$extern")
            }
            Some((module, export)) => {
                self.externs
                    .push((format!("type {export}"), module.to_string()));
                export.to_string()
            }
            None => path.clone(),
        };
        out.push(Item::TypeAlias {
            docs: Vec::new(),
            export: true,
            name,
            generics: Vec::new(),
            ty: Type::name(&target),
        });
    }

    /// `e` converted for extern code (`out`) or from it: `Int` to and from `number`, lists
    /// element by element, maps copied and, from extern code, sorted by key (spec chapter 7,
    /// section 8.4).
    /// `nt` is the TypeScript type of a native module (spec chapter 7, section 8.7): a `bigint`
    /// crosses as it is.
    fn extern_convert(
        &mut self,
        e: Expr,
        t: &IrType,
        out: bool,
        nt: Option<&NativeTy>,
    ) -> Result<Expr, Error> {
        Ok(match self.extern_converter(t, out, nt)? {
            Some(f) => match t {
                IrType::Int => call(f, vec![e]),
                IrType::List(_) if !out && matches!(nt, Some(NativeTy::Slice(_))) => {
                    call(f, vec![e])
                }
                IrType::List(inner) => {
                    let f = self
                        .extern_converter(inner, out, inner_native(nt))?
                        .ok_or("a list without a converter")?;
                    call(member(e, "map"), vec![f])
                }
                IrType::Map(k, v) => self.extern_map(e, k, v, out, nt)?,
                IrType::Fn(params, ret) => self.extern_callback(e, params, ret)?,
                // A parameter by its name: `p0.some ? p0.value : undefined`.
                IrType::Option(inner) if out && matches!(e, Expr::Ident(_)) => {
                    let value = self.extern_convert(
                        member(e.clone(), "value"),
                        inner,
                        true,
                        inner_native(nt),
                    )?;
                    Expr::Cond(
                        Box::new(member(e, "some")),
                        Box::new(value),
                        Box::new(Expr::Undefined),
                    )
                }
                _ => call(f, vec![e]),
            },
            None => e,
        })
    }

    /// A callback passed to extern code: an arrow function that converts its arguments from the
    /// target and its result to it (spec chapter 8, section 8).
    fn extern_callback(
        &mut self,
        f: Expr,
        params: &[(Mode, IrType)],
        ret: &IrType,
    ) -> Result<Expr, Error> {
        let names: Vec<String> = (0..params.len()).map(|i| format!("a{i}")).collect();
        let mut args = Vec::new();
        for (n, (_, t)) in names.iter().zip(params) {
            args.push(self.extern_convert(ident(n), t, false, None)?);
        }
        let body = self.extern_convert(call(f, args), ret, true, None)?;
        Ok(Expr::Arrow(
            names.into_iter().map(|n| (n, None)).collect(),
            None,
            ArrowBody::Expr(Box::new(body)),
        ))
    }

    /// The fields of a struct bound to a TypeScript type in `extern` (a record): each with its
    /// TypeScript name and, for a record of a native module, the type the reader found.
    #[allow(clippy::type_complexity)]
    fn record_fields(&self, d: DefRef) -> Option<Vec<(ir::Field, String, Option<NativeTy>)>> {
        let s = self
            .structs
            .get(&d)
            .filter(|s| s.extern_paths.contains_key("ts"))?;
        let native = s.extern_fields.get("ts");
        Some(
            s.fields
                .iter()
                .enumerate()
                .map(|(i, f)| match native.and_then(|n| n.get(i)) {
                    Some((name, nt)) => (f.clone(), name.clone(), Some(nt.clone())),
                    None => (f.clone(), camel(&f.name), None),
                })
                .collect(),
        )
    }

    /// `rt.toExternMap(m, key, value)` or `rt.fromExternMap(m, key, value)`.
    fn extern_map(
        &mut self,
        e: Expr,
        k: &IrType,
        v: &IrType,
        out: bool,
        nt: Option<&NativeTy>,
    ) -> Result<Expr, Error> {
        let f = |t: &IrType, nt: Option<&NativeTy>, this: &mut Self| -> Result<Expr, Error> {
            Ok(match this.extern_converter(t, out, nt)? {
                Some(f) => f,
                None => this.rt("same"),
            })
        };
        let (kn, vn) = match nt {
            Some(NativeTy::Map { key, value, .. }) => (Some(key.as_ref()), Some(value.as_ref())),
            _ => (None, None),
        };
        let key = f(k, kn, self)?;
        let value = f(v, vn, self)?;
        let name = if out { "toExternMap" } else { "fromExternMap" };
        Ok(self.rt_call(name, vec![e, key, value]))
    }

    /// The function that converts a value of type `t` for extern code or from it; `None` if the
    /// value crosses as it is.
    fn extern_converter(
        &mut self,
        t: &IrType,
        out: bool,
        nt: Option<&NativeTy>,
    ) -> Result<Option<Expr>, Error> {
        let arrow = |body: Expr| {
            Expr::Arrow(
                vec![("v".to_string(), None)],
                None,
                ArrowBody::Expr(Box::new(body)),
            )
        };
        Ok(match t {
            IrType::Int if matches!(nt, Some(NativeTy::Int(n)) if n == "bigint") => None,
            // A `readonly` array of a native module comes back as a copy that wolfpack owns.
            IrType::List(inner) if !out && matches!(nt, Some(NativeTy::Slice(_))) => Some(arrow(
                match self.extern_converter(inner, out, inner_native(nt))? {
                    Some(f) => call(member(ident("v"), "map"), vec![f]),
                    None => call(member(ident("v"), "slice"), Vec::new()),
                },
            )),
            IrType::Int => Some(self.rt(if out { "toNumber" } else { "fromNumber" })),
            IrType::Float | IrType::Bool | IrType::String | IrType::Void => None,
            t if self.opaque(t) => None,
            IrType::List(inner) => self
                .extern_converter(inner, out, inner_native(nt))?
                .map(|f| arrow(call(member(ident("v"), "map"), vec![f]))),
            IrType::Map(k, v) => Some(arrow(self.extern_map(ident("v"), k, v, out, nt)?)),
            // A record: a plain object with the fields converted (chapter 7, section 8.4a).
            IrType::Named(d, _) if self.record_fields(*d).is_some() => {
                let fields = self.record_fields(*d).unwrap_or_default();
                let mut entries = Vec::new();
                for (f, theirs, fnt) in &fields {
                    let ours = camel(&f.name);
                    let (from, to) = if out {
                        (ours, theirs.clone())
                    } else {
                        (theirs.clone(), ours)
                    };
                    let value = member(ident("v"), &from);
                    entries.push((to, self.extern_convert(value, &f.ty, out, fnt.as_ref())?));
                }
                let object = Expr::Object(entries);
                Some(arrow(if out {
                    object
                } else {
                    Expr::New(self.def_name(*d), vec![object])
                }))
            }
            // An `Option` for extern code is `T | undefined` (chapter 8, section 8, D8.12).
            IrType::Option(inner) if out => {
                let value = self.extern_convert(
                    member(ident("v"), "value"),
                    inner,
                    true,
                    inner_native(nt),
                )?;
                Some(arrow(Expr::Cond(
                    Box::new(member(ident("v"), "some")),
                    Box::new(value),
                    Box::new(Expr::Undefined),
                )))
            }
            // A callback is only a parameter (chapter 7, section 8.4, rule 7).
            IrType::Fn(params, ret) if out => {
                Some(arrow(self.extern_callback(ident("v"), params, ret)?))
            }
            other => {
                let side = if out { "parameter" } else { "result" };
                return Err(format!("extern {side} of type {other:?} for TypeScript"));
            }
        })
    }

    /// Code of std modules for intrinsics that need the module's own types.
    fn std_snippets(&mut self, index: usize, items: &mut Vec<Item>) {
        let Some(used) = self.snippets.get(&index).cloned() else {
            return;
        };
        let parse_error = || "new ParseError({ input: text })".to_string();
        for s in used {
            let text = match s {
                "stringParseInt" => {
                    self.rt.extend(["Result", "parseIntText", "ok", "err"]);
                    format!(
                        "/** `String.parse_int` */\nexport function stringParseInt(text: string): $rt.Result<bigint, ParseError> {{\n  const value = $rt.parseIntText(text);\n  return value === undefined ? $rt.err({}) : $rt.ok(value);\n}}",
                        parse_error()
                    )
                }
                "stringParseFloat" => {
                    self.rt.extend(["Result", "parseFloatText", "ok", "err"]);
                    format!(
                        "/** `String.parse_float` */\nexport function stringParseFloat(text: string): $rt.Result<number, ParseError> {{\n  const value = $rt.parseFloatText(text);\n  return value === undefined ? $rt.err({}) : $rt.ok(value);\n}}",
                        parse_error()
                    )
                }
                "mapEntries" => "/** `Map.entries` */\nexport function mapEntries<K, V>(map: Map<K, V>): Entry<K, V>[] {\n  return [...map].map(([key, value]) => new Entry<K, V>({ key, value }));\n}".to_string(),
                "fsReadText" | "fsWriteText" | "fsExists" => {
                    self.rt.extend(["Result", "ok", "err", "nodeFs"]);
                    let names = |v: &str| self.names_of_variant("FsError", v);
                    let not_found = names("NotFound");
                    let denied = names("PermissionDenied");
                    let other = names("Other");
                    let error = format!(
                        "function fsError(error: unknown, path: string): FsError {{\n  const code = (error as {{ code?: string }}).code;\n  if (code === \"ENOENT\") {{\n    return new {not_found}(path);\n  }}\n  if (code === \"EACCES\" || code === \"EPERM\") {{\n    return new {denied}(path);\n  }}\n  return new {other}(String((error as {{ message?: string }}).message));\n}}"
                    );
                    match s {
                        "fsReadText" => format!("{error}\n\n/** `fs.read_text` */\nexport function fsReadText(path: string): $rt.Result<string, FsError> {{\n  try {{\n    return $rt.ok($rt.nodeFs().readFileSync(path, \"utf8\") as string);\n  }} catch (error) {{\n    return $rt.err(fsError(error, path));\n  }}\n}}"),
                        "fsWriteText" => ("/** `fs.write_text` */\nexport function fsWriteText(path: string, content: string): $rt.Result<undefined, FsError> {\n  try {\n    $rt.nodeFs().writeFileSync(path, content, \"utf8\");\n    return $rt.ok(undefined);\n  } catch (error) {\n    return $rt.err(fsError(error, path));\n  }\n}").to_string(),

                        _ => "/** `fs.exists` */\nexport function fsExists(path: string): boolean {\n  return $rt.nodeFs().existsSync(path) as boolean;\n}".to_string(),
                    }
                }
                _ => continue,
            };
            items.push(Item::Raw(text));
        }
    }

    fn names_of_variant(&self, enum_name: &str, variant: &str) -> String {
        for (def, e) in &self.enums {
            if e.name == enum_name {
                for v in &e.variants {
                    if v.name == variant {
                        return self.names.get(&v.def).cloned().unwrap_or_default();
                    }
                }
                let _ = def;
            }
        }
        format!("{enum_name}{variant}")
    }

    // ----- functions --------------------------------------------------------------------------

    /// Generics, parameters, result type and body of a function.
    #[allow(clippy::type_complexity)]
    fn function(
        &mut self,
        f: &'p ir::Function,
        this: Option<Expr>,
    ) -> Result<
        (
            Vec<(String, Option<Type>)>,
            Vec<(String, Type)>,
            Type,
            Vec<Stmt>,
        ),
        Error,
    > {
        self.function_with(f, this, false)
    }

    /// [`Self::function`], with `self` passed as a `Ref` when `self_by_ref`.
    #[allow(clippy::type_complexity)]
    fn function_with(
        &mut self,
        f: &'p ir::Function,
        this: Option<Expr>,
        self_by_ref: bool,
    ) -> Result<
        (
            Vec<(String, Option<Type>)>,
            Vec<(String, Type)>,
            Type,
            Vec<Stmt>,
        ),
        Error,
    > {
        let saved = std::mem::take(&mut self.f);
        self.f.self_expr = this;
        self.f.self_by_ref = self_by_ref;
        self.f.rets.push((f.ret.clone(), true));
        let generics = self.generics(&f.generics);
        let mut params = Vec::new();
        let info = self.fns.get(&f.def).cloned();
        // The user interface bounds: of the type's parameters, of the interface's in a default,
        // and of the function's own.
        let mut bounds: Vec<(String, &IrType)> = Vec::new();
        if let Some((_, names)) = info.as_ref().and_then(|i| i.owner.as_ref()) {
            for (n, bs) in names.iter().zip(&f.impl_bounds) {
                bounds.extend(bs.iter().map(|b| (n.clone(), b)));
            }
        }
        if let Some(IrType::Named(i, _)) = &self.self_iface {
            if let Some(iface) = self.ifaces.get(i).copied() {
                for g in &iface.generics {
                    bounds.extend(g.bounds.iter().map(|b| (g.name.clone(), b)));
                }
            }
        }
        // In a default of an interface that mentions `Self`, `Self` is bounded by it.
        if let (true, Some(t @ IrType::Named(i, _))) = (self.self_param, self.self_iface.clone()) {
            self.f.bounds.insert(("Self".to_string(), i), t);
        }
        for g in &f.generics {
            bounds.extend(g.bounds.iter().map(|b| (g.name.clone(), b)));
        }
        for (n, b) in bounds {
            if let IrType::Named(i, _) = b {
                if self.view_iface(*i) {
                    self.f.bounds.insert((n, *i), b.clone());
                }
            }
        }
        let owner = info.as_ref().and_then(|i| i.owner.as_ref()).map(|o| o.0);
        let dicts = info.map(|i| i.dicts).unwrap_or_default();
        for (g, d) in dicts {
            let name = self.dict_name(&g, d);
            let ty = self.dict_ty(d, &IrType::Param(g.clone()));
            self.f.dicts.insert((g, d), ident(&name));
            params.push((name, ty));
        }
        // An implementation of an interface keeps the interface's signature: the dictionaries of
        // the type's bounds are fields of `self`.
        if let (Some(owner), Some(this)) = (owner, self.f.self_expr.clone()) {
            if let Some(iface) = f.interface {
                let mut dicts = self.type_dicts(owner);
                dicts.extend(self.head_dicts(owner, iface));
                for (g, d) in dicts {
                    let name = self.dict_name(&g, d);
                    self.f
                        .dicts
                        .entry((g, d))
                        .or_insert(member(this.clone(), &name));
                }
            }
        }
        for p in &f.params {
            let name = self.local_name(p.local);
            let ty = self.param_ty(p.mode, &p.ty);
            if self.boxed(p.mode, &p.ty) {
                self.f.locals.insert(p.local, Local::Boxed(name.clone()));
            } else {
                if p.mode == Mode::Mut {
                    self.f.by_reference.insert(p.local);
                }
                self.f.locals.insert(p.local, Local::Plain(name.clone()));
            }
            params.push((name, ty));
        }
        let ret = self.ret_ty(&f.ret);
        let mut body = Vec::new();
        if let Some(b) = &f.body {
            let target = if matches!(f.ret, IrType::Void) {
                Target::Discard
            } else {
                Target::Return
            };
            self.block(b, target, &mut body)?;
        }
        self.f = saved;
        Ok((generics, params, ret, body))
    }

    // ----- statements -------------------------------------------------------------------------

    fn block(&mut self, b: &'p Block, target: Target, out: &mut Vec<Stmt>) -> Result<(), Error> {
        for s in &b.stmts {
            self.stmt(s, out)?;
        }
        if let Some(tail) = &b.tail {
            self.tail(tail, target, out)?;
        }
        Ok(())
    }

    fn ret_repr(&self) -> Repr {
        match self.f.rets.last() {
            Some((t, true)) if undef_result(t) => Repr::Undef,
            _ => Repr::Tagged,
        }
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
                out.push(Stmt::If(c, t, o));
                Ok(())
            }
            ExprKind::Match {
                scrutinee,
                mode,
                arms,
            } => self.match_(scrutinee, *mode, arms, target, out),
            ExprKind::IfChain {
                items,
                then,
                otherwise,
            } => {
                // `$chain: { if (...) { ... then; break $chain; } otherwise }`; without an
                // `else` the `if`s alone.
                let Some(o) = otherwise else {
                    return self.chain_items(items, then, None, &target, out);
                };
                let label = self.temp("chain");
                let mut body = Vec::new();
                self.chain_items(items, then, Some(&label), &target, &mut body)?;
                let used = breaks_to(&body, &label);
                self.block(o, target, &mut body)?;
                if used {
                    out.push(Stmt::Labeled(label, Box::new(Stmt::Block(body))));
                } else {
                    // `then` always leaves: what follows the `if`s runs only when an item failed.
                    out.extend(body);
                }
                Ok(())
            }
            ExprKind::Block(b) => self.block(b, target, out),
            ExprKind::Loop { label, body } => {
                let to = match target {
                    Target::Discard => None,
                    t => Some(t),
                };
                self.f.loops.push((label.clone(), to));
                let mut b = Vec::new();
                self.block(body, Target::Discard, &mut b)?;
                self.f.loops.pop();
                out.push(labeled_loop(label, Stmt::While(Expr::Bool(true), b)));
                Ok(())
            }
            ExprKind::Return(_) | ExprKind::Break { .. } | ExprKind::Continue { .. } => {
                self.control(e, out)?;
                Ok(())
            }
            ExprKind::Panic { message, location } => {
                let m = self.expr(message, out)?;
                let p = self.rt_call("panic", vec![m, Expr::Str(location.clone())]);
                out.push(Stmt::Expr(p));
                Ok(())
            }
            ExprKind::Fail(message) => {
                let p = self.rt_call("fail", vec![Expr::Str(message.clone())]);
                out.push(Stmt::Expr(p));
                Ok(())
            }
            _ => {
                match target {
                    Target::Discard => {
                        let v = self.value(e, out, Repr::Tagged)?;
                        if !pure(&v) {
                            out.push(Stmt::Expr(v));
                        }
                    }
                    Target::Return => {
                        let want = self.ret_repr();
                        let v = self.value(e, out, want)?;
                        if matches!(e.ty, IrType::Never) {
                            out.push(Stmt::Expr(v));
                        } else {
                            out.push(Stmt::Return(Some(v)));
                        }
                    }
                    Target::Assign(place, repr) => {
                        let v = self.value(e, out, repr)?;
                        let v = self.widened(e, v);
                        if matches!(e.ty, IrType::Never) {
                            out.push(Stmt::Expr(v));
                        } else {
                            out.push(Stmt::Assign(place, v));
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// `return`, `break` and `continue` as statements.
    fn control(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match &e.kind {
            ExprKind::Return(value) => match value {
                Some(v) => {
                    let want = self.ret_repr();
                    let v = self.value(v, out, want)?;
                    out.push(Stmt::Return(Some(v)));
                }
                None => out.push(Stmt::Return(None)),
            },
            ExprKind::Break { label, value } => {
                if let Some(v) = value {
                    // `break value` leaves the innermost loop, a `loop`, and `break label: value`
                    // the labelled one (chapter 3, rule 4.5; D3.11).
                    let target = match label {
                        Some(l) => self
                            .f
                            .loops
                            .iter()
                            .rev()
                            .find(|(name, _)| name.as_deref() == Some(l.as_str()))
                            .and_then(|(_, t)| t.clone()),
                        None => self.f.loops.last().and_then(|(_, t)| t.clone()),
                    };
                    match target {
                        Some(Target::Return) => {
                            let want = self.ret_repr();
                            let v = self.value(v, out, want)?;
                            out.push(Stmt::Return(Some(v)));
                            return Ok(());
                        }
                        Some(Target::Assign(place, repr)) => {
                            let v = self.value(v, out, repr)?;
                            out.push(Stmt::Assign(place, v));
                        }
                        _ => self.tail(v, Target::Discard, out)?,
                    }
                }
                out.push(Stmt::Break(label.as_deref().map(label_name)));
            }
            ExprKind::Continue { label } => {
                out.push(Stmt::Continue(label.as_deref().map(label_name)));
            }
            _ => {}
        }
        Ok(())
    }

    /// Emits a loop body with its label in scope.
    fn loop_block(
        &mut self,
        label: &Option<String>,
        body: &'p Block,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        self.f.loops.push((label.clone(), None));
        let r = self.block(body, Target::Discard, out);
        self.f.loops.pop();
        r
    }

    fn stmt(&mut self, s: &'p IrStmt, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match s {
            IrStmt::Let {
                local,
                mutable,
                value,
            } => {
                let ty = self.local_ty(*local);
                let t = self.ty(&ty);
                let name = self.local_name(*local);
                if matches!(
                    value.kind,
                    ExprKind::If { .. }
                        | ExprKind::IfChain { .. }
                        | ExprKind::Match { .. }
                        | ExprKind::Block(_)
                        | ExprKind::Loop { .. }
                ) && needs_stmts(value)
                {
                    out.push(Stmt::Let(name.clone(), Some(t), None));
                    self.tail(value, Target::Assign(ident(&name), Repr::Tagged), out)?;
                } else {
                    let v = self.value(value, out, Repr::Tagged)?;
                    let v = self.widened(value, v);
                    if *mutable {
                        out.push(Stmt::Let(name.clone(), Some(t), Some(v)));
                    } else {
                        out.push(Stmt::Const(name.clone(), Some(t), v));
                    }
                }
                self.f.locals.insert(*local, Local::Plain(name));
            }
            IrStmt::LetPattern {
                pattern,
                mutable,
                value,
            } => {
                // `const p = value; const x = p.x; ...` (the pattern always matches).
                let v = self.value(value, out, Repr::Tagged)?;
                let subject = if pure(&v) {
                    v
                } else {
                    let t = self.temp("v");
                    out.push(Stmt::Const(t.clone(), None, v));
                    ident(&t)
                };
                let mut conds = Vec::new();
                let mut bindings = Vec::new();
                self.test(
                    pattern,
                    subject,
                    &value.ty,
                    Repr::Tagged,
                    &mut conds,
                    &mut bindings,
                )?;
                for (local, path) in bindings {
                    let name = self.local_name(local);
                    if *mutable {
                        out.push(Stmt::Let(name.clone(), None, Some(path)));
                    } else {
                        out.push(Stmt::Const(name.clone(), None, path));
                    }
                    self.f.locals.insert(local, Local::Plain(name));
                }
            }
            IrStmt::Assign { target, op, value } => self.assign(target, *op, value, out)?,
            IrStmt::While { label, cond, body } => {
                let mut pre = Vec::new();
                let c = self.expr(cond, &mut pre)?;
                let mut b = Vec::new();
                self.loop_block(label, body, &mut b)?;
                if pre.is_empty() {
                    out.push(labeled_loop(label, Stmt::While(c, b)));
                } else {
                    pre.push(Stmt::If(not(c), vec![Stmt::Break(None)], None));
                    pre.extend(b);
                    out.push(labeled_loop(label, Stmt::While(Expr::Bool(true), pre)));
                }
            }
            IrStmt::ForList {
                label,
                var,
                mode,
                list,
                body,
            } => {
                let l = self.expr(list, out)?;
                let elem = match &list.ty {
                    IrType::List(t) => (**t).clone(),
                    _ => IrType::Void,
                };
                if *mode == Mode::Mut && !by_reference(&elem, &self.structs) {
                    let items = if pure(&l) {
                        l
                    } else {
                        let t = self.temp("items");
                        out.push(Stmt::Const(t.clone(), None, l));
                        ident(&t)
                    };
                    let i = self.temp("i");
                    self.f
                        .locals
                        .insert(*var, Local::Alias(Place::Elem(items.clone(), ident(&i))));
                    let mut b = Vec::new();
                    self.loop_block(label, body, &mut b)?;
                    let stmt = Stmt::For(
                        Box::new(Stmt::Let(i.clone(), None, Some(Expr::Num("0".to_string())))),
                        bin(ident(&i), "<", member(items, "length")),
                        Expr::Unary("++", Box::new(ident(&i))),
                        b,
                    );
                    out.push(labeled_loop(label, stmt));
                } else {
                    let name = self.local_name(*var);
                    self.f.locals.insert(*var, Local::Plain(name.clone()));
                    let mut b = Vec::new();
                    self.loop_block(label, body, &mut b)?;
                    out.push(labeled_loop(label, Stmt::ForOf(name, l, b)));
                }
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
                let e = self.expr(end, out)?;
                let e = if pure(&e) {
                    e
                } else {
                    let t = self.temp("end");
                    out.push(Stmt::Const(t.clone(), None, e));
                    ident(&t)
                };
                let name = self.local_name(*var);
                self.f.locals.insert(*var, Local::Plain(name.clone()));
                let mut b = Vec::new();
                self.loop_block(label, body, &mut b)?;
                let stmt = Stmt::For(
                    Box::new(Stmt::Let(name.clone(), Some(Type::name("bigint")), Some(s))),
                    bin(ident(&name), if *inclusive { "<=" } else { "<" }, e),
                    Expr::Unary("++", Box::new(ident(&name))),
                    b,
                );
                out.push(labeled_loop(label, stmt));
            }
            IrStmt::Expr(e) => self.tail(e, Target::Discard, out)?,
        }
        Ok(())
    }

    /// The place an assignment target names.
    fn place(&mut self, target: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<PlaceKind, Error> {
        Ok(match &target.kind {
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Plain(n)) => {
                    if self.f.by_reference.contains(l) {
                        PlaceKind::Whole(ident(&n), target.ty.clone())
                    } else {
                        PlaceKind::Simple(ident(&n))
                    }
                }
                Some(Local::Boxed(n)) => PlaceKind::Simple(member(ident(&n), "v")),
                Some(Local::Alias(Place::Expr(e))) => PlaceKind::Simple(e),
                Some(Local::Alias(Place::Elem(l, i))) => {
                    PlaceKind::Simple(Expr::Index(Box::new(l), Box::new(i)))
                }
                None => return Err("assignment to an unknown local".to_string()),
            },
            ExprKind::SelfValue => {
                let s = self.f.self_expr.clone().unwrap_or(Expr::This);
                if self.f.self_by_ref {
                    PlaceKind::Simple(s)
                } else {
                    PlaceKind::Whole(s, target.ty.clone())
                }
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base, out)?;
                PlaceKind::Simple(member(b, &camel(name)))
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base, out)?;
                let b = self.stable(b, "base", out);
                let i = self.expr(index, out)?;
                let i = self.stable(i, "key", out);
                match &base.ty {
                    IrType::Map(..) => PlaceKind::Key(b, i),
                    _ => PlaceKind::Elem(b, i),
                }
            }
            _ => return Err("assignment to an expression that is not a place".to_string()),
        })
    }

    /// The expression itself if it may be evaluated twice, or a temporary holding it.
    fn stable(&mut self, e: Expr, base: &str, out: &mut Vec<Stmt>) -> Expr {
        if pure(&e) {
            return e;
        }
        let t = self.temp(base);
        out.push(Stmt::Const(t.clone(), None, e));
        ident(&t)
    }

    /// A new variant of an enum with payloads stored in a variable, as a value of the whole
    /// enum: `new ShapeRect(1n, 2n) as Shape`. TypeScript narrows a variable of a union type to
    /// the class of the value assigned to it, and would then reject a `match` on it that tests
    /// the other variants.
    fn widened(&mut self, value: &IrExpr, v: Expr) -> Expr {
        if matches!(value.kind, ExprKind::Variant { .. }) && !self.plain_enum(&value.ty) {
            Expr::As(Box::new(v), self.ty(&value.ty))
        } else {
            v
        }
    }

    fn assign(
        &mut self,
        target: &'p IrExpr,
        op: Option<BinaryOp>,
        value: &'p IrExpr,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let place = self.place(target, out)?;
        let v = self.expr(value, out)?;
        let v = match op {
            None if matches!(place, PlaceKind::Simple(_)) => self.widened(value, v),
            None => v,
            Some(op) => {
                let current = match &place {
                    PlaceKind::Simple(e) | PlaceKind::Whole(e, _) => e.clone(),
                    PlaceKind::Elem(l, i) => self.rt_call("at", vec![l.clone(), i.clone()]),
                    PlaceKind::Key(m, k) => self.rt_call("mapGet", vec![m.clone(), k.clone()]),
                };
                self.binary_values(op, current, v, &target.ty)
            }
        };
        match place {
            PlaceKind::Simple(e) => out.push(Stmt::Assign(e, v)),
            PlaceKind::Elem(l, i) => {
                let c = self.rt_call("setAt", vec![l, i, v]);
                out.push(Stmt::Expr(c));
            }
            PlaceKind::Key(m, k) => {
                let c = self.rt_call("mapReplace", vec![m, k, v]);
                out.push(Stmt::Expr(c));
            }
            PlaceKind::Whole(e, ty) => match ty {
                // A `mut` parameter passed by reference: change the value in place.
                IrType::List(_) => {
                    let len = member(e.clone(), "length");
                    out.push(Stmt::Expr(call(
                        member(e, "splice"),
                        vec![Expr::Num("0".to_string()), len, Expr::Spread(Box::new(v))],
                    )));
                }
                IrType::Map(..) => {
                    let t = self.temp("new");
                    out.push(Stmt::Const(t.clone(), None, v));
                    out.push(Stmt::Expr(call(member(e.clone(), "clear"), Vec::new())));
                    out.push(Stmt::ForOf(
                        "[k, v]".to_string(),
                        ident(&t),
                        vec![Stmt::Expr(call(
                            member(e, "set"),
                            vec![ident("k"), ident("v")],
                        ))],
                    ));
                }
                IrType::Named(d, _) if self.structs.contains_key(&d) => {
                    out.push(Stmt::Expr(call(
                        member(ident("Object"), "assign"),
                        vec![e, v],
                    )));
                }
                // An enum with payloads: the object becomes the new variant in place.
                IrType::Named(d, _) if self.enums.contains_key(&d) => {
                    let c = self.rt_call("become", vec![e, v]);
                    out.push(Stmt::Expr(c));
                }
                _ => return Err("assignment to `self` of this type".to_string()),
            },
        }
        Ok(())
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Expr, Error> {
        self.value(e, out, Repr::Tagged)
    }

    /// An expression in the wanted representation (for `Option` values).
    fn value(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>, want: Repr) -> Result<Expr, Error> {
        let (v, have) = self.native(e, out, want)?;
        Ok(if matches!(e.ty, IrType::Option(_)) && have != want {
            match want {
                Repr::Tagged => self.rt_call("tagged", vec![v]),
                Repr::Undef => self.rt_call("untag", vec![v]),
            }
        } else {
            v
        })
    }

    /// Several subexpressions in order: if a later one needs statements, earlier ones are kept
    /// in temporaries so that they are still evaluated first.
    fn seq(
        &mut self,
        items: &[(&'p IrExpr, Repr)],
        out: &mut Vec<Stmt>,
    ) -> Result<Vec<Expr>, Error> {
        let mut parts: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        for (e, r) in items {
            let mut pre = Vec::new();
            let v = self.value(e, &mut pre, *r)?;
            parts.push((v, pre));
        }
        let last_with_stmts = parts.iter().rposition(|(_, pre)| !pre.is_empty());
        let mut values = Vec::new();
        for (i, (v, pre)) in parts.into_iter().enumerate() {
            out.extend(pre);
            let later = last_with_stmts.is_some_and(|l| l > i);
            if later && !pure(&v) {
                let t = self.temp("v");
                out.push(Stmt::Const(t.clone(), None, v));
                values.push(ident(&t));
            } else {
                values.push(v);
            }
        }
        Ok(values)
    }

    fn native(
        &mut self,
        e: &'p IrExpr,
        out: &mut Vec<Stmt>,
        want: Repr,
    ) -> Result<(Expr, Repr), Error> {
        let tagged = |x: Expr| Ok((x, Repr::Tagged));
        match &e.kind {
            ExprKind::Int(v) => tagged(Expr::BigInt(*v)),
            ExprKind::Float(v) => tagged(float_lit(*v)),
            ExprKind::Str(s) => tagged(Expr::Str(s.clone())),
            ExprKind::Interpolate(parts) => {
                // `${x}` prints `Int`, `Bool` and `String` values as `to_string` does.
                let items: Vec<(&'p IrExpr, Repr)> = parts
                    .iter()
                    .map(|x| (plain_text(self.p, x).unwrap_or(x), Repr::Tagged))
                    .collect();
                let values = self.seq(&items, out)?;
                tagged(Expr::Template(values))
            }
            ExprKind::Bool(b) => tagged(Expr::Bool(*b)),
            ExprKind::Unit => tagged(Expr::Undefined),
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Plain(n)) => tagged(ident(&n)),
                Some(Local::Boxed(n)) => tagged(member(ident(&n), "v")),
                Some(Local::Alias(Place::Expr(p))) => tagged(p),
                Some(Local::Alias(Place::Elem(l, i))) => tagged(Expr::As(
                    Box::new(Expr::Index(Box::new(l), Box::new(i))),
                    self.ty(&e.ty),
                )),
                None => Err(format!("unknown local {l:?}")),
            },
            ExprKind::SelfValue => tagged(self.f.self_expr.clone().unwrap_or(Expr::This)),
            ExprKind::FnValue(def) => {
                let d = self.p.def(*def).ok_or("unknown function")?.clone();
                if d.intrinsic.is_some() {
                    return Err(format!("the intrinsic `{}` used as a value", d.name));
                }
                match d.kind {
                    DefKind::Method { owner, .. } if self.structs.contains_key(&owner) => {
                        let o = self.def_name(owner);
                        tagged(member(ident(&o), &camel(&d.name)))
                    }
                    _ => {
                        let name = ident(&self.def_name(*def));
                        // A declared function returning `T | undefined`, or taking dictionaries,
                        // used as a function value.
                        match self.fns.get(def).cloned() {
                            Some(info) if undef_result(&info.ret) || !info.dicts.is_empty() => {
                                let mut map = BTreeMap::new();
                                if let IrType::Fn(ps, r) = &e.ty {
                                    for ((_, p), (_, a)) in info.params.iter().zip(ps) {
                                        unify(p, a, &mut map);
                                    }
                                    unify(&info.ret, r, &mut map);
                                }
                                self.unify_bounds(&info.bounds, &mut map);
                                let params: Vec<(String, Option<Type>)> = info
                                    .params
                                    .iter()
                                    .enumerate()
                                    .map(|(i, (m, t))| {
                                        let t = subst(t, &map);
                                        (format!("p{i}"), Some(self.param_ty(*m, &t)))
                                    })
                                    .collect();
                                let mut args: Vec<Expr> = Vec::new();
                                for (g, d) in &info.dicts {
                                    let t = map.get(g).cloned().unwrap_or(IrType::Void);
                                    args.push(self.dict(*d, &t)?);
                                }
                                args.extend(params.iter().map(|(n, _)| ident(n)));
                                let mut body = call(name, args);
                                if undef_result(&info.ret) {
                                    body = self.rt_call("tagged", vec![body]);
                                }
                                tagged(Expr::Arrow(params, None, ArrowBody::Expr(Box::new(body))))
                            }
                            _ => tagged(name),
                        }
                    }
                }
            }
            ExprKind::Const(def) => tagged(ident(&self.def_name(*def))),
            ExprKind::Call { callee, args } => self.call(callee, args, &e.ty, out),
            ExprKind::Field { base, name } => {
                let b = self.expr(base, out)?;
                tagged(member(b, &camel(name)))
            }
            ExprKind::Index { base, index } => {
                let v = self.seq(&[(base, Repr::Tagged), (index, Repr::Tagged)], out)?;
                let mut it = v.into_iter();
                let (b, i) = (
                    it.next().unwrap_or(Expr::Undefined),
                    it.next().unwrap_or(Expr::Undefined),
                );
                match &base.ty {
                    IrType::Map(..) => tagged(self.rt_call("mapGet", vec![b, i])),
                    _ => tagged(self.rt_call("at", vec![b, i])),
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
                    None => Expr::Undefined,
                };
                let x = match end {
                    Some(s) => self.expr(s, out)?,
                    None => Expr::Undefined,
                };
                let f = if matches!(base.ty, IrType::String) {
                    "sliceStr"
                } else {
                    "sliceList"
                };
                tagged(self.rt_call(f, vec![b, s, x, Expr::Bool(*inclusive)]))
            }
            ExprKind::Struct { def, fields, base } => {
                let mut items: Vec<(&IrExpr, Repr)> =
                    fields.iter().map(|(_, v)| (v, Repr::Tagged)).collect();
                if let Some(b) = base {
                    items.push((b, Repr::Tagged));
                }
                let mut values = self.seq(&items, out)?;
                // `..base`: the base is spread first, the written fields replace its values.
                let spread = if base.is_some() { values.pop() } else { None };
                let mut entries: Vec<(String, Expr)> = Vec::new();
                if let Some(b) = spread {
                    entries.push((String::new(), b));
                }
                entries.extend(fields.iter().zip(values).map(|((n, _), v)| (camel(n), v)));
                let object = Expr::Object(entries);
                let mut args = vec![object];
                args.extend(self.instance_dicts(*def, &e.ty)?);
                let name = self.def_name(*def);
                let name = self.with_args(name, &e.ty);
                tagged(Expr::New(name, args))
            }
            ExprKind::Variant { def, values } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("variant without an enum".to_string()),
                };
                let name = self.p.def(*def).map(|d| d.name.clone()).unwrap_or_default();
                if self
                    .enums
                    .get(&parent)
                    .is_some_and(|en| en.variants.iter().all(|v| v.payload.is_empty()))
                {
                    let lit = Expr::Str(name);
                    return tagged(Expr::As(Box::new(lit), self.ty(&e.ty)));
                }
                let items: Vec<(&IrExpr, Repr)> =
                    values.iter().map(|v| (v, Repr::Tagged)).collect();
                let mut vs = self.seq(&items, out)?;
                vs.extend(self.instance_dicts(parent, &e.ty)?);
                // The class lives in the module of the enum.
                let _ = self.def_name(parent);
                let class = self.variant_class(*def, parent);
                let class = self.with_args(class, &e.ty);
                tagged(Expr::New(class, vs))
            }
            ExprKind::Some(v) => {
                let x = self.expr(v, out)?;
                if want == Repr::Undef {
                    Ok((x, Repr::Undef))
                } else {
                    tagged(self.rt_call("some", vec![x]))
                }
            }
            ExprKind::None => {
                if want == Repr::Undef {
                    Ok((Expr::Undefined, Repr::Undef))
                } else {
                    let n = self.rt("none");
                    tagged(n)
                }
            }
            ExprKind::Ok(v) => {
                let x = self.expr(v, out)?;
                tagged(self.rt_call("ok", vec![x]))
            }
            ExprKind::Err(v) => {
                let x = self.expr(v, out)?;
                tagged(self.rt_call("err", vec![x]))
            }
            ExprKind::List(items) => {
                let list: Vec<(&IrExpr, Repr)> = items.iter().map(|v| (v, Repr::Tagged)).collect();
                let vs = self.seq(&list, out)?;
                if vs.is_empty() {
                    let t = self.ty(&e.ty);
                    tagged(Expr::As(Box::new(Expr::Array(Vec::new())), t))
                } else {
                    tagged(Expr::Array(vs))
                }
            }
            ExprKind::Map(entries) => {
                let mut list = Vec::new();
                for (k, v) in entries {
                    list.push((k, Repr::Tagged));
                    list.push((v, Repr::Tagged));
                }
                let vs = self.seq(&list, out)?;
                let pairs: Vec<Expr> = vs.chunks(2).map(|c| Expr::Array(c.to_vec())).collect();
                let (k, v) = match &e.ty {
                    IrType::Map(k, v) => (self.ty(k), self.ty(v)),
                    _ => (Type::name("never"), Type::name("never")),
                };
                let args = if pairs.is_empty() {
                    Vec::new()
                } else {
                    vec![Expr::Array(pairs)]
                };
                tagged(Expr::New(
                    format!("Map<{}, {}>", ts::ty(&k), ts::ty(&v)),
                    args,
                ))
            }
            ExprKind::Unary { op, operand } => {
                let v = self.expr(operand, out)?;
                tagged(match op {
                    UnaryOp::Not => not(v),
                    UnaryOp::BitNot => Expr::Unary("~", Box::new(v)),
                    UnaryOp::Neg => {
                        if matches!(operand.ty, IrType::Int) {
                            self.rt_call("neg", vec![v])
                        } else {
                            Expr::Unary("-", Box::new(v))
                        }
                    }
                })
            }
            ExprKind::Binary { op, lhs, rhs } => tagged(self.binary(*op, lhs, rhs, out)?),
            ExprKind::Closure { params, body } => {
                let ret = match &e.ty {
                    IrType::Fn(_, r) => (**r).clone(),
                    _ => IrType::Void,
                };
                tagged(self.closure(params, body, &ret)?)
            }
            ExprKind::If { .. }
            | ExprKind::IfChain { .. }
            | ExprKind::Match { .. }
            | ExprKind::Block(_)
            | ExprKind::Loop { .. } => {
                if let ExprKind::If {
                    cond,
                    then,
                    otherwise: Some(o),
                } = &e.kind
                {
                    let simple = |b: &Block| {
                        b.stmts.is_empty()
                            && b.tail
                                .as_ref()
                                .is_some_and(|t| !needs_stmts(t) && !matches!(t.ty, IrType::Never))
                    };
                    if simple(then) && simple(o) && !needs_stmts(cond) {
                        let c = self.expr(cond, out)?;
                        let (Some(a), Some(b)) = (&then.tail, &o.tail) else {
                            return Err("if without values".to_string());
                        };
                        let a = self.value(a, out, want)?;
                        let b = self.value(b, out, want)?;
                        return Ok((Expr::Cond(Box::new(c), Box::new(a), Box::new(b)), want));
                    }
                }
                if let ExprKind::Block(b) = &e.kind {
                    if b.stmts.is_empty() {
                        if let Some(t) = &b.tail {
                            return Ok((self.value(t, out, want)?, want));
                        }
                    }
                }
                let t = self.temp("r");
                let ty = if matches!(e.ty, IrType::Option(_)) && want == Repr::Undef {
                    self.ret_ty(&e.ty)
                } else {
                    self.ty(&e.ty)
                };
                out.push(Stmt::Let(t.clone(), Some(ty), None));
                self.tail(e, Target::Assign(ident(&t), want), out)?;
                Ok((ident(&t), want))
            }
            ExprKind::Return(_) | ExprKind::Break { .. } | ExprKind::Continue { .. } => {
                self.control(e, out)?;
                tagged(Expr::As(Box::new(Expr::Undefined), Type::name("never")))
            }
            ExprKind::Try {
                value,
                coerce_error,
            } => self.try_(value, coerce_error.as_ref(), out),
            ExprKind::Coerce { value, to } => {
                let v = self.expr(value, out)?;
                tagged(self.coerce(v, &value.ty, to)?)
            }
            ExprKind::Copy(v) => {
                let x = self.expr(v, out)?;
                if self.primitive(&v.ty) {
                    tagged(x)
                } else {
                    tagged(self.rt_call("copy", vec![x]))
                }
            }
            ExprKind::Panic { message, location } => {
                let m = self.expr(message, out)?;
                tagged(self.rt_call("panic", vec![m, Expr::Str(location.clone())]))
            }
            ExprKind::Fail(message) => {
                tagged(self.rt_call("fail", vec![Expr::Str(message.clone())]))
            }
            ExprKind::Assert {
                cond,
                text,
                location,
            } => {
                let c = self.expr(cond, out)?;
                tagged(self.rt_call(
                    "assert",
                    vec![c, Expr::Str(text.clone()), Expr::Str(location.clone())],
                ))
            }
        }
    }

    /// The class of a variant, imported from the module of its enum if needed.
    fn variant_class(&mut self, variant: DefRef, parent: DefRef) -> String {
        self.def_name_local(variant, parent)
    }

    /// `Name<Args>` for a generic class.
    fn with_args(&mut self, name: String, t: &IrType) -> String {
        match t {
            IrType::Named(_, args) if !args.is_empty() => {
                let args: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let t = self.ty(a);
                        ts::ty(&t)
                    })
                    .collect();
                format!("{name}<{}>", args.join(", "))
            }
            _ => name,
        }
    }

    fn closure(
        &mut self,
        params: &'p [ir::Param],
        body: &'p Block,
        ret: &IrType,
    ) -> Result<Expr, Error> {
        // Function values return `Option` as data, so that they can be stored and passed on.
        self.f.rets.push((ret.clone(), false));
        let mut ps = Vec::new();
        for p in params {
            let name = self.local_name(p.local);
            let ty = self.param_ty(p.mode, &p.ty);
            if self.boxed(p.mode, &p.ty) {
                self.f.locals.insert(p.local, Local::Boxed(name.clone()));
            } else {
                self.f.locals.insert(p.local, Local::Plain(name.clone()));
            }
            ps.push((name, Some(ty)));
        }
        let ret_ty = match ret {
            IrType::Void => Type::name("void"),
            other => self.ty(other),
        };
        let result = if body.stmts.is_empty()
            && body
                .tail
                .as_ref()
                .is_some_and(|t| !needs_stmts(t) && !matches!(t.ty, IrType::Never))
        {
            let want = self.ret_repr();
            let mut pre = Vec::new();
            let tail = body.tail.as_ref().ok_or("closure")?;
            let v = self.value(tail, &mut pre, want)?;
            if pre.is_empty() {
                Expr::Arrow(ps, Some(ret_ty), ArrowBody::Expr(Box::new(v)))
            } else {
                pre.push(Stmt::Return(Some(v)));
                Expr::Arrow(ps, Some(ret_ty), ArrowBody::Block(pre))
            }
        } else {
            let target = if matches!(ret, IrType::Void) {
                Target::Discard
            } else {
                Target::Return
            };
            let mut stmts = Vec::new();
            self.block(body, target, &mut stmts)?;
            Expr::Arrow(ps, Some(ret_ty), ArrowBody::Block(stmts))
        };
        self.f.rets.pop();
        Ok(result)
    }

    fn try_(
        &mut self,
        value: &'p IrExpr,
        coerce: Option<&IrType>,
        out: &mut Vec<Stmt>,
    ) -> Result<(Expr, Repr), Error> {
        let (v, repr) = self.native(value, out, Repr::Undef)?;
        let t = self.temp("t");
        out.push(Stmt::Const(t.clone(), None, v));
        let tv = ident(&t);
        match &value.ty {
            IrType::Result(_, err_ty) => {
                let ret = match coerce {
                    Some(to) => {
                        let e = member(tv.clone(), "error");
                        let c = self.coerce(e, err_ty, to)?;
                        self.rt_call("err", vec![c])
                    }
                    None => tv.clone(),
                };
                out.push(Stmt::If(
                    not(member(tv.clone(), "ok")),
                    vec![Stmt::Return(Some(ret))],
                    None,
                ));
                Ok((member(tv, "value"), Repr::Tagged))
            }
            IrType::Option(_) => {
                let none = if self.ret_repr() == Repr::Undef {
                    Expr::Undefined
                } else {
                    self.rt("none")
                };
                let (cond, inner) = match repr {
                    Repr::Undef => (bin(tv.clone(), "===", Expr::Undefined), tv),
                    Repr::Tagged => (not(member(tv.clone(), "some")), member(tv, "value")),
                };
                out.push(Stmt::If(cond, vec![Stmt::Return(Some(none))], None));
                Ok((inner, Repr::Tagged))
            }
            _ => Err("`?` on a value that is neither `Result` nor `Option`".to_string()),
        }
    }

    /// A value of a concrete type used as a value of an interface type.
    fn coerce(&mut self, v: Expr, from: &IrType, to: &IrType) -> Result<Expr, Error> {
        let from = &self.norm(from);
        // A `String` or a collection as a value of `Display` keeps the text of its type: a list
        // of `Display` writes a string without quotes (spec chapter 7, section 4.1).
        if matches!(
            from,
            IrType::String
                | IrType::List(_)
                | IrType::Map(..)
                | IrType::Option(_)
                | IrType::Result(..)
        ) {
            let f = if *from == IrType::String {
                self.rt("display")
            } else {
                self.show_fn(from)
            };
            self.rt.insert("Shown");
            return Ok(Expr::New("$rt.Shown".to_string(), vec![v, f]));
        }
        // A value of a type parameter: the dictionary of its bound makes the interface value.
        // Without one (an implementation of an interface keeps the interface's signature, and a
        // default has none) the value is cast: right for classes, not for a payload-less enum.
        if let (IrType::Param(n), IrType::Named(iface, _)) = (from, to) {
            if let Some(d) = self.f.dicts.get(&(n.clone(), Dict::View(*iface))).cloned() {
                // A dictionary that takes a place gets one of its own for the value.
                let arg = if self.place_view(*iface) {
                    Expr::Object(vec![("v".to_string(), v)])
                } else {
                    v
                };
                return Ok(call(d, vec![arg]));
            }
            return Ok(match self.f.bounds.get(&(n.clone(), *iface)).cloned() {
                Some(b) => Expr::As(Box::new(v), self.iface_type(&b, from)),
                None => v,
            });
        }
        if let (IrType::Named(_, _), IrType::Named(iface, _)) = (from, to) {
            if !self.plain_enum(from) {
                return self.with_head_dicts(v, from, *iface);
            }
        }
        if !self.plain_enum(from) {
            return Ok(v);
        }
        // A payload-less enum is a string: a class of this module holds it and has the
        // interface's methods.
        let (IrType::Named(e, _), IrType::Named(iface, _)) = (from, to) else {
            return Ok(v);
        };
        self.wrappers
            .entry((*e, *iface))
            .or_insert_with(|| (from.clone(), to.clone()));
        Ok(Expr::New(self.wrapper_name(*e, *iface), vec![v]))
    }

    /// The wrapper of a payload-less enum whose value is the place `r` (a `Ref`), for a `mut self`
    /// method of an interface: the change reaches the place.
    fn plain_view(&mut self, r: Expr, from: &IrType, to: &IrType) -> Expr {
        let (IrType::Named(e, _), IrType::Named(iface, _)) = (from, to) else {
            return r;
        };
        self.wrappers
            .entry((*e, *iface))
            .or_insert_with(|| (from.clone(), to.clone()));
        let class = self.wrapper_name(*e, *iface);
        let proto = member(ident(&class), "prototype");
        let view = self.rt_call("viewOf", vec![proto, r]);
        Expr::As(Box::new(view), Type::name(&class))
    }

    /// The super-interfaces of an interface with the type arguments `args`, and theirs in turn,
    /// each once, with their type arguments.
    fn supers_with_args(&self, iface: DefRef, args: Vec<IrType>) -> Vec<(DefRef, Vec<IrType>)> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::from([iface]);
        let mut stack = vec![(iface, args)];
        while let Some((def, args)) = stack.pop() {
            let Some(i) = self.ifaces.get(&def).copied() else {
                continue;
            };
            let map: BTreeMap<String, IrType> = i
                .generics
                .iter()
                .map(|g| g.name.clone())
                .zip(args)
                .collect();
            for s in &i.supers {
                if let IrType::Named(d, a) = subst(s, &map) {
                    if seen.insert(d) && Some(d) != self.comparable {
                        out.push((d, a.clone()));
                        stack.push((d, a));
                    }
                }
            }
        }
        out
    }

    /// The class that holds a payload-less enum used as a value of an interface: `Color$Animal`.
    fn wrapper_name(&self, e: DefRef, iface: DefRef) -> String {
        let name = |d: DefRef| self.p.def(d).map(|d| d.name.clone()).unwrap_or_default();
        format!("{}${}", type_name(&name(e)), type_name(&name(iface)))
    }

    /// The class of [`Self::coerce`]: the value is the field `v`, so that the class is the `Ref`
    /// that a `mut self` method takes, and a copy of the object is a copy of the value.
    fn plain_wrapper(
        &mut self,
        e: DefRef,
        iface: DefRef,
        from: &IrType,
        to: &IrType,
    ) -> Result<Item, Error> {
        let en = self.enums.get(&e).copied().ok_or("enum")?;
        let args = match to {
            IrType::Named(_, a) => a.clone(),
            _ => Vec::new(),
        };
        // The methods of the interface and of its super-interfaces (spec chapter 4, section 8.2,
        // rule 6), with the interface that declares each.
        let mut owners = vec![(iface, args.clone())];
        owners.extend(self.supers_with_args(iface, args));
        let mut names: Vec<(DefRef, Vec<IrType>, String)> = Vec::new();
        for (owner, oargs) in &owners {
            for (i, n) in self.iface_fns.keys() {
                if i == owner && !names.iter().any(|(_, _, m)| m == n) {
                    names.push((*owner, oargs.clone(), n.clone()));
                }
            }
        }
        let mut methods = Vec::new();
        for (owner, oargs, n) in names {
            // A default that the enum gets: called on the wrapper itself.
            if !en.methods.iter().any(|f| f.name == n) {
                let default: Option<&'p ir::Function> = self
                    .ifaces
                    .get(&owner)
                    .copied()
                    .and_then(|i| i.methods.iter().find(|f| f.name == n && f.body.is_some()));
                if let Some(f) = default {
                    methods.push(self.delegate(Expr::This, e, owner, &oargs, f)?);
                    continue;
                }
            }
            let f = en
                .methods
                .iter()
                .find(|f| f.name == n)
                .ok_or("missing method")?;
            let fname = self.def_name_local(f.def, e);
            let params: Vec<(String, Type)> = f
                .params
                .iter()
                .map(|p| (camel(&p.name), self.param_ty(p.mode, &p.ty)))
                .collect();
            let recv = if f.self_mode == Some(Mode::Mut) {
                Expr::This
            } else {
                member(Expr::This, "v")
            };
            let mut args = vec![recv];
            args.extend(params.iter().map(|(n, _)| ident(n)));
            let result = call(ident(&fname), args);
            let body = if matches!(f.ret, IrType::Void) {
                vec![Stmt::Expr(result)]
            } else {
                vec![Stmt::Return(Some(result))]
            };
            methods.push(Method {
                docs: Vec::new(),
                is_static: false,
                name: camel(&n),
                generics: Vec::new(),
                params,
                ret: self.ret_ty(&f.ret),
                body,
            });
        }
        // The text of a `Display` enum, as a class of the enum has it: `errText` of a generic
        // error prefers it to the message.
        let shown = en
            .methods
            .iter()
            .find(|f| f.interface.is_some() && f.interface == self.display && f.body.is_some());
        if let Some(f) = shown.filter(|_| !methods.iter().any(|m| m.name == "toString")) {
            let fname = self.def_name_local(f.def, e);
            methods.push(Method {
                docs: Vec::new(),
                is_static: false,
                name: "toString".to_string(),
                generics: Vec::new(),
                params: Vec::new(),
                ret: Type::name("string"),
                body: vec![Stmt::Return(Some(call(
                    ident(&fname),
                    vec![member(Expr::This, "v")],
                )))],
            });
        }
        let value = self.ty(from);
        let class = self.wrapper_name(e, iface);
        let enum_name = self.p.def(e).map(|d| d.name.clone()).unwrap_or_default();
        let iface_name = self
            .p
            .def(iface)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        let implements = vec![self.iface_type(to, from)];
        Ok(Item::Class {
            docs: vec![format!("`{enum_name}` as a value of `{iface_name}`.")],
            export: false,
            name: class,
            generics: Vec::new(),
            implements,
            fields: vec![("v".to_string(), value.clone(), None)],
            constructor: Some((
                vec![("v".to_string(), value)],
                vec![Stmt::Assign(member(Expr::This, "v"), ident("v"))],
            )),
            methods,
        })
    }

    /// The name of a function of an enum, imported from the enum's module if needed.
    fn def_name_local(&mut self, def: DefRef, owner: DefRef) -> String {
        let name = self.names.get(&def).cloned().unwrap_or_default();
        match self.def_module(owner) {
            Some(m) if m != self.module => {
                if let Some(a) = self.imported.get(&def) {
                    return a.clone();
                }
                self.imported.insert(def, name.clone());
                self.taken.insert(name.clone());
                self.imports
                    .entry(m)
                    .or_default()
                    .insert((name.clone(), name.clone()));
                name
            }
            _ => name,
        }
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
            let t = self.temp("b");
            out.push(Stmt::Let(t.clone(), Some(Type::name("boolean")), Some(l)));
            pre.push(Stmt::Assign(ident(&t), r));
            let cond = if op == BinaryOp::And {
                ident(&t)
            } else {
                not(ident(&t))
            };
            out.push(Stmt::If(cond, pre, None));
            return Ok(ident(&t));
        }
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            // `x == Status.Open` for a variant without payload: compare the `kind`.
            for (a, b) in [(lhs, rhs), (rhs, lhs)] {
                if let ExprKind::Variant { def, values } = &b.kind {
                    if values.is_empty() && !self.plain_enum(&b.ty) {
                        let name = self.p.def(*def).map(|d| d.name.clone()).unwrap_or_default();
                        let x = self.expr(a, out)?;
                        let sym = if op == BinaryOp::Eq { "===" } else { "!==" };
                        return Ok(bin(member(x, "kind"), sym, Expr::Str(name)));
                    }
                }
            }
        }
        let v = self.seq(&[(lhs, Repr::Tagged), (rhs, Repr::Tagged)], out)?;
        let mut it = v.into_iter();
        let (l, r) = (
            it.next().unwrap_or(Expr::Undefined),
            it.next().unwrap_or(Expr::Undefined),
        );
        Ok(self.binary_values(op, l, r, &lhs.ty))
    }

    fn binary_values(&mut self, op: BinaryOp, l: Expr, r: Expr, ty: &IrType) -> Expr {
        use BinaryOp as B;
        let int = matches!(ty, IrType::Int);
        match op {
            B::Add if int => self.rt_call("add", vec![l, r]),
            B::Sub if int => self.rt_call("sub", vec![l, r]),
            B::Mul if int => self.rt_call("mul", vec![l, r]),
            B::Div if int => self.rt_call("div", vec![l, r]),
            B::Rem if int => self.rt_call("rem", vec![l, r]),
            B::Shl => self.rt_call("shl", vec![l, r]),
            B::Shr => self.rt_call("shr", vec![l, r]),
            B::Add => bin(l, "+", r),
            B::Sub => bin(l, "-", r),
            B::Mul => bin(l, "*", r),
            B::Div => bin(l, "/", r),
            B::Rem => bin(l, "%", r),
            B::BitAnd => bin(l, "&", r),
            B::BitOr => bin(l, "|", r),
            B::BitXor => bin(l, "^", r),
            B::Eq | B::NotEq => {
                let e = if self.primitive(ty) {
                    bin(l, "===", r)
                } else {
                    self.rt_call("eq", vec![l, r])
                };
                if op == B::NotEq {
                    match e {
                        Expr::Binary(a, "===", b) => Expr::Binary(a, "!==", b),
                        other => not(other),
                    }
                } else {
                    e
                }
            }
            B::Lt | B::Le | B::Gt | B::Ge => {
                let sym = match op {
                    B::Lt => "<",
                    B::Le => "<=",
                    B::Gt => ">",
                    _ => ">=",
                };
                if matches!(ty, IrType::String) {
                    let c = self.rt_call("cmpStr", vec![l, r]);
                    bin(c, sym, Expr::Num("0".to_string()))
                } else {
                    bin(l, sym, r)
                }
            }
            B::And => bin(l, "&&", r),
            B::Or => bin(l, "||", r),
        }
    }

    // ----- calls ------------------------------------------------------------------------------

    fn call(
        &mut self,
        callee: &'p Callee,
        args: &'p [ir::Arg],
        ty: &IrType,
        out: &mut Vec<Stmt>,
    ) -> Result<(Expr, Repr), Error> {
        let (recv, info, intrinsic) = match callee {
            Callee::Fn(def) => {
                let d = self.p.def(*def).ok_or("unknown function")?;
                (None, self.fns.get(def).cloned(), d.intrinsic.clone())
            }
            Callee::Method { method, recv } => {
                let d = self.p.def(*method).ok_or("unknown method")?;
                (
                    Some(recv),
                    self.fns.get(method).cloned(),
                    d.intrinsic.clone(),
                )
            }
            Callee::Interface {
                interface,
                name,
                recv,
            }
            | Callee::Default {
                interface,
                name,
                recv,
                ..
            } => (
                Some(recv),
                self.iface_fns.get(&(*interface, name.clone())).cloned(),
                None,
            ),
            Callee::Value(f) => {
                let info = match &f.ty {
                    IrType::Fn(params, ret) => Some(FnInfo {
                        params: params.clone(),
                        ret: (**ret).clone(),
                        dicts: Vec::new(),
                        owner: None,
                        self_mode: None,
                        bounds: Vec::new(),
                    }),
                    _ => None,
                };
                (None, info, None)
            }
        };
        // Receiver and arguments, in order.
        let mut items: Vec<(&'p IrExpr, Repr)> = Vec::new();
        if let Some(r) = recv {
            items.push((r, Repr::Tagged));
        }
        let mut refs = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let boxed = a.mode == Mode::Mut
                && info
                    .as_ref()
                    .and_then(|f| f.params.get(i))
                    .is_some_and(|(m, t)| self.boxed(*m, t));
            refs.push(boxed);
            items.push((&a.value, Repr::Tagged));
        }
        let mut values = Vec::new();
        let mut pre_all = Vec::new();
        // References to places are built without evaluating the place.
        let mut plain_items = Vec::new();
        // A payload-less enum is a string: a `mut self` method takes the place, also through an
        // interface (its wrapper then holds the place, see `plain_view`), and so does one of a
        // type parameter, whose dictionary takes the place (see `place_view`).
        let iface_mut = match callee {
            Callee::Interface { interface, .. } | Callee::Default { interface, .. } => {
                info.as_ref()
                    .is_some_and(|f| f.self_mode == Some(Mode::Mut))
                    && recv.is_some_and(|r| {
                        self.plain_enum(&r.ty)
                            || matches!(self.norm(&r.ty), IrType::Param(_))
                                && self.place_view(*interface)
                            || matches!(callee, Callee::Default { .. })
                                && self.self_typed(*interface)
                    })
            }
            _ => false,
        };
        let recv_ref = iface_mut
            || matches!(callee, Callee::Method { method, .. } if self.plain_mut_self(*method));
        for (i, (e, r)) in items.iter().enumerate() {
            let is_ref = recv.is_some() && i == 0 && recv_ref
                || recv.is_some() && i > 0 && refs.get(i - 1).copied().unwrap_or(false)
                || recv.is_none() && refs.get(i).copied().unwrap_or(false);
            if is_ref {
                plain_items.push(None);
                let v = self.ref_to(e, &mut pre_all)?;
                values.push(Some(v));
            } else {
                plain_items.push(Some((*e, *r)));
                values.push(None);
            }
        }
        let evaluated = {
            let list: Vec<(&'p IrExpr, Repr)> = plain_items.iter().flatten().copied().collect();
            self.seq(&list, out)?
        };
        out.extend(pre_all);
        let mut evaluated = evaluated.into_iter();
        let mut all: Vec<Expr> = values
            .into_iter()
            .map(|v| v.unwrap_or_else(|| evaluated.next().unwrap_or(Expr::Undefined)))
            .collect();
        // A variant passed for a parameter of a generic type, also inside an option, a result,
        // a list or a map: TypeScript would infer the type argument from the variant's class,
        // not the enum.
        if let Some(f) = &info {
            let skip = usize::from(recv.is_some());
            for (i, a) in args.iter().enumerate() {
                let generic = f.params.get(i).is_some_and(|(_, t)| generic(t));
                if generic && self.holds_variant(&a.value) {
                    if let Some(v) = all.get_mut(i + skip) {
                        let t = self.ty(&a.value.ty);
                        *v = Expr::As(Box::new(std::mem::replace(v, Expr::Undefined)), t);
                    }
                }
            }
        }
        // Dictionaries, after the receiver.
        if let Some(f) = info.as_ref().filter(|f| !f.dicts.is_empty()) {
            let mut map = BTreeMap::new();
            if let (Some(r), Some((owner, names))) = (recv, &f.owner) {
                if let IrType::Named(d, targs) = &r.ty {
                    if d == owner {
                        for (n, t) in names.iter().zip(targs) {
                            map.insert(n.clone(), t.clone());
                        }
                    }
                }
            }
            for ((_, p), a) in f.params.iter().zip(args) {
                unify(p, &a.value.ty, &mut map);
            }
            unify(&f.ret, ty, &mut map);
            self.unify_bounds(&f.bounds, &mut map);
            let mut ds = Vec::new();
            for (g, d) in &f.dicts {
                let t = map.get(g).cloned().unwrap_or(IrType::Void);
                ds.push(self.dict(*d, &t)?);
            }
            let at = usize::from(recv.is_some()).min(all.len());
            all.splice(at..at, ds);
        }
        let ret_repr = match &info {
            _ if matches!(callee, Callee::Value(_)) => Repr::Tagged,
            Some(f)
                if undef_result(&f.ret)
                    || (intrinsic.is_some() && matches!(f.ret, IrType::Option(_))) =>
            {
                Repr::Undef
            }
            None if intrinsic.is_some() => Repr::Undef,
            _ => Repr::Tagged,
        };
        if let Some(name) = intrinsic {
            // Sorting and the smallest or largest item use the elements' `compare`.
            let short = name.strip_prefix("std.").unwrap_or(&name);
            if matches!(short, "list.List.sort" | "list.List.min" | "list.List.max") {
                let list = recv.map(|r| &r.ty).or(args.first().map(|a| &a.value.ty));
                if let Some(IrType::List(t)) = list {
                    if let Some(c) = self.dict_for(Dict::Cmp, t) {
                        all.push(c);
                    }
                }
            }
            // The text of a collection is composed from the types of its parts.
            if short.ends_with(".to_string")
                && matches!(
                    recv.map(|r| &r.ty),
                    Some(
                        IrType::List(_) | IrType::Map(..) | IrType::Option(_) | IrType::Result(..)
                    )
                )
            {
                if let Some(r) = recv {
                    let f = self.show_fn(&r.ty);
                    let x = all
                        .drain(..)
                        .next()
                        .ok_or("`to_string` without a receiver")?;
                    return Ok((call(f, vec![x]), Repr::Tagged));
                }
            }
            let v = self.intrinsic(&name, all, ty)?;
            let repr = if matches!(ty, IrType::Option(_)) {
                Repr::Undef
            } else {
                Repr::Tagged
            };
            return Ok((v, repr));
        }
        let e = match callee {
            Callee::Fn(def) => {
                let d = self.p.def(*def).ok_or("unknown function")?.clone();
                match d.kind {
                    DefKind::Method { owner, .. } if self.structs.contains_key(&owner) => {
                        let o = self.def_name(owner);
                        call(member(ident(&o), &camel(&d.name)), all)
                    }
                    DefKind::Method { owner, .. } => {
                        let n = self.def_name_local(*def, owner);
                        call(ident(&n), all)
                    }
                    _ => {
                        let n = self.def_name(*def);
                        call(ident(&n), all)
                    }
                }
            }
            Callee::Method { method, recv } => {
                let d = self.p.def(*method).ok_or("unknown method")?.clone();
                let r = if all.is_empty() {
                    Expr::Undefined
                } else {
                    all.remove(0)
                };
                // A method of an implementation that takes dictionaries from the head of its
                // block: the receiver gets them first.
                let iface = match d.kind {
                    DefKind::Method { owner, .. } => self
                        .structs
                        .get(&owner)
                        .map(|s| &s.methods)
                        .or_else(|| self.enums.get(&owner).map(|e| &e.methods))
                        .and_then(|ms| ms.iter().find(|f| f.def == *method))
                        .and_then(|f| f.interface),
                    _ => None,
                };
                let r = match iface {
                    Some(i) if !matches!(recv.ty, IrType::Param(_)) => {
                        self.with_head_dicts(r, &recv.ty, i)?
                    }
                    _ => r,
                };
                match d.kind {
                    DefKind::Method { owner, .. } if self.structs.contains_key(&owner) => {
                        call(member(r, &camel(&d.name)), all)
                    }
                    DefKind::Method { owner, .. } => {
                        let n = self.def_name_local(*method, owner);
                        let mut a = vec![r];
                        a.extend(all);
                        call(ident(&n), a)
                    }
                    _ => return Err("method of an unknown type".to_string()),
                }
            }
            Callee::Interface {
                interface,
                name,
                recv,
            } => {
                let r = if all.is_empty() {
                    Expr::Undefined
                } else {
                    all.remove(0)
                };
                let param = matches!(recv.ty, IrType::Param(_));
                if Some(*interface) == self.display && name == "to_string" {
                    if param {
                        let f = self.dict(Dict::Show, &recv.ty)?;
                        call(f, vec![r])
                    } else {
                        let r = match self.display {
                            Some(i) => self.with_head_dicts(r, &recv.ty, i)?,
                            None => r,
                        };
                        self.rt_call("display", vec![r])
                    }
                } else if param && Some(*interface) == self.comparable && name == "compare" {
                    let mut a = vec![r];
                    a.extend(all);
                    let f = self.dict(Dict::Cmp, &recv.ty)?;
                    call(f, a)
                } else {
                    // A payload-less enum that gets a default is a string: its wrapper has the
                    // method. A value of a type parameter gets it from its dictionary.
                    let to = IrType::Named(*interface, self.iface_args(*interface, &recv.ty));
                    let r = if iface_mut {
                        self.view_place(r, &recv.ty, &to)?
                    } else {
                        self.coerce(r, &recv.ty, &to)?
                    };
                    call(member(r, &camel(name)), all)
                }
            }
            Callee::Default {
                interface,
                name,
                recv,
                ..
            } => {
                let r = if all.is_empty() {
                    Expr::Undefined
                } else {
                    all.remove(0)
                };
                let iargs = self.iface_args(*interface, &recv.ty);
                let to = IrType::Named(*interface, iargs.clone());
                // The default of an interface that mentions `Self` takes the value itself (its
                // place for `mut self`) and the dictionary of its type.
                let self_typed = self.self_typed(*interface);
                let r = if self_typed {
                    r
                } else if iface_mut {
                    self.view_place(r, &recv.ty, &to)?
                } else {
                    self.coerce(r, &recv.ty, &to)?
                };
                let mut a = vec![r];
                let iface = self.ifaces.get(interface).copied();
                let default = iface.and_then(|i| i.methods.iter().find(|f| f.name == *name));
                if let (Some(i), Some(f)) = (iface, default) {
                    let mut map: BTreeMap<String, IrType> = i
                        .generics
                        .iter()
                        .map(|g| g.name.clone())
                        .zip(iargs)
                        .collect();
                    map.insert("Self".to_string(), recv.ty.clone());
                    a.extend(self.default_dicts(f, &map)?);
                }
                a.extend(all);
                call(self.default_fn(*interface, name), a)
            }
            Callee::Value(f) => {
                let fv = self.expr(f, out)?;
                call(fv, all)
            }
        };
        Ok((e, ret_repr))
    }

    /// A `Ref` to the place an argument names, for a `mut` parameter.
    fn ref_to(&mut self, e: &'p IrExpr, out: &mut Vec<Stmt>) -> Result<Expr, Error> {
        match &e.kind {
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Boxed(n)) => Ok(ident(&n)),
                Some(Local::Plain(n)) => Ok(Expr::RefTo(Box::new(ident(&n)))),
                Some(Local::Alias(Place::Expr(p))) => Ok(self.ref_of(p, out)),
                Some(Local::Alias(Place::Elem(l, i))) => {
                    Ok(self.ref_of(Expr::Index(Box::new(l), Box::new(i)), out))
                }
                None => Err("unknown local".to_string()),
            },
            // `self` of a `mut self` method of a payload-less enum is already a `Ref`.
            ExprKind::SelfValue if self.f.self_by_ref => Ok(ident("self")),
            ExprKind::Field { base, name } => {
                let b = self.expr(base, out)?;
                let b = self.stable(b, "base", out);
                Ok(self.ref_of(member(b, &camel(name)), out))
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base, out)?;
                let i = self.expr(index, out)?;
                let f = if matches!(base.ty, IrType::Map(..)) {
                    "refKey"
                } else {
                    "refAt"
                };
                Ok(self.rt_call(f, vec![b, i]))
            }
            _ => Err("a `mut` argument that is not a place".to_string()),
        }
    }

    /// `Expr::RefTo` of a place; a place that reads `this` reads it through a constant, since
    /// inside the getter and the setter `this` is the `Ref` itself.
    fn ref_of(&mut self, place: Expr, out: &mut Vec<Stmt>) -> Expr {
        let place = match place {
            Expr::Member(base, name) if uses_this(&base) => {
                let t = self.temp("this");
                out.push(Stmt::Const(t.clone(), None, *base));
                member(ident(&t), &name)
            }
            Expr::Index(base, i) if uses_this(&base) || uses_this(&i) => {
                let mut hoist = |e: Expr, out: &mut Vec<Stmt>| {
                    if uses_this(&e) {
                        let t = self.temp("this");
                        out.push(Stmt::Const(t.clone(), None, e));
                        ident(&t)
                    } else {
                        e
                    }
                };
                let b = hoist(*base, out);
                let i = hoist(*i, out);
                Expr::Index(Box::new(b), Box::new(i))
            }
            other => other,
        };
        Expr::RefTo(Box::new(place))
    }

    /// A call of an intrinsic of std, with the receiver first.
    fn intrinsic(&mut self, name: &str, mut a: Vec<Expr>, ty: &IrType) -> Result<Expr, Error> {
        let short = name.strip_prefix("std.").unwrap_or(name);
        let arg = |a: &mut Vec<Expr>| {
            if a.is_empty() {
                Expr::Undefined
            } else {
                a.remove(0)
            }
        };
        let len = |e: Expr| call(ident("BigInt"), vec![member(e, "length")]);
        Ok(match short {
            "io.print" => self.rt_call("print", a),
            "io.write" => self.rt_call("write", a),
            "io.eprint" => self.rt_call("eprint", a),
            "io.read_line" => self.rt_call("readLine", a),
            "io.args" => self.rt_call("args", a),
            "process.exit" => self.rt_call("exit", a),
            "process.env" => self.rt_call("env", a),
            "prelude.replace" => self.rt_call("replace", a),
            "prelude.error" => self.rt_call("textError", a),
            "prelude.Bool.to_string" => call(ident("String"), a),
            "string.String.to_string" => arg(&mut a),
            "math.Int.to_string" => call(member(arg(&mut a), "toString"), Vec::new()),
            "math.Int.to_float" => call(ident("Number"), a),
            "math.Int.abs" => self.rt_call("intAbs", a),
            "math.Int.pow" => self.rt_call("intPow", a),
            "math.Int.min" | "math.Int.max" => {
                let x = arg(&mut a);
                let y = arg(&mut a);
                let op = if short.ends_with("min") { "<" } else { ">" };
                if pure(&x) && pure(&y) {
                    Expr::Cond(
                        Box::new(bin(x.clone(), op, y.clone())),
                        Box::new(x),
                        Box::new(y),
                    )
                } else {
                    let f = if short.ends_with("min") {
                        "listMin"
                    } else {
                        "listMax"
                    };
                    let r = self.rt_call(f, vec![Expr::Array(vec![x, y])]);
                    Expr::As(Box::new(r), Type::name("bigint"))
                }
            }
            "math.Int.compare" | "string.String.compare" => self.rt_call("compare", a),
            "math.Float.to_string" => self.rt_call("floatToString", a),
            "math.Float.to_int" => self.rt_call("floatToInt", a),
            "math.Float.abs" => call(member(ident("Math"), "abs"), a),
            "math.Float.min" => self.rt_call("floatMin", a),
            "math.Float.max" => self.rt_call("floatMax", a),
            "math.Float.pow" => call(member(ident("Math"), "pow"), a),
            "math.Float.sqrt" => call(member(ident("Math"), "sqrt"), a),
            "math.Float.floor" => call(member(ident("Math"), "floor"), a),
            "math.Float.ceil" => call(member(ident("Math"), "ceil"), a),
            "math.Float.round" => self.rt_call("floatRound", a),
            "math.Float.is_nan" => call(member(ident("Number"), "isNaN"), a),
            "string.String.len" => self.rt_call("strLen", a),
            "string.String.is_empty" => bin(
                member(arg(&mut a), "length"),
                "===",
                Expr::Num("0".to_string()),
            ),
            "string.String.contains" => {
                let s = arg(&mut a);
                call(member(s, "includes"), a)
            }
            "string.String.starts_with" => {
                let s = arg(&mut a);
                call(member(s, "startsWith"), a)
            }
            "string.String.ends_with" => {
                let s = arg(&mut a);
                call(member(s, "endsWith"), a)
            }
            "string.String.find" => self.rt_call("strFind", a),
            "string.String.split" => self.rt_call("strSplit", a),
            "string.String.lines" => self.rt_call("strLines", a),
            "string.String.chars" => call(member(ident("Array"), "from"), a),
            "string.String.trim" => self.rt_call("strTrim", a),
            "string.String.replace" => self.rt_call("strReplace", a),
            "string.String.repeat" => self.rt_call("strRepeat", a),
            "string.String.to_upper_ascii" => self.rt_call("strUpper", a),
            "string.String.to_lower_ascii" => self.rt_call("strLower", a),
            "string.String.parse_int" => self.std_fn("std.string", "stringParseInt", a),
            "string.String.parse_float" => self.std_fn("std.string", "stringParseFloat", a),
            "string.join" => {
                let parts = arg(&mut a);
                call(member(parts, "join"), a)
            }
            "string.fixed" => self.rt_call("fixed", a),
            "string.pad" => self.rt_call("pad", a),
            "list.List.new" => {
                let t = self.ty(ty);
                Expr::As(Box::new(Expr::Array(Vec::new())), t)
            }
            "list.List.len" => len(arg(&mut a)),
            "list.List.is_empty" => bin(
                member(arg(&mut a), "length"),
                "===",
                Expr::Num("0".to_string()),
            ),
            "list.List.push" => {
                let l = arg(&mut a);
                call(member(l, "push"), a)
            }
            "list.List.pop" => call(member(arg(&mut a), "pop"), Vec::new()),
            "list.List.insert" => self.rt_call("listInsert", a),
            "list.List.remove" => self.rt_call("listRemove", a),
            "list.List.swap" => self.rt_call("listSwap", a),
            "list.List.reverse" => call(member(arg(&mut a), "reverse"), Vec::new()),
            "list.List.clear" => call(
                member(arg(&mut a), "splice"),
                vec![Expr::Num("0".to_string())],
            ),
            "list.List.append" => {
                let l = arg(&mut a);
                let other = arg(&mut a);
                call(member(l, "push"), vec![Expr::Spread(Box::new(other))])
            }
            "list.List.map" => {
                let l = arg(&mut a);
                let f = arg(&mut a);
                call(member(l, "map"), vec![self.unary_fn(f)])
            }
            "list.List.filter" => {
                let l = arg(&mut a);
                let f = arg(&mut a);
                call(member(l, "filter"), vec![self.unary_fn(f)])
            }
            "list.List.any" => {
                let l = arg(&mut a);
                let f = arg(&mut a);
                call(member(l, "some"), vec![self.unary_fn(f)])
            }
            "list.List.all" => {
                let l = arg(&mut a);
                let f = arg(&mut a);
                call(member(l, "every"), vec![self.unary_fn(f)])
            }
            "list.List.count" => self.rt_call("listCount", a),
            "list.List.contains" => self.rt_call("listContains", a),
            "list.List.index_of" => self.rt_call("listIndexOf", a),
            "list.List.get" => self.rt_call("listGet", a),
            "list.List.sort" => self.rt_call("listSort", a),
            "list.sort_by" => self.rt_call("listSortBy", a),
            "list.List.min" => self.rt_call("listMin", a),
            "list.List.max" => self.rt_call("listMax", a),
            "list.sum" => self.rt_call("listSum", a),
            "list.range" => self.rt_call("listRange", a),
            "map.Map.new" => {
                let (k, v) = match ty {
                    IrType::Map(k, v) => (self.ty(k), self.ty(v)),
                    _ => (Type::name("never"), Type::name("never")),
                };
                Expr::New(format!("Map<{}, {}>", ts::ty(&k), ts::ty(&v)), Vec::new())
            }
            "map.Map.len" => call(ident("BigInt"), vec![member(arg(&mut a), "size")]),
            "map.Map.is_empty" => bin(
                member(arg(&mut a), "size"),
                "===",
                Expr::Num("0".to_string()),
            ),
            "map.Map.contains_key" => {
                let m = arg(&mut a);
                call(member(m, "has"), a)
            }
            "map.Map.insert" => self.rt_call("mapInsert", a),
            "map.Map.remove" => self.rt_call("mapRemove", a),
            "map.Map.keys" => Expr::Array(vec![Expr::Spread(Box::new(call(
                member(arg(&mut a), "keys"),
                Vec::new(),
            )))]),
            "map.Map.entries" => self.std_fn("std.map", "mapEntries", a),
            "map.Map.clear" => call(member(arg(&mut a), "clear"), Vec::new()),
            "map.get" => self.rt_call("mapGetCopy", a),
            "map.values" => self.rt_call("mapValues", a),
            "fs.read_text" => self.std_fn("std.fs", "fsReadText", a),
            "fs.write_text" => self.std_fn("std.fs", "fsWriteText", a),
            "fs.exists" => self.std_fn("std.fs", "fsExists", a),
            other => return Err(format!("the intrinsic `{other}` for TypeScript")),
        })
    }

    /// A function value passed to `Array.map` and friends, which also pass the index: wrap
    /// named functions so that they get only the element.
    fn unary_fn(&self, f: Expr) -> Expr {
        match f {
            Expr::Arrow(..) => f,
            other => Expr::Arrow(
                vec![("x".to_string(), None)],
                None,
                ArrowBody::Expr(Box::new(call(other, vec![ident("x")]))),
            ),
        }
    }

    /// A function of a std module file, importing it.
    fn std_fn(&mut self, module: &str, name: &'static str, args: Vec<Expr>) -> Expr {
        let index = self
            .p
            .modules
            .iter()
            .position(|m| m.role == ModuleRole::Std && m.path.join(".") == module);
        if let Some(i) = index {
            self.snippets.entry(i).or_default().insert(name);
            self.wanted.insert(i);
            if i != self.module {
                self.imports
                    .entry(i)
                    .or_default()
                    .insert((name.to_string(), name.to_string()));
            }
        }
        call(ident(name), args)
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
        let (raw, repr) = self.native(scrutinee, out, Repr::Undef)?;
        let is_place = matches!(
            scrutinee.kind,
            ExprKind::Local(_) | ExprKind::Field { .. } | ExprKind::SelfValue
        );
        // `match mut` changes the place itself, unless the place is an object whose payload the
        // arms change: then a temporary names the same object and lets TypeScript narrow it
        // (`(items[$i] as Status)` is not narrowed).
        let object = matches!(&scrutinee.ty, IrType::Named(d, _)
            if self.structs.contains_key(d) || self.enums.contains_key(d));
        let whole = arms.iter().any(|a| matches!(a.pattern, Pattern::Bind(_)));
        let subject = if pure(&raw) || (mode == Mode::Mut && is_place && (whole || !object)) {
            raw
        } else {
            let t = self.temp("m");
            out.push(Stmt::Const(t.clone(), None, raw));
            ident(&t)
        };
        let guarded = arms.iter().any(|a| a.guard.is_some());
        // Each alternative of `|` with bindings becomes its own branch.
        let mut branches: Vec<Branch<'p>> = Vec::new();
        for arm in arms {
            let alts: Vec<&Pattern> = match &arm.pattern {
                Pattern::Or(alts) if alts.iter().any(binds) => alts.iter().collect(),
                p => vec![p],
            };
            for alt in alts {
                let mut conds = Vec::new();
                let mut bindings = Vec::new();
                self.test(
                    alt,
                    subject.clone(),
                    &scrutinee.ty,
                    repr,
                    &mut conds,
                    &mut bindings,
                )?;
                branches.push((conds, bindings, arm));
            }
        }
        if guarded {
            let label = self.temp("match");
            let mut body = Vec::new();
            for (conds, bindings, arm) in branches {
                let mut inner = Vec::new();
                self.bind(&bindings, mode, &mut inner);
                let mut b = Vec::new();
                self.block(&arm.body, target.clone(), &mut b)?;
                b.push(Stmt::Break(Some(label.clone())));
                match &arm.guard {
                    Some(g) => {
                        let gv = self.expr(g, &mut inner)?;
                        inner.push(Stmt::If(gv, b, None));
                    }
                    None => inner.extend(b),
                }
                match and_all(conds) {
                    Some(c) => body.push(Stmt::If(c, inner, None)),
                    None => body.push(Stmt::Block(inner)),
                }
            }
            let unreachable = self.rt_call("unreachable", Vec::new());
            body.push(Stmt::Throw(unreachable));
            out.push(Stmt::Labeled(label, Box::new(Stmt::Block(body))));
            return Ok(());
        }
        // An `if` chain; the last branch without conditions is a plain `else`. When every arm
        // only tests the top-level variant, TypeScript narrows the last branch by elimination,
        // so it needs no condition either.
        let by_elimination = arms.iter().all(|a| top_only(&a.pattern));
        let mut chain: Option<Vec<Stmt>> = None;
        let mut first = true;
        for (conds, bindings, arm) in branches.into_iter().rev() {
            let mut b = Vec::new();
            self.bind(&bindings, mode, &mut b);
            self.block(&arm.body, target.clone(), &mut b)?;
            let conds = if first && by_elimination {
                Vec::new()
            } else {
                conds
            };
            match and_all(conds) {
                None => chain = Some(b),
                Some(c) => {
                    let otherwise = match chain.take() {
                        Some(rest) if rest.is_empty() => None,
                        Some(rest) => Some(rest),
                        None if first => {
                            let u = self.rt_call("unreachable", Vec::new());
                            Some(vec![Stmt::Throw(u)])
                        }
                        None => None,
                    };
                    chain = Some(vec![Stmt::If(c, b, otherwise)]);
                }
            }
            first = false;
        }
        if let Some(stmts) = chain {
            out.extend(stmts);
        }
        Ok(())
    }

    /// The items of a chain of conditions from the first one: each is an `if` around the rest,
    /// and the innermost runs `then` and leaves the block `label` (spec chapter 3, section 5,
    /// rule 14a). Alternatives of `|` with bindings are tried one after the other, as in a
    /// `match`.
    fn chain_items(
        &mut self,
        items: &'p [CondItem],
        then: &'p Block,
        label: Option<&str>,
        target: &Target,
        out: &mut Vec<Stmt>,
    ) -> Result<(), Error> {
        let Some((first, rest)) = items.split_first() else {
            self.block(then, target.clone(), out)?;
            if let Some(label) = label {
                if !matches!(
                    out.last(),
                    Some(Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_))
                ) {
                    out.push(Stmt::Break(Some(label.to_string())));
                }
            }
            return Ok(());
        };
        match first {
            CondItem::Bool(cond) => {
                let c = self.expr(cond, out)?;
                let mut inner = Vec::new();
                self.chain_items(rest, then, label, target, &mut inner)?;
                out.push(Stmt::If(c, inner, None));
            }
            CondItem::Let {
                pattern,
                mode,
                value,
            } => {
                let (raw, repr) = self.native(value, out, Repr::Undef)?;
                let is_place = matches!(
                    value.kind,
                    ExprKind::Local(_) | ExprKind::Field { .. } | ExprKind::SelfValue
                );
                let object = matches!(&value.ty, IrType::Named(d, _)
                    if self.structs.contains_key(d) || self.enums.contains_key(d));
                let whole = matches!(pattern, Pattern::Bind(_));
                let subject =
                    if pure(&raw) || (*mode == Mode::Mut && is_place && (whole || !object)) {
                        raw
                    } else {
                        let t = self.temp("m");
                        out.push(Stmt::Const(t.clone(), None, raw));
                        ident(&t)
                    };
                let alts: Vec<&Pattern> = match pattern {
                    Pattern::Or(alts) if alts.iter().any(binds) => alts.iter().collect(),
                    p => vec![p],
                };
                for alt in alts {
                    let mut conds = Vec::new();
                    let mut bindings = Vec::new();
                    self.test(
                        alt,
                        subject.clone(),
                        &value.ty,
                        repr,
                        &mut conds,
                        &mut bindings,
                    )?;
                    let mut inner = Vec::new();
                    self.bind(&bindings, *mode, &mut inner);
                    self.chain_items(rest, then, label, target, &mut inner)?;
                    match and_all(conds) {
                        Some(c) => out.push(Stmt::If(c, inner, None)),
                        None => out.push(Stmt::Block(inner)),
                    }
                }
            }
        }
        Ok(())
    }

    fn bind(&mut self, bindings: &[(LocalRef, Expr)], mode: Mode, out: &mut Vec<Stmt>) {
        for (local, path) in bindings {
            if mode == Mode::Mut {
                self.f
                    .locals
                    .insert(*local, Local::Alias(Place::Expr(path.clone())));
            } else {
                let name = self.local_name(*local);
                out.push(Stmt::Const(name.clone(), None, path.clone()));
                self.f.locals.insert(*local, Local::Plain(name));
            }
        }
    }

    /// The conditions under which `subject` matches `p`, and the bindings it makes.
    fn test(
        &mut self,
        p: &Pattern,
        subject: Expr,
        ty: &IrType,
        repr: Repr,
        conds: &mut Vec<Expr>,
        bindings: &mut Vec<(LocalRef, Expr)>,
    ) -> Result<(), Error> {
        match p {
            Pattern::Wild => {}
            Pattern::Bind(l) => {
                let value = if matches!(ty, IrType::Option(_)) && repr == Repr::Undef {
                    self.rt_call("tagged", vec![subject])
                } else {
                    subject
                };
                bindings.push((*l, value));
            }
            Pattern::Int(v) => conds.push(bin(subject, "===", Expr::BigInt(*v))),
            Pattern::Str(s) => conds.push(bin(subject, "===", Expr::Str(s.clone()))),
            Pattern::Bool(true) => conds.push(subject),
            Pattern::Bool(false) => conds.push(not(subject)),
            Pattern::Const(def) => {
                let n = self.def_name(*def);
                conds.push(bin(subject, "===", ident(&n)));
            }
            Pattern::None => match repr {
                Repr::Undef => conds.push(bin(subject, "===", Expr::Undefined)),
                Repr::Tagged => conds.push(not(member(subject, "some"))),
            },
            Pattern::Some(inner) => {
                let t = match ty {
                    IrType::Option(t) => (**t).clone(),
                    _ => IrType::Void,
                };
                let sub = match repr {
                    Repr::Undef => {
                        conds.push(bin(subject.clone(), "!==", Expr::Undefined));
                        subject
                    }
                    Repr::Tagged => {
                        conds.push(member(subject.clone(), "some"));
                        member(subject, "value")
                    }
                };
                self.test(inner, sub, &t, Repr::Tagged, conds, bindings)?;
            }
            Pattern::Ok(inner) | Pattern::Err(inner) => {
                let (a, b) = match ty {
                    IrType::Result(a, b) => ((**a).clone(), (**b).clone()),
                    _ => (IrType::Void, IrType::Void),
                };
                let ok = matches!(p, Pattern::Ok(_));
                conds.push(if ok {
                    member(subject.clone(), "ok")
                } else {
                    not(member(subject.clone(), "ok"))
                });
                let (sub, t) = if ok {
                    (member(subject, "value"), a)
                } else {
                    (member(subject, "error"), b)
                };
                self.test(inner, sub, &t, Repr::Tagged, conds, bindings)?;
            }
            Pattern::Variant { def, fields } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("pattern of an unknown variant".to_string()),
                };
                let en = self
                    .enums
                    .get(&parent)
                    .copied()
                    .ok_or("pattern of an unknown enum")?;
                let vname = self.p.def(*def).map(|d| d.name.clone()).unwrap_or_default();
                let plain = en.variants.iter().all(|v| v.payload.is_empty());
                if plain {
                    conds.push(bin(subject, "===", Expr::Str(vname)));
                    return Ok(());
                }
                conds.push(bin(
                    member(subject.clone(), "kind"),
                    "===",
                    Expr::Str(vname),
                ));
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
                let (names, types): (Vec<String>, Vec<IrType>) = match &variant.payload {
                    Payload::None => (Vec::new(), Vec::new()),
                    Payload::Tuple(t) => {
                        ((0..t.len()).map(|i| format!("_{i}")).collect(), t.clone())
                    }
                    Payload::Struct(f) => (
                        f.iter().map(|f| camel(&f.name)).collect(),
                        f.iter().map(|f| f.ty.clone()).collect(),
                    ),
                };
                for ((sub, n), t) in fields.iter().zip(&names).zip(&types) {
                    let t = subst(t, &map);
                    self.test(
                        sub,
                        member(subject.clone(), n),
                        &t,
                        Repr::Tagged,
                        conds,
                        bindings,
                    )?;
                }
            }
            Pattern::Struct { def, fields } => {
                let s = self
                    .structs
                    .get(def)
                    .copied()
                    .ok_or("pattern of an unknown struct")?;
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
                    self.test(
                        sub,
                        member(subject.clone(), &camel(&f.name)),
                        &t,
                        Repr::Tagged,
                        conds,
                        bindings,
                    )?;
                }
            }
            Pattern::Or(alts) => {
                let mut any = Vec::new();
                for alt in alts {
                    let mut c = Vec::new();
                    let mut b = Vec::new();
                    self.test(alt, subject.clone(), ty, repr, &mut c, &mut b)?;
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

/// The conditions and bindings of one alternative of a `match` arm.
type Branch<'p> = (Vec<Expr>, Vec<(LocalRef, Expr)>, &'p Arm);

/// An assignment target.
enum PlaceKind {
    Simple(Expr),
    /// A `mut` parameter passed by reference, or `self`: assigned as a whole.
    Whole(Expr, IrType),
    Elem(Expr, Expr),
    Key(Expr, Expr),
}

/// Whether statements hold a `break label`.
fn breaks_to(stmts: &[Stmt], label: &str) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Break(Some(l)) => l == label,
        Stmt::If(_, a, b) => breaks_to(a, label) || b.as_ref().is_some_and(|b| breaks_to(b, label)),
        Stmt::Block(b) | Stmt::While(_, b) | Stmt::For(_, _, _, b) | Stmt::ForOf(_, _, b) => {
            breaks_to(b, label)
        }
        Stmt::Labeled(_, s) => breaks_to(std::slice::from_ref(s), label),
        Stmt::Try(a, _, b) => breaks_to(a, label) || breaks_to(b, label),
        _ => false,
    })
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

/// Patterns that test only the top-level variant (or nothing), so that TypeScript can narrow the
/// last branch of a chain by elimination.
fn top_only(p: &Pattern) -> bool {
    let irrefutable = |p: &Pattern| matches!(p, Pattern::Wild | Pattern::Bind(_));
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

fn float_lit(v: f64) -> Expr {
    if v.is_nan() {
        Expr::Ident("NaN".to_string())
    } else if v.is_infinite() {
        Expr::Ident(if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string())
    } else {
        Expr::Num(format!("{v:?}"))
    }
}

/// Registers the tests of a module with `node:test`.
const REGISTER: &str = "for (const t of $tests()) {\n  test(t.name, t.run);\n}";

/// Whether generated items use the runtime.
fn items_use_rt(items: &[Item]) -> bool {
    let text = ts::print(&ts::File {
        header: String::new(),
        items: items.to_vec(),
    });
    text.contains("$rt.")
}

/// The import path of `to` from the file `from`, both relative to the project root.
pub(crate) fn relative(from: &str, to: &str) -> String {
    let mut from_dir: Vec<&str> = from.split('/').collect();
    from_dir.pop();
    let to_parts: Vec<&str> = to.split('/').collect();
    let mut common = 0;
    while common < from_dir.len()
        && common + 1 < to_parts.len()
        && from_dir.get(common) == to_parts.get(common)
    {
        common += 1;
    }
    let ups = from_dir.len() - common;
    let mut parts: Vec<String> = Vec::new();
    if ups == 0 {
        parts.push(".".to_string());
    } else {
        for _ in 0..ups {
            parts.push("..".to_string());
        }
    }
    for p in to_parts.iter().skip(common) {
        parts.push(p.to_string());
    }
    let joined = parts.join("/");
    match joined.strip_suffix(".ts") {
        Some(stem) => format!("{stem}.js"),
        None => joined,
    }
}

pub(crate) fn runtime_file(used: &BTreeSet<&'static str>, version: &str) -> String {
    format!(
        "// Generated by wolf {version}. Do not edit.\n// The runtime of wolfpack programs in TypeScript.\n\n{}",
        runtime::file(used)
    )
}

/// The type parameters of a type that a function of one of its blocks declares: all of them, or
/// for a block of one instantiation those that its head keeps as parameters (spec chapter 4,
/// section 8.3, rule 8).
fn instance_generics<G: Clone>(
    generics: &[(String, G)],
    instance: &Option<Vec<IrType>>,
) -> Vec<(String, G)> {
    match instance {
        Some(args) => generics
            .iter()
            .filter(|(n, _)| args.iter().any(|a| matches!(a, IrType::Param(p) if p == n)))
            .cloned()
            .collect(),
        None => generics.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::relative;

    #[test]
    fn import_paths_are_relative_and_end_in_js() {
        assert_eq!(relative("src/main.ts", "src/wolf_rt.ts"), "./wolf_rt.js");
        assert_eq!(
            relative("src/todo/store.ts", "src/todo/model.ts"),
            "./model.js"
        );
        assert_eq!(
            relative("src/todo/store.ts", "src/wolf_rt.ts"),
            "../wolf_rt.js"
        );
        assert_eq!(
            relative("examples/demo.ts", "src/text/words.ts"),
            "../src/text/words.js"
        );
        assert_eq!(
            relative("src/std/string.ts", "src/std/prelude.ts"),
            "./prelude.js"
        );
    }
}
