//! IR to the Rust tree (spec chapter 8).
//!
//! Main decisions (ADR 0010):
//!
//! - Every expression is emitted for the context it is used in: an owned value (`own`
//!   parameters, `let`, results, fields), a value for a read parameter (copy types by value,
//!   everything else by reference), a `&mut` for a `mut` parameter, or an operand. The context is
//!   pushed into the branches of `if`, `match` and blocks.
//! - How an argument is passed depends on the parameter's declared type: a concrete copy type by
//!   value, a type parameter or any other type by reference. Read parameters of function types are
//!   always references (`dyn Fn(&i64)`).
//! - Recursive fields and payloads are boxed; values of interface types are `Box<dyn Trait>`.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ir::{
    self as ir, Arm, BinaryOp, Block, Callee, DefKind, DefRef, Expr as IrExpr, ExprKind, LocalRef,
    Mode, ModuleRole, Pattern, Payload, Program, ProgramKind, Stmt as IrStmt, Type as IrType,
    UnaryOp,
};

use crate::names::{constant, type_name, value};
use crate::rs::{self, Expr, Item, Pat, Stmt, Type};

/// A construct the backend does not support yet.
pub type Error = String;

/// The context an expression is emitted for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    /// An owned value.
    Own,
    /// A value for a read parameter: by reference if the flag is set.
    Read(bool),
    /// `&mut`
    Mut,
    /// An operand of an operator, a receiver or the base of a field.
    Op,
}

/// How a local is reached.
#[derive(Clone, Debug)]
enum Local {
    /// A variable that owns the value.
    Val(String),
    /// A `&T`.
    Ref(String),
    /// A `&mut T`.
    Mut(String),
    /// A `Box<T>` taken out of a recursive payload by `match own`.
    Boxed(String),
}

/// A test of a test build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestEntry {
    /// The cargo target: `lib`, `bin` or the name of a file of `tests/`.
    pub target: String,
    /// The path of the test function in its crate.
    pub path: String,
    /// `src/main.wolf: adds two numbers`
    pub name: String,
}

#[derive(Clone)]
struct FnInfo {
    params: Vec<(Mode, IrType)>,
    ret: IrType,
    owner: Option<DefRef>,
    self_mode: Option<Mode>,
    /// An `own self` method of an interface: it takes `self: Box<Self>` (spec chapter 4, 8.4).
    boxed_self: bool,
}

#[derive(Default)]
struct FnState {
    locals: BTreeMap<LocalRef, Local>,
    reads: BTreeSet<LocalRef>,
    mutated: BTreeSet<LocalRef>,
    self_local: Option<Local>,
    /// Read parameters of type `String` or `List`: `&str` or `&[T]`.
    unsized_params: BTreeSet<LocalRef>,
    /// Integer literals get a suffix (they reach a type parameter).
    suffix: bool,
}

pub(crate) struct Emitter<'p> {
    p: &'p Program,
    tests: bool,
    /// The crate name of a library (for examples and integration tests).
    crate_name: String,
    fns: BTreeMap<DefRef, FnInfo>,
    iface_fns: BTreeMap<(DefRef, String), FnInfo>,
    enums: BTreeMap<DefRef, &'p ir::Enum>,
    structs: BTreeMap<DefRef, &'p ir::Struct>,
    ifaces: BTreeMap<DefRef, &'p ir::Interface>,
    names: BTreeMap<DefRef, String>,
    /// Declarations of std: their path in the runtime and the piece that holds them.
    std_names: BTreeMap<DefRef, (String, &'static str)>,
    /// Names the root of a library exports again.
    reexported: BTreeMap<DefRef, String>,
    /// The module path of each module (`crate::todo::model`), if it is emitted.
    mod_paths: Vec<Option<Vec<String>>>,
    /// Fields and payloads that are boxed: (type, field index).
    boxed: BTreeSet<(DefRef, usize)>,
    /// Types that derive `Clone` and `PartialEq`.
    clone: BTreeSet<DefRef>,
    eq: BTreeSet<DefRef>,
    /// Interfaces used as types of values.
    dyn_ifaces: BTreeSet<DefRef>,
    error_iface: Option<DefRef>,
    comparable: Option<DefRef>,
    equatable: Option<DefRef>,
    copyable: Option<DefRef>,
    hashable: Option<DefRef>,
    /// Methods that take `mut self`.
    mut_methods: BTreeSet<DefRef>,
    /// Interface methods that take `mut self`.
    mut_iface: BTreeSet<(DefRef, String)>,
    /// Whether the functions being emitted are the declarations of an interface.
    in_iface_decl: bool,
    pub(crate) rt: BTreeSet<&'static str>,
    pub(crate) test_list: Vec<TestEntry>,
    // State of the module being emitted.
    module: usize,
    uses: BTreeMap<String, BTreeSet<String>>,
    f: FnState,
}

// ----- small helpers --------------------------------------------------------------------------

fn path(s: &str) -> Expr {
    Expr::Path(s.to_string())
}

fn lit(s: &str) -> Expr {
    Expr::Lit(s.to_string())
}

fn call(f: Expr, args: Vec<Expr>) -> Expr {
    Expr::Call(Box::new(f), args)
}

fn method(recv: Expr, name: &str, args: Vec<Expr>) -> Expr {
    Expr::Method(Box::new(recv), name.to_string(), Vec::new(), args)
}

fn reference(e: Expr) -> Expr {
    match e {
        Expr::Unary("*", inner) => *inner,
        other => Expr::Unary("&", Box::new(other)),
    }
}

fn deref(e: Expr) -> Expr {
    match e {
        Expr::Unary("&", inner) => *inner,
        other => Expr::Unary("*", Box::new(other)),
    }
}

fn bin(a: Expr, op: &'static str, b: Expr) -> Expr {
    Expr::Binary(Box::new(a), op, Box::new(b))
}

fn not(e: Expr) -> Expr {
    match e {
        Expr::Unary("!", inner) => *inner,
        Expr::Binary(a, "==", b) => Expr::Binary(a, "!=", b),
        other => Expr::Unary("!", Box::new(other)),
    }
}

fn block(stmts: Vec<Stmt>, tail: Option<Expr>) -> rs::Block {
    rs::Block {
        stmts,
        tail: tail.map(Box::new),
    }
}

fn to_string(e: Expr) -> Expr {
    method(e, "to_string", Vec::new())
}

/// Whether an emitted expression has a reference type.
fn is_ref(e: &Expr) -> bool {
    matches!(e, Expr::Unary("&", _) | Expr::Str(_))
        || matches!(e, Expr::Method(_, n, _, _) if n == "at")
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

fn mentions_param(t: &IrType) -> bool {
    match t {
        IrType::Param(_) => true,
        IrType::List(a) | IrType::Option(a) => mentions_param(a),
        IrType::Map(a, b) | IrType::Result(a, b) => mentions_param(a) || mentions_param(b),
        IrType::Named(_, args) => args.iter().any(mentions_param),
        IrType::Fn(ps, r) => ps.iter().any(|(_, t)| mentions_param(t)) || mentions_param(r),
        _ => false,
    }
}

/// `adds_two_numbers` for `adds two numbers`.
fn test_name(name: &str) -> String {
    let mut out = String::new();
    for word in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(&word.to_ascii_lowercase());
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "t_");
    }
    value(&out)
}

fn float_lit(v: f64) -> Expr {
    if v.is_nan() {
        path("f64::NAN")
    } else if v.is_infinite() {
        path(if v > 0.0 {
            "f64::INFINITY"
        } else {
            "f64::NEG_INFINITY"
        })
    } else {
        lit(&format!("{v:?}"))
    }
}

// ----- scans ----------------------------------------------------------------------------------

/// Locals a body reads, and locals it changes (they need `let mut`).
#[derive(Default)]
struct Scan<'a> {
    /// Methods that take `mut self`.
    mut_methods: Option<&'a BTreeSet<DefRef>>,
    mut_iface: Option<&'a BTreeSet<(DefRef, String)>>,
    reads: BTreeSet<LocalRef>,
    mutated: BTreeSet<LocalRef>,
    /// Locals that are used inside closures.
    captured: BTreeSet<LocalRef>,
    depth: usize,
}

/// The local a place starts at.
fn root(e: &IrExpr) -> Option<LocalRef> {
    match &e.kind {
        ExprKind::Local(l) => Some(*l),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => root(base),
        _ => None,
    }
}

impl Scan<'_> {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn change(&mut self, e: &IrExpr) {
        if let Some(l) = root(e) {
            self.mutated.insert(l);
        }
    }

    fn stmt(&mut self, s: &IrStmt) {
        match s {
            IrStmt::Let { value, .. } => self.expr(value),
            IrStmt::Assign { target, value, .. } => {
                self.change(target);
                if !matches!(target.kind, ExprKind::Local(_)) {
                    self.expr(target);
                }
                self.expr(value);
            }
            IrStmt::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            IrStmt::ForList {
                list, body, mode, ..
            } => {
                // `for x in own xs` consumes the list without changing it.
                if *mode == Mode::Mut {
                    self.change(list);
                }
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
                if self.depth > 0 {
                    self.captured.insert(*l);
                }
            }
            ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Method { recv, method } => {
                        // A method that takes `mut self` changes its receiver.
                        if self.mut_methods.is_some_and(|m| m.contains(method)) {
                            self.change(recv);
                        }
                        self.expr(recv);
                    }
                    Callee::Interface {
                        recv,
                        interface,
                        name,
                    } => {
                        let key = (*interface, name.clone());
                        if self.mut_iface.is_some_and(|m| m.contains(&key)) {
                            self.change(recv);
                        }
                        self.expr(recv)
                    }
                    Callee::Value(f) => self.expr(f),
                    Callee::Fn(_) => {}
                }
                for a in args {
                    if a.mode == Mode::Mut {
                        self.change(&a.value);
                    }
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
            ExprKind::Struct { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
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
            ExprKind::Closure { body, .. } => {
                self.depth += 1;
                self.block(body);
                self.depth -= 1;
            }
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
                scrutinee,
                arms,
                mode,
            } => {
                if *mode == Mode::Mut {
                    self.change(scrutinee);
                }
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

impl<'p> Emitter<'p> {
    pub(crate) fn new(p: &'p Program, tests: bool) -> Emitter<'p> {
        let mut e = Emitter {
            p,
            tests,
            crate_name: p.name.replace('-', "_"),
            fns: BTreeMap::new(),
            iface_fns: BTreeMap::new(),
            enums: BTreeMap::new(),
            structs: BTreeMap::new(),
            ifaces: BTreeMap::new(),
            names: BTreeMap::new(),
            std_names: BTreeMap::new(),
            reexported: BTreeMap::new(),
            mod_paths: Vec::new(),
            boxed: BTreeSet::new(),
            clone: BTreeSet::new(),
            eq: BTreeSet::new(),
            dyn_ifaces: BTreeSet::new(),
            error_iface: None,
            comparable: None,
            equatable: None,
            copyable: None,
            hashable: None,
            mut_methods: BTreeSet::new(),
            mut_iface: BTreeSet::new(),
            in_iface_decl: false,
            rt: BTreeSet::new(),
            test_list: Vec::new(),
            module: 0,
            uses: BTreeMap::new(),
            f: FnState::default(),
        };
        e.tables();
        e
    }

    // ----- tables -----------------------------------------------------------------------------

    fn tables(&mut self) {
        let p = self.p;
        let info = |f: &ir::Function, owner: Option<DefRef>| FnInfo {
            params: f.params.iter().map(|x| (x.mode, x.ty.clone())).collect(),
            ret: f.ret.clone(),
            owner,
            self_mode: f.self_mode,
            boxed_self: f.self_mode == Some(Mode::Own) && f.interface.is_some(),
        };
        for m in &p.modules {
            let path = match m.role {
                ModuleRole::Std => None,
                ModuleRole::Entry | ModuleRole::Example | ModuleRole::Test => Some(Vec::new()),
                ModuleRole::Src => Some(m.path.iter().map(|s| value(s)).collect()),
            };
            self.mod_paths.push(path);
            let std = m.role == ModuleRole::Std;
            for item in &m.items {
                match item {
                    ir::Item::Struct(s) => {
                        self.names.insert(s.def, type_name(&s.name));
                        self.structs.insert(s.def, s);
                        for f in &s.methods {
                            self.fns.insert(f.def, info(f, Some(s.def)));
                            self.names.insert(f.def, value(&f.name));
                        }
                    }
                    ir::Item::Enum(e) => {
                        self.names.insert(e.def, type_name(&e.name));
                        self.enums.insert(e.def, e);
                        for v in &e.variants {
                            self.names.insert(v.def, type_name(&v.name));
                        }
                        for f in &e.methods {
                            self.fns.insert(f.def, info(f, Some(e.def)));
                            self.names.insert(f.def, value(&f.name));
                        }
                    }
                    ir::Item::Interface(i) => {
                        self.names.insert(i.def, type_name(&i.name));
                        self.ifaces.insert(i.def, i);
                        for f in &i.methods {
                            let mut x = info(f, None);
                            x.boxed_self = f.self_mode == Some(Mode::Own);
                            self.iface_fns.insert((i.def, f.name.clone()), x);
                        }
                        if std {
                            match i.name.as_str() {
                                "Comparable" => self.comparable = Some(i.def),
                                "Equatable" => self.equatable = Some(i.def),
                                "Copyable" => self.copyable = Some(i.def),
                                "Hashable" => self.hashable = Some(i.def),
                                "Error" => self.error_iface = Some(i.def),
                                _ => {}
                            }
                        }
                    }
                    ir::Item::Fn(f) => {
                        self.names.insert(f.def, value(&f.name));
                        self.fns.insert(f.def, info(f, None));
                    }
                    ir::Item::Const(c) => {
                        self.names.insert(c.def, constant(&c.name));
                    }
                    ir::Item::ExternFn(x) => {
                        self.names.insert(x.def, value(&x.name));
                        self.fns.insert(
                            x.def,
                            FnInfo {
                                params: x.params.clone(),
                                ret: x.ret.clone(),
                                owner: None,
                                self_mode: None,
                                boxed_self: false,
                            },
                        );
                    }
                    ir::Item::ExternType(x) => {
                        self.names.insert(x.def, type_name(&x.name));
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
        // std lives in the runtime.
        for m in p.modules.iter().filter(|m| m.role == ModuleRole::Std) {
            for item in &m.items {
                match item {
                    ir::Item::Struct(s) => {
                        let piece = match s.name.as_str() {
                            "ParseError" => "ParseError",
                            "Entry" => "Entry",
                            _ => continue,
                        };
                        self.std_names
                            .insert(s.def, (format!("wolf_rt::{}", s.name), piece));
                    }
                    ir::Item::Enum(e) => {
                        let piece = match e.name.as_str() {
                            "Ordering" => "Ordering",
                            "FsError" => "FsError",
                            _ => continue,
                        };
                        self.std_names
                            .insert(e.def, (format!("wolf_rt::{}", e.name), piece));
                        for v in &e.variants {
                            self.std_names
                                .insert(v.def, (format!("wolf_rt::{}::{}", e.name, v.name), piece));
                        }
                    }
                    ir::Item::Interface(i) => {
                        let piece = match i.name.as_str() {
                            "Error" => "Error",
                            "Comparable" => "Comparable",
                            _ => continue,
                        };
                        self.std_names
                            .insert(i.def, (format!("wolf_rt::{}", i.name), piece));
                    }
                    ir::Item::Const(c) => {
                        let path = match c.name.as_str() {
                            "pi" => "std::f64::consts::PI",
                            "e" => "std::f64::consts::E",
                            "int_min" => "i64::MIN",
                            "int_max" => "i64::MAX",
                            _ => continue,
                        };
                        self.std_names.insert(c.def, (path.to_string(), ""));
                    }
                    _ => {}
                }
            }
        }
        for m in p.modules.iter().filter(|m| m.role == ModuleRole::Entry) {
            for r in &m.reexports {
                let kind = p.def(r.def).map(|d| d.kind);
                let n = match kind {
                    Some(DefKind::Struct | DefKind::Enum | DefKind::Interface) => {
                        type_name(&r.name)
                    }
                    Some(DefKind::Const) => constant(&r.name),
                    _ => value(&r.name),
                };
                self.reexported.insert(r.def, n);
            }
        }
        self.mut_methods = self
            .fns
            .iter()
            .filter(|(_, f)| f.self_mode == Some(Mode::Mut))
            .map(|(d, _)| *d)
            .collect();
        self.mut_iface = self
            .iface_fns
            .iter()
            .filter(|(_, f)| f.self_mode == Some(Mode::Mut))
            .map(|(k, _)| k.clone())
            .collect();
        self.recursion();
        self.derives();
        self.dyn_types();
    }

    /// The types a type contains directly (also through `Option` and `Result`), by field.
    fn contained(&self, t: &IrType, out: &mut Vec<DefRef>) {
        match t {
            IrType::Named(d, args) => {
                if self.structs.contains_key(d) || self.enums.contains_key(d) {
                    out.push(*d);
                }
                let _ = args;
            }
            IrType::Option(a) => self.contained(a, out),
            IrType::Result(a, b) => {
                self.contained(a, out);
                self.contained(b, out);
            }
            _ => {}
        }
    }

    fn field_types(&self, def: DefRef) -> Vec<IrType> {
        if let Some(s) = self.structs.get(&def) {
            return s.fields.iter().map(|f| f.ty.clone()).collect();
        }
        if let Some(e) = self.enums.get(&def) {
            let mut out = Vec::new();
            for v in &e.variants {
                match &v.payload {
                    Payload::None => {}
                    Payload::Tuple(ts) => out.extend(ts.iter().cloned()),
                    Payload::Struct(fs) => out.extend(fs.iter().map(|f| f.ty.clone())),
                }
            }
            return out;
        }
        Vec::new()
    }

    /// Fields that make a type recursive are boxed (spec chapter 8, section 5.1).
    fn recursion(&mut self) {
        let defs: Vec<DefRef> = self
            .structs
            .keys()
            .chain(self.enums.keys())
            .copied()
            .collect();
        // reach[a] = types reachable from a through direct containment.
        let mut reach: BTreeMap<DefRef, BTreeSet<DefRef>> = BTreeMap::new();
        for d in &defs {
            let mut seen = BTreeSet::new();
            let mut stack = vec![*d];
            while let Some(x) = stack.pop() {
                for t in self.field_types(x) {
                    let mut out = Vec::new();
                    self.contained(&t, &mut out);
                    for y in out {
                        if seen.insert(y) {
                            stack.push(y);
                        }
                    }
                }
            }
            reach.insert(*d, seen);
        }
        for d in &defs {
            for (i, t) in self.field_types(*d).iter().enumerate() {
                // `next: Option<Node>` is boxed as a whole: `Box<Option<Node>>`.
                let mut targets = Vec::new();
                self.contained(t, &mut targets);
                let cycle = targets
                    .iter()
                    .any(|target| target == d || reach.get(target).is_some_and(|r| r.contains(d)));
                if cycle {
                    self.boxed.insert((*d, i));
                }
            }
        }
    }

    /// Which types derive `Clone` and `PartialEq`: those whose fields all allow it.
    fn derives(&mut self) {
        let defs: Vec<DefRef> = self
            .structs
            .keys()
            .chain(self.enums.keys())
            .copied()
            .collect();
        let mut clone: BTreeSet<DefRef> = defs.iter().copied().collect();
        let mut eq: BTreeSet<DefRef> = clone.clone();
        loop {
            let mut changed = false;
            for d in &defs {
                let types = self.field_types(*d);
                if clone.contains(d) && !types.iter().all(|t| self.allows(t, &clone, true)) {
                    clone.remove(d);
                    changed = true;
                }
                if eq.contains(d) && !types.iter().all(|t| self.allows(t, &eq, false)) {
                    eq.remove(d);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        self.clone = clone;
        self.eq = eq;
    }

    fn allows(&self, t: &IrType, set: &BTreeSet<DefRef>, clone: bool) -> bool {
        match t {
            IrType::List(a) | IrType::Option(a) => self.allows(a, set, clone),
            IrType::Map(a, b) | IrType::Result(a, b) => {
                self.allows(a, set, clone) && self.allows(b, set, clone)
            }
            IrType::Named(d, args) => {
                let own = if self.ifaces.contains_key(d) {
                    clone && Some(*d) != self.error_iface
                } else if self.std_names.contains_key(d) {
                    true
                } else {
                    set.contains(d) || !(self.structs.contains_key(d) || self.enums.contains_key(d))
                };
                own && args.iter().all(|a| self.allows(a, set, clone))
            }
            IrType::Fn(..) => false,
            _ => true,
        }
    }

    /// Interfaces used as types of values get a hidden copy method and `Clone` for their box.
    fn dyn_types(&mut self) {
        let mut found = BTreeSet::new();
        let visit = |t: &IrType, found: &mut BTreeSet<DefRef>| {
            fn walk(t: &IrType, f: &mut dyn FnMut(DefRef)) {
                match t {
                    IrType::Named(d, args) => {
                        f(*d);
                        for a in args {
                            walk(a, f);
                        }
                    }
                    IrType::List(a) | IrType::Option(a) => walk(a, f),
                    IrType::Map(a, b) | IrType::Result(a, b) => {
                        walk(a, f);
                        walk(b, f);
                    }
                    IrType::Fn(ps, r) => {
                        for (_, p) in ps {
                            walk(p, f);
                        }
                        walk(r, f);
                    }
                    _ => {}
                }
            }
            walk(t, &mut |d| {
                found.insert(d);
            });
        };
        for l in self.p.locals.values() {
            visit(&l.ty, &mut found);
        }
        for s in self.structs.values() {
            for f in &s.fields {
                visit(&f.ty, &mut found);
            }
        }
        for d in self.enums.keys() {
            for t in self.field_types(*d) {
                visit(&t, &mut found);
            }
        }
        for f in self.fns.values() {
            for (_, t) in &f.params {
                visit(t, &mut found);
            }
            visit(&f.ret, &mut found);
        }
        self.dyn_ifaces = found
            .into_iter()
            .filter(|d| self.ifaces.contains_key(d) && !self.std_names.contains_key(d))
            .collect();
    }

    // ----- names and paths --------------------------------------------------------------------

    fn def_module(&self, def: DefRef) -> Option<usize> {
        self.p.def(def).map(|d| d.module)
    }

    fn name_of(&self, def: DefRef) -> String {
        self.names
            .get(&def)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn role(&self) -> ModuleRole {
        self.p
            .modules
            .get(self.module)
            .map(|m| m.role)
            .unwrap_or(ModuleRole::Src)
    }

    /// Whether the current file is a crate of its own (an example or an integration test of a
    /// library).
    fn outside(&self) -> bool {
        matches!(self.role(), ModuleRole::Example | ModuleRole::Test)
    }

    /// The path of the runtime from the current file.
    fn rt(&mut self, piece: &'static str) -> String {
        if !piece.is_empty() {
            self.rt.insert(piece);
        }
        let base = if self.outside() {
            format!("{}::wolf_rt", self.crate_name)
        } else {
            "crate::wolf_rt".to_string()
        };
        self.uses.entry(base).or_default();
        "wolf_rt".to_string()
    }

    fn rt_path(&mut self, piece: &'static str, name: &str) -> Expr {
        let r = self.rt(piece);
        path(&format!("{r}::{name}"))
    }

    fn rt_call(&mut self, piece: &'static str, name: &str, args: Vec<Expr>) -> Expr {
        let f = self.rt_path(piece, name);
        call(f, args)
    }

    /// Brings a trait of the runtime into scope, for its methods.
    fn rt_trait(&mut self, piece: &'static str, name: &str) {
        self.rt.insert(piece);
        let base = if self.outside() {
            format!("{}::wolf_rt", self.crate_name)
        } else {
            "crate::wolf_rt".to_string()
        };
        self.uses.entry(base).or_default().insert(name.to_string());
    }

    /// A reference to a top-level declaration from the current file, importing it.
    fn def_path(&mut self, def: DefRef) -> String {
        if let Some((p, piece)) = self.std_names.get(&def).cloned() {
            if !piece.is_empty() {
                self.rt(piece);
            }
            return p;
        }
        let name = self.name_of(def);
        let Some(module) = self.def_module(def) else {
            return name;
        };
        if self.outside() {
            if let Some(n) = self.reexported.get(&def).cloned() {
                self.uses
                    .entry(self.crate_name.clone())
                    .or_default()
                    .insert(n.clone());
                return n;
            }
        }
        if module == self.module {
            return name;
        }
        let Some(Some(mp)) = self.mod_paths.get(module).cloned() else {
            return name;
        };
        let base = if mp.is_empty() {
            "crate".to_string()
        } else {
            format!("crate::{}", mp.join("::"))
        };
        self.uses.entry(base).or_default().insert(name.clone());
        name
    }

    /// The path of a variant: `Shape::Circle`.
    fn variant_path(&mut self, variant: DefRef, parent: DefRef) -> String {
        if let Some((p, piece)) = self.std_names.get(&variant).cloned() {
            self.rt(piece);
            return p;
        }
        let e = self.def_path(parent);
        format!("{e}::{}", self.name_of(variant))
    }

    fn local_name(&self, l: LocalRef) -> String {
        self.p
            .locals
            .get(&l)
            .map(|i| value(&i.name))
            .unwrap_or_else(|| "x".to_string())
    }

    fn local_ty(&self, l: LocalRef) -> IrType {
        self.p
            .locals
            .get(&l)
            .map(|i| i.ty.clone())
            .unwrap_or(IrType::Void)
    }

    // ----- types ------------------------------------------------------------------------------

    fn plain_enum(&self, t: &IrType) -> bool {
        matches!(t, IrType::Named(d, _) if self.enums.get(d).is_some_and(|e| e.variants.iter().all(|v| v.payload.is_empty())))
    }

    /// Types that are `Copy` in Rust.
    fn copy(&self, t: &IrType) -> bool {
        match t {
            IrType::Int | IrType::Float | IrType::Bool | IrType::Void | IrType::Never => true,
            IrType::Option(a) => self.copy(a),
            _ => self.plain_enum(t),
        }
    }

    fn ty(&mut self, t: &IrType) -> Result<Type, Error> {
        Ok(match t {
            IrType::Int => Type::name("i64"),
            IrType::Float => Type::name("f64"),
            IrType::Bool => Type::name("bool"),
            IrType::String => Type::name("String"),
            IrType::Void => Type::Unit,
            IrType::Never => Type::Never,
            IrType::List(a) => Type::Path("Vec".to_string(), vec![self.ty(a)?]),
            IrType::Map(k, v) => {
                let r = self.rt("Map");
                Type::Path(format!("{r}::Map"), vec![self.ty(k)?, self.ty(v)?])
            }
            IrType::Option(a) => Type::Path("Option".to_string(), vec![self.ty(a)?]),
            IrType::Result(a, b) => {
                Type::Path("Result".to_string(), vec![self.ty(a)?, self.ty(b)?])
            }
            IrType::Named(d, args) => {
                let mut list = Vec::new();
                for a in args {
                    list.push(self.ty(a)?);
                }
                let p = self.def_path(*d);
                if self.ifaces.contains_key(d) {
                    Type::Path("Box".to_string(), vec![Type::Dyn(p, list)])
                } else {
                    Type::Path(p, list)
                }
            }
            IrType::Fn(params, ret) => {
                let dyn_fn = self.dyn_fn(params, ret)?;
                Type::Path("Box".to_string(), vec![dyn_fn])
            }
            IrType::Param(n) => Type::name(n),
            IrType::SelfType => Type::name("Self"),
        })
    }

    /// `dyn Fn(&A, &mut B) -> R`: read parameters are always references.
    fn dyn_fn(&mut self, params: &[(Mode, IrType)], ret: &IrType) -> Result<Type, Error> {
        let mut ps = Vec::new();
        for (m, t) in params {
            let inner = self.ty(t)?;
            ps.push(match m {
                Mode::Read => Type::Ref(Box::new(inner)),
                Mode::Mut => Type::RefMut(Box::new(inner)),
                Mode::Own => inner,
            });
        }
        let r = match ret {
            IrType::Void => Type::Unit,
            other => self.ty(other)?,
        };
        Ok(Type::DynFn(ps, Box::new(r)))
    }

    /// The type of a parameter of a declared function.
    fn param_ty(&mut self, mode: Mode, t: &IrType) -> Result<Type, Error> {
        Ok(match mode {
            Mode::Own => self.ty(t)?,
            Mode::Mut => Type::RefMut(Box::new(self.ty(t)?)),
            Mode::Read => {
                if self.copy(t) {
                    self.ty(t)?
                } else {
                    match t {
                        IrType::String => Type::Ref(Box::new(Type::name("str"))),
                        IrType::List(a) => Type::Ref(Box::new(Type::Slice(Box::new(self.ty(a)?)))),
                        IrType::Fn(ps, r) => Type::Ref(Box::new(self.dyn_fn(ps, r)?)),
                        IrType::Named(d, args) if self.ifaces.contains_key(d) => {
                            let mut list = Vec::new();
                            for a in args {
                                list.push(self.ty(a)?);
                            }
                            let p = self.def_path(*d);
                            Type::Ref(Box::new(Type::Dyn(p, list)))
                        }
                        _ => Type::Ref(Box::new(self.ty(t)?)),
                    }
                }
            }
        })
    }

    fn ret_ty(&mut self, t: &IrType) -> Result<Option<Type>, Error> {
        Ok(match t {
            IrType::Void => None,
            other => Some(self.ty(other)?),
        })
    }

    /// Type parameters with their Rust bounds.
    fn generics(&mut self, generics: &[ir::Generic]) -> Result<Vec<(String, Vec<String>)>, Error> {
        let mut out = Vec::new();
        for g in generics {
            let mut bounds = Vec::new();
            for b in &g.bounds {
                let IrType::Named(def, args) = b else {
                    continue;
                };
                if Some(*def) == self.equatable {
                    bounds.push("PartialEq".to_string());
                } else if Some(*def) == self.copyable {
                    bounds.push("Clone".to_string());
                } else if Some(*def) == self.hashable {
                    bounds.push("std::hash::Hash".to_string());
                    bounds.push("Eq".to_string());
                    bounds.push("Clone".to_string());
                } else if Some(*def) == self.comparable {
                    let r = self.rt("Comparable");
                    bounds.push(format!("{r}::Comparable"));
                } else {
                    let mut list = Vec::new();
                    for a in args {
                        list.push(self.ty(a)?);
                    }
                    let p = self.def_path(*def);
                    let t = rs::Type::Path(p, list);
                    bounds.push(type_text(&t));
                }
            }
            bounds.dedup();
            out.push((g.name.clone(), bounds));
        }
        Ok(out)
    }

    // ----- modules ----------------------------------------------------------------------------

    /// The files of the project, by path.
    pub(crate) fn files(&mut self) -> Result<Vec<(String, String)>, Error> {
        let mut out = Vec::new();
        // Module files, with the declarations of their child modules.
        let mut children: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
        for (i, m) in self.p.modules.iter().enumerate() {
            if m.role != ModuleRole::Src {
                continue;
            }
            let Some(Some(mp)) = self.mod_paths.get(i).cloned() else {
                continue;
            };
            for k in 0..mp.len() {
                let parent = mp.get(..k).map(<[String]>::to_vec).unwrap_or_default();
                if let Some(c) = mp.get(k) {
                    children.entry(parent).or_default().insert(c.clone());
                }
            }
        }
        let mut written: BTreeSet<Vec<String>> = BTreeSet::new();
        for (i, m) in self.p.modules.iter().enumerate() {
            let wanted = match m.role {
                ModuleRole::Entry | ModuleRole::Src => true,
                ModuleRole::Example => !self.tests,
                ModuleRole::Test => self.tests,
                ModuleRole::Std => false,
            };
            if !wanted {
                continue;
            }
            let Some(Some(mp)) = self.mod_paths.get(i).cloned() else {
                continue;
            };
            let file = match m.role {
                ModuleRole::Entry => match self.p.kind {
                    ProgramKind::Cli => "src/main.rs".to_string(),
                    ProgramKind::Lib => "src/lib.rs".to_string(),
                },
                ModuleRole::Example => format!("{}.rs", m.path.join("/")),
                ModuleRole::Test => format!("{}.rs", m.path.join("/")),
                _ => format!("src/{}.rs", mp.join("/")),
            };
            let kids = if matches!(m.role, ModuleRole::Entry | ModuleRole::Src) {
                children.get(&mp).cloned().unwrap_or_default()
            } else {
                BTreeSet::new()
            };
            if m.role == ModuleRole::Src {
                written.insert(mp.clone());
            }
            let text = self.module(i, &kids)?;
            out.push((file, text));
        }
        // Directories without a module of their own still need a file declaring the children.
        for (parent, kids) in &children {
            if parent.is_empty() || written.contains(parent) {
                continue;
            }
            let mut text = String::from("// Generated by wolf. Do not edit.\n\n");
            for k in kids {
                text.push_str(&format!("pub mod {k};\n"));
            }
            out.push((format!("src/{}.rs", parent.join("/")), text));
        }
        Ok(out)
    }

    fn module(&mut self, index: usize, kids: &BTreeSet<String>) -> Result<String, Error> {
        let p = self.p;
        let m = p.modules.get(index).ok_or("module")?;
        self.module = index;
        self.uses.clear();
        let mut items = Vec::new();
        let mut tests = Vec::new();
        for item in &m.items {
            match item {
                ir::Item::Fn(f) if f.in_tests => {
                    if self.tests {
                        tests.push(self.function_item(f)?);
                    }
                }
                ir::Item::Test(t) => {
                    if self.tests {
                        tests.push(self.test_fn(m, t)?);
                    }
                }
                _ => self.item(item, &mut items)?,
            }
        }
        let entry = m.role == ModuleRole::Entry;
        let runnable = matches!(m.role, ModuleRole::Entry | ModuleRole::Example)
            && (m.role == ModuleRole::Example || p.kind == ProgramKind::Cli)
            && m.items
                .iter()
                .any(|it| matches!(it, ir::Item::Fn(f) if f.name == "main"));
        if runnable {
            let result = m.items.iter().any(|it| {
                matches!(it, ir::Item::Fn(f) if f.name == "main" && matches!(f.ret, IrType::Result(..)))
            });
            let run = if result {
                self.rt_call("run_main_result", "run_main_result", vec![path("main_")])
            } else {
                self.rt_call("run_main", "run_main", vec![path("main_")])
            };
            items.push(Item::Fn(rs::Fn {
                docs: Vec::new(),
                attrs: Vec::new(),
                vis: String::new(),
                name: "main".to_string(),
                generics: Vec::new(),
                self_param: None,
                params: Vec::new(),
                ret: None,
                body: Some(block(vec![Stmt::Expr(run)], None)),
            }));
        }
        // Re-exports of a library.
        let mut reexports = Vec::new();
        if entry && p.kind == ProgramKind::Lib {
            let mut by_module: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for r in &m.reexports {
                let Some(module) = self.def_module(r.def) else {
                    continue;
                };
                let Some(Some(mp)) = self.mod_paths.get(module).cloned() else {
                    continue;
                };
                let name = self.name_of(r.def);
                let alias = self.reexported.get(&r.def).cloned().unwrap_or(name.clone());
                let entry = if alias == name {
                    name
                } else {
                    format!("{name} as {alias}")
                };
                by_module
                    .entry(format!("crate::{}", mp.join("::")))
                    .or_default()
                    .push(entry);
            }
            for (module, mut names) in by_module {
                names.sort();
                reexports.push(if names.len() == 1 {
                    format!("pub use {module}::{};", names.join(""))
                } else {
                    format!("pub use {module}::{{{}}};", names.join(", "))
                });
            }
        }
        if m.role == ModuleRole::Test {
            // An integration test of Cargo is a crate of tests of its own.
            items.extend(tests);
        } else if !tests.is_empty() {
            items.push(Item::Tests(tests));
        }
        // Head: module declarations, then imports.
        let mut head = Vec::new();
        if entry {
            for k in kids {
                head.push(Item::Mod(String::new(), k.clone()));
            }
            if p.kind == ProgramKind::Lib {
                head.push(Item::Raw("#[doc(hidden)]\npub mod wolf_rt;".to_string()));
            } else {
                head.push(Item::Mod(String::new(), "wolf_rt".to_string()));
            }
        } else {
            for k in kids {
                head.push(Item::Mod("pub ".to_string(), k.clone()));
            }
        }
        let mut uses: Vec<String> = Vec::new();
        for (base, names) in std::mem::take(&mut self.uses) {
            if base.ends_with("wolf_rt") {
                if !(entry && base == "crate::wolf_rt") {
                    uses.push(base.clone());
                }
                for n in names {
                    uses.push(format!("{base}::{n}"));
                }
                continue;
            }
            let names: Vec<String> = names.into_iter().collect();
            match names.as_slice() {
                [] => {}
                [one] => uses.push(format!("{base}::{one}")),
                many => uses.push(format!("{base}::{{{}}}", many.join(", "))),
            }
        }
        uses.sort_by_key(|u| use_key(u));
        for u in uses {
            head.push(Item::Use(u));
        }
        for r in reexports {
            head.push(Item::Raw(r));
        }
        head.extend(items);
        let docs = m
            .docs
            .iter()
            .flat_map(|d| d.lines().map(str::to_string).collect::<Vec<_>>())
            .collect();
        // Overflow and division by zero are panics at run time, also when Rust can see them in
        // constants (chapter 7, section 3).
        let attrs = if matches!(
            m.role,
            ModuleRole::Entry | ModuleRole::Example | ModuleRole::Test
        ) {
            vec![
                "#![allow(arithmetic_overflow, unconditional_panic, clippy::type_complexity)]"
                    .to_string(),
            ]
        } else {
            Vec::new()
        };
        Ok(rs::print(&rs::File {
            header: format!("Generated by wolf from {}. Do not edit.", m.file),
            docs,
            attrs,
            items: head,
        }))
    }

    fn item(&mut self, item: &'p ir::Item, out: &mut Vec<Item>) -> Result<(), Error> {
        match item {
            ir::Item::Struct(s) => self.struct_item(s, out),
            ir::Item::Enum(e) => self.enum_item(e, out),
            ir::Item::Interface(i) => self.interface_item(i, out),
            ir::Item::Fn(f) => {
                if f.body.is_some() {
                    out.push(self.function_item(f)?);
                }
                Ok(())
            }
            ir::Item::Const(c) => {
                let (ty, value) = match &c.value {
                    ir::ConstValue::Int(v) => (Type::name("i64"), lit(&v.to_string())),
                    ir::ConstValue::Float(v) => (Type::name("f64"), float_lit(*v)),
                    ir::ConstValue::Bool(v) => (Type::name("bool"), lit(&v.to_string())),
                    ir::ConstValue::Str(s) => {
                        (Type::Ref(Box::new(Type::name("str"))), Expr::Str(s.clone()))
                    }
                };
                out.push(Item::Const {
                    docs: c.docs.clone(),
                    vis: if c.public {
                        "pub ".to_string()
                    } else {
                        String::new()
                    },
                    name: self.name_of(c.def),
                    ty,
                    value,
                });
                Ok(())
            }
            ir::Item::ExternFn(x) => self.extern_fn(x, out),
            _ => Ok(()),
        }
    }

    fn vis(&self, public: bool) -> String {
        if public || self.role() == ModuleRole::Entry {
            "pub ".to_string()
        } else {
            String::new()
        }
    }

    fn function_item(&mut self, f: &'p ir::Function) -> Result<Item, Error> {
        let name = if f.name == "main" && f.self_mode.is_none() {
            "main_".to_string()
        } else {
            self.name_of(f.def)
        };
        let mut func = self.function(f, name, None)?;
        func.vis = if f.name == "main" {
            String::new()
        } else {
            self.vis(f.public)
        };
        Ok(Item::Fn(func))
    }

    /// A function or method.
    fn function(
        &mut self,
        f: &'p ir::Function,
        name: String,
        self_ty: Option<&IrType>,
    ) -> Result<rs::Fn, Error> {
        self.f = FnState::default();
        let mut scan = Scan {
            mut_methods: Some(&self.mut_methods),
            mut_iface: Some(&self.mut_iface),
            ..Scan::default()
        };
        if let Some(b) = &f.body {
            scan.block(b);
        }
        self.f.reads = scan.reads;
        self.f.mutated = scan.mutated;
        let generics = self.generics(&f.generics)?;
        let self_param = f.self_mode.map(|m| {
            match m {
                Mode::Read => {
                    self.f.self_local = Some(Local::Ref("self".to_string()));
                    "&self"
                }
                Mode::Mut => {
                    self.f.self_local = Some(Local::Mut("self".to_string()));
                    "&mut self"
                }
                Mode::Own if f.interface.is_some() || self.in_iface_decl => {
                    self.f.self_local = Some(Local::Boxed("self".to_string()));
                    "self: Box<Self>"
                }
                Mode::Own => {
                    self.f.self_local = Some(Local::Val("self".to_string()));
                    "self"
                }
            }
            .to_string()
        });
        let _ = self_ty;
        let mut params = Vec::new();
        for p in &f.params {
            let n = self.local_name(p.local);
            let t = self.param_ty(p.mode, &p.ty)?;
            let l = match p.mode {
                Mode::Own => Local::Val(n.clone()),
                Mode::Mut => Local::Mut(n.clone()),
                Mode::Read if self.copy(&p.ty) => Local::Val(n.clone()),
                Mode::Read => Local::Ref(n.clone()),
            };
            let mutable = p.mode == Mode::Own && self.f.mutated.contains(&p.local);
            if p.mode == Mode::Read && matches!(p.ty, IrType::String | IrType::List(_)) {
                self.f.unsized_params.insert(p.local);
            }
            self.f.locals.insert(p.local, l);
            // A parameter the body never uses is `_name`, as Rust expects.
            let unused = f.body.is_some()
                && !self.f.reads.contains(&p.local)
                && !self.f.mutated.contains(&p.local);
            let n = if unused { format!("_{n}") } else { n };
            params.push((Pat::Bind(n, mutable), t));
        }
        let ret = self.ret_ty(&f.ret)?;
        let body = match &f.body {
            Some(b) => Some(self.block(b, Want::Own)?),
            None => None,
        };
        self.f = FnState::default();
        Ok(rs::Fn {
            docs: f.docs.clone(),
            attrs: Vec::new(),
            vis: String::new(),
            name,
            generics,
            self_param,
            params,
            ret,
            body,
        })
    }

    /// A test function.
    fn test_fn(&mut self, m: &'p ir::Module, t: &'p ir::Test) -> Result<Item, Error> {
        let name = test_name(&t.name);
        let target = match m.role {
            ModuleRole::Test => m.path.iter().skip(1).cloned().collect::<Vec<_>>().join("_"),
            _ => match self.p.kind {
                ProgramKind::Cli => "bin".to_string(),
                ProgramKind::Lib => "lib".to_string(),
            },
        };
        let mp = self
            .mod_paths
            .get(self.module)
            .cloned()
            .flatten()
            .unwrap_or_default();
        let path_in_crate = if m.role == ModuleRole::Test {
            name.clone()
        } else {
            let mut parts = mp.clone();
            parts.push("tests".to_string());
            parts.push(name.clone());
            parts.join("::")
        };
        self.test_list.push(TestEntry {
            target,
            path: path_in_crate,
            name: format!("{}: {}", m.file, t.name),
        });
        self.f = FnState::default();
        let mut scan = Scan {
            mut_methods: Some(&self.mut_methods),
            mut_iface: Some(&self.mut_iface),
            ..Scan::default()
        };
        scan.block(&t.body);
        self.f.reads = scan.reads;
        self.f.mutated = scan.mutated;
        let body = self.block(&t.body, Want::Own)?;
        self.f = FnState::default();
        let run = self.rt_call(
            "test",
            "test",
            vec![Expr::Closure(
                Vec::new(),
                Box::new(Expr::Block(body)),
                false,
            )],
        );
        let item = rs::Fn {
            docs: Vec::new(),
            attrs: vec!["#[test]".to_string()],
            vis: String::new(),
            name,
            generics: Vec::new(),
            self_param: None,
            params: Vec::new(),
            ret: None,
            body: Some(block(vec![Stmt::Expr(run)], None)),
        };
        Ok(Item::Fn(item))
    }

    fn derive_list(&self, def: DefRef, plain: bool) -> Vec<String> {
        if plain {
            return ["Clone", "Copy", "PartialEq", "Eq", "Hash"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        }
        let mut d = Vec::new();
        if self.clone.contains(&def) {
            d.push("Clone".to_string());
        }
        if self.eq.contains(&def) {
            d.push("PartialEq".to_string());
        }
        d
    }

    fn struct_item(&mut self, s: &'p ir::Struct, out: &mut Vec<Item>) -> Result<(), Error> {
        if self.std_names.contains_key(&s.def) {
            return Ok(());
        }
        let generics: Vec<(String, Vec<String>)> = s
            .generics
            .iter()
            .map(|g| (g.name.clone(), Vec::new()))
            .collect();
        let mut fields = Vec::new();
        for (i, f) in s.fields.iter().enumerate() {
            let mut t = self.ty(&f.ty)?;
            if self.boxed.contains(&(s.def, i)) {
                t = Type::Path("Box".to_string(), vec![t]);
            }
            let vis = if f.public {
                "pub ".to_string()
            } else {
                String::new()
            };
            fields.push((f.docs.clone(), vis, value(&f.name), t));
        }
        out.push(Item::Struct {
            docs: s.docs.clone(),
            derives: self.derive_list(s.def, false),
            vis: self.vis(s.public),
            name: self.name_of(s.def),
            generics,
            fields,
        });
        let args: Vec<IrType> = s
            .generics
            .iter()
            .map(|g| IrType::Param(g.name.clone()))
            .collect();
        let self_ty = IrType::Named(s.def, args);
        self.impls(s.def, &s.generics, &s.methods, &self_ty, out)
    }

    /// The `impl` blocks of a type: inherent functions grouped by the bounds of their block, and
    /// one block per implemented interface.
    fn impls(
        &mut self,
        def: DefRef,
        type_generics: &[ir::Generic],
        methods: &'p [ir::Function],
        self_ty: &IrType,
        out: &mut Vec<Item>,
    ) -> Result<(), Error> {
        let ty = {
            let mut list = Vec::new();
            for g in type_generics {
                list.push(Type::name(&g.name));
            }
            Type::Path(self.name_of(def), list)
        };
        let mut groups: Vec<ImplGroup<'p>> = Vec::new();
        for f in methods.iter().filter(|f| f.body.is_some()) {
            let bounds = if f.impl_bounds.is_empty() {
                type_generics.iter().map(|g| g.bounds.clone()).collect()
            } else {
                f.impl_bounds.clone()
            };
            match groups
                .iter_mut()
                .find(|(i, b, _)| *i == f.interface && *b == bounds)
            {
                Some(g) => g.2.push(f),
                None => groups.push((f.interface, bounds, vec![f])),
            }
        }
        for (iface, bounds, fns) in groups {
            if iface
                .is_some_and(|i| [self.equatable, self.copyable, self.hashable].contains(&Some(i)))
            {
                continue;
            }
            let gens: Vec<ir::Generic> = type_generics
                .iter()
                .zip(bounds.iter().chain(std::iter::repeat(&Vec::new())))
                .map(|(g, b)| ir::Generic {
                    name: g.name.clone(),
                    bounds: b.clone(),
                })
                .collect();
            let generics = self.generics(&gens)?;
            let mut items = Vec::new();
            for f in fns {
                let mut func = self.function(f, self.name_of(f.def), Some(self_ty))?;
                if iface.is_none() {
                    func.vis = self.vis(f.public);
                }
                if iface.is_some() {
                    func.name = value(&f.name);
                }
                items.push(func);
            }
            let trait_ = match iface {
                Some(i) => {
                    let p = self.def_path(i);
                    if self.dyn_ifaces.contains(&i) && self.clone.contains(&def) {
                        let r =
                            Type::Path("Box".to_string(), vec![Type::Dyn(p.clone(), Vec::new())]);
                        items.push(rs::Fn {
                            docs: Vec::new(),
                            attrs: Vec::new(),
                            vis: String::new(),
                            name: "copy_dyn".to_string(),
                            generics: Vec::new(),
                            self_param: Some("&self".to_string()),
                            params: Vec::new(),
                            ret: Some(r),
                            body: Some(block(
                                Vec::new(),
                                Some(call(
                                    path("Box::new"),
                                    vec![if self.copy(self_ty) {
                                        deref(path("self"))
                                    } else {
                                        method(path("self"), "clone", Vec::new())
                                    }],
                                )),
                            )),
                        });
                    }
                    Some(Type::name(&p))
                }
                None => None,
            };
            // A `new()` without parameters goes with `Default` (Clippy's `new_without_default`).
            let new = trait_.is_none()
                && items.iter().any(|f| {
                    f.name == "new"
                        && f.self_param.is_none()
                        && f.params.is_empty()
                        && f.generics.is_empty()
                });
            out.push(Item::Impl {
                generics: generics.clone(),
                trait_,
                ty: ty.clone(),
                fns: items,
            });
            if new {
                out.push(Item::Impl {
                    generics,
                    trait_: Some(Type::name("Default")),
                    ty: ty.clone(),
                    fns: vec![rs::Fn {
                        docs: Vec::new(),
                        attrs: Vec::new(),
                        vis: String::new(),
                        name: "default".to_string(),
                        generics: Vec::new(),
                        self_param: None,
                        params: Vec::new(),
                        ret: Some(Type::name("Self")),
                        body: Some(block(Vec::new(), Some(call(path("Self::new"), Vec::new())))),
                    }],
                });
            }
        }
        Ok(())
    }

    fn enum_item(&mut self, e: &'p ir::Enum, out: &mut Vec<Item>) -> Result<(), Error> {
        if self.std_names.contains_key(&e.def) {
            return Ok(());
        }
        let plain = e.variants.iter().all(|v| v.payload.is_empty());
        let generics: Vec<(String, Vec<String>)> = e
            .generics
            .iter()
            .map(|g| (g.name.clone(), Vec::new()))
            .collect();
        let mut variants = Vec::new();
        let mut index = 0;
        for v in &e.variants {
            let mut boxed = |t: Type, this: &Self| {
                let b = this.boxed.contains(&(e.def, index));
                index += 1;
                if b {
                    Type::Path("Box".to_string(), vec![t])
                } else {
                    t
                }
            };
            let kind = match &v.payload {
                Payload::None => rs::Variant::Unit,
                Payload::Tuple(ts) => {
                    let mut list = Vec::new();
                    for t in ts {
                        let t = self.ty(t)?;
                        list.push(boxed(t, self));
                    }
                    rs::Variant::Tuple(list)
                }
                Payload::Struct(fs) => {
                    let mut list = Vec::new();
                    for f in fs {
                        let t = self.ty(&f.ty)?;
                        list.push((value(&f.name), boxed(t, self)));
                    }
                    rs::Variant::Struct(list)
                }
            };
            variants.push((v.docs.clone(), self.name_of(v.def), kind));
        }
        out.push(Item::Enum {
            docs: e.docs.clone(),
            derives: self.derive_list(e.def, plain),
            vis: self.vis(e.public),
            name: self.name_of(e.def),
            generics,
            variants,
        });
        let args: Vec<IrType> = e
            .generics
            .iter()
            .map(|g| IrType::Param(g.name.clone()))
            .collect();
        let self_ty = IrType::Named(e.def, args);
        self.impls(e.def, &e.generics, &e.methods, &self_ty, out)
    }

    fn interface_item(&mut self, i: &'p ir::Interface, out: &mut Vec<Item>) -> Result<(), Error> {
        if self.std_names.contains_key(&i.def)
            || [self.equatable, self.copyable, self.hashable].contains(&Some(i.def))
            || self
                .def_module(i.def)
                .and_then(|m| self.p.modules.get(m))
                .is_some_and(|m| m.role == ModuleRole::Std)
        {
            return Ok(());
        }
        let generics = self.generics(&i.generics)?;
        let mut methods = Vec::new();
        self.in_iface_decl = true;
        for f in &i.methods {
            let func = self.function(f, value(&f.name), None);
            let mut func = match func {
                Ok(func) => func,
                Err(e) => {
                    self.in_iface_decl = false;
                    return Err(e);
                }
            };
            func.body = None;
            methods.push(func);
        }
        self.in_iface_decl = false;
        let name = self.name_of(i.def);
        let copyable = self.dyn_ifaces.contains(&i.def) && self.implementors_clone(i.def);
        if copyable {
            methods.push(rs::Fn {
                docs: vec!["A copy of the value behind a `Box<dyn ...>`.".to_string()],
                attrs: Vec::new(),
                vis: String::new(),
                name: "copy_dyn".to_string(),
                generics: Vec::new(),
                self_param: Some("&self".to_string()),
                params: Vec::new(),
                ret: Some(Type::Path(
                    "Box".to_string(),
                    vec![Type::Dyn(name.clone(), Vec::new())],
                )),
                body: None,
            });
        }
        out.push(Item::Trait {
            docs: i.docs.clone(),
            vis: self.vis(i.public),
            name: name.clone(),
            generics,
            methods,
        });
        if copyable {
            out.push(Item::Raw(format!(
                "impl Clone for Box<dyn {name}> {{\n    fn clone(&self) -> Self {{\n        self.copy_dyn()\n    }}\n}}"
            )));
        }
        Ok(())
    }

    fn implementors_clone(&self, iface: DefRef) -> bool {
        let implements = |list: &[IrType]| {
            list.iter()
                .any(|t| matches!(t, IrType::Named(d, _) if *d == iface))
        };
        self.structs
            .iter()
            .filter(|(_, s)| implements(&s.implements))
            .all(|(d, _)| self.clone.contains(d))
            && self
                .enums
                .iter()
                .filter(|(_, e)| implements(&e.implements))
                .all(|(d, _)| self.clone.contains(d))
    }

    fn extern_fn(&mut self, x: &'p ir::ExternFn, out: &mut Vec<Item>) -> Result<(), Error> {
        let Some(target) = x.paths.get("rust") else {
            return Ok(());
        };
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (i, (m, t)) in x.params.iter().enumerate() {
            let n = format!("p{i}");
            params.push((Pat::Bind(n.clone(), false), self.param_ty(*m, t)?));
            args.push(path(&n));
        }
        let ret = self.ret_ty(&x.ret)?;
        let body = call(path(target), args);
        out.push(Item::Fn(rs::Fn {
            docs: Vec::new(),
            attrs: Vec::new(),
            vis: self.vis(x.public),
            name: self.name_of(x.def),
            generics: Vec::new(),
            self_param: None,
            params,
            ret,
            body: Some(block(Vec::new(), Some(body))),
        }));
        Ok(())
    }

    // ----- statements -------------------------------------------------------------------------

    fn block(&mut self, b: &'p Block, want: Want) -> Result<rs::Block, Error> {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            self.stmt(s, &mut stmts)?;
        }
        let tail = match &b.tail {
            Some(t) => Some(self.expr(t, want)?),
            None => None,
        };
        Ok(rs::Block {
            stmts,
            tail: tail.map(Box::new),
        })
    }

    fn stmt(&mut self, s: &'p IrStmt, out: &mut Vec<Stmt>) -> Result<(), Error> {
        match s {
            IrStmt::Let { local, value, .. } => {
                let ty = self.local_ty(*local);
                let name = self.local_name(*local);
                let read = self.f.reads.contains(local);
                let mutable = self.f.mutated.contains(local);
                if !read && !mutable {
                    let v = self.expr(value, Want::Own)?;
                    out.push(Stmt::Let(Pat::Wild, None, Some(v)));
                    return Ok(());
                }
                let v = self.expr(value, Want::Own)?;
                let annotate = match &value.kind {
                    ExprKind::Int(_)
                    | ExprKind::Float(_)
                    | ExprKind::List(_)
                    | ExprKind::Map(_)
                    | ExprKind::Some(_)
                    | ExprKind::None
                    | ExprKind::Ok(_)
                    | ExprKind::Err(_)
                    | ExprKind::If { .. }
                    | ExprKind::Match { .. }
                    | ExprKind::Block(_)
                    | ExprKind::Unary { .. }
                    | ExprKind::Binary { .. }
                    | ExprKind::Coerce { .. }
                    | ExprKind::Closure { .. }
                    | ExprKind::FnValue(_) => true,
                    ExprKind::Call { .. } => mentions_param_result(value),
                    _ => false,
                };
                let t = if annotate && !matches!(ty, IrType::Void | IrType::Never) {
                    Some(self.ty(&ty)?)
                } else {
                    None
                };
                self.f.locals.insert(*local, Local::Val(name.clone()));
                out.push(Stmt::Let(Pat::Bind(name, mutable), t, Some(v)));
            }
            IrStmt::Assign { target, op, value } => {
                let s = self.assign(target, *op, value)?;
                out.push(Stmt::Expr(s));
            }
            IrStmt::While { cond, body } => {
                if matches!(cond.kind, ExprKind::Bool(true)) {
                    if let Some(s) = self.while_let(body)? {
                        out.push(s);
                        return Ok(());
                    }
                }
                let b = self.block(body, Want::Own)?;
                if matches!(cond.kind, ExprKind::Bool(true)) {
                    out.push(Stmt::Loop(b));
                } else {
                    let c = self.expr(cond, Want::Op)?;
                    out.push(Stmt::While(c, b));
                }
            }
            IrStmt::ForList {
                var,
                mode,
                list,
                body,
            } => {
                let elem = match &list.ty {
                    IrType::List(t) => (**t).clone(),
                    _ => IrType::Void,
                };
                let name = self.local_name(*var);
                let used = self.f.reads.contains(var) || self.f.mutated.contains(var);
                let (iter, pat, local) = match mode {
                    Mode::Mut => {
                        let place = self.place(list)?;
                        (
                            method(place, "iter_mut", Vec::new()),
                            Pat::Bind(name.clone(), false),
                            Local::Mut(name.clone()),
                        )
                    }
                    Mode::Own => {
                        let v = self.expr(list, Want::Own)?;
                        (
                            v,
                            Pat::Bind(name.clone(), self.f.mutated.contains(var)),
                            Local::Val(name.clone()),
                        )
                    }
                    Mode::Read => {
                        let (v, owned) = self.iterable(list)?;
                        if owned {
                            (v, Pat::Bind(name.clone(), false), Local::Val(name.clone()))
                        } else if self.copy(&elem) {
                            (
                                v,
                                Pat::Ref(Box::new(Pat::Bind(name.clone(), false))),
                                Local::Val(name.clone()),
                            )
                        } else {
                            (v, Pat::Bind(name.clone(), false), Local::Ref(name.clone()))
                        }
                    }
                };
                self.f.locals.insert(*var, local);
                let b = self.block(body, Want::Own)?;
                let pat = if used { pat } else { Pat::Wild };
                out.push(Stmt::For(pat, iter, b));
            }
            IrStmt::ForRange {
                var,
                start,
                end,
                inclusive,
                body,
            } => {
                let saved = self.f.suffix;
                self.f.suffix = true;
                let s = self.expr(start, Want::Op)?;
                self.f.suffix = saved;
                let e = self.expr(end, Want::Op)?;
                let name = self.local_name(*var);
                self.f.locals.insert(*var, Local::Val(name.clone()));
                let used = self.f.reads.contains(var);
                let b = self.block(body, Want::Own)?;
                let pat = if used {
                    Pat::Bind(name, false)
                } else {
                    Pat::Wild
                };
                out.push(Stmt::For(
                    pat,
                    Expr::Range(Box::new(s), Box::new(e), *inclusive),
                    b,
                ));
            }
            IrStmt::Expr(e) => {
                let v = self.expr(e, Want::Own)?;
                if matches!(e.ty, IrType::Result(..)) {
                    out.push(Stmt::Let(Pat::Wild, None, Some(v)));
                } else if !matches!(v, Expr::Tuple(ref t) if t.is_empty()) {
                    out.push(Stmt::Expr(v));
                }
            }
        }
        Ok(())
    }

    /// `while true { match e { p => body, _ => break } }` as `while let p = e { body }`.
    fn while_let(&mut self, body: &'p Block) -> Result<Option<Stmt>, Error> {
        let only: &IrExpr = match (body.stmts.as_slice(), &body.tail) {
            ([IrStmt::Expr(e)], None) => e,
            ([], Some(e)) => e,
            _ => return Ok(None),
        };
        let ExprKind::Match {
            scrutinee,
            mode,
            arms,
        } = &only.kind
        else {
            return Ok(None);
        };
        let [first, second] = arms.as_slice() else {
            return Ok(None);
        };
        let breaks = |a: &Arm| {
            a.guard.is_none()
                && match (a.body.stmts.as_slice(), &a.body.tail) {
                    ([], Some(t)) => matches!(t.kind, ExprKind::Break),
                    ([IrStmt::Expr(t)], None) => matches!(t.kind, ExprKind::Break),
                    _ => false,
                }
        };
        let (keep, stop) = if breaks(second) {
            (first, second)
        } else if breaks(first) {
            (second, first)
        } else {
            return Ok(None);
        };
        if keep.guard.is_some() || !matches!(stop.pattern, Pattern::Wild | Pattern::None) {
            return Ok(None);
        }
        let Expr::Match(subject, mut rarms) = self.match_(scrutinee, *mode, arms, Want::Own)?
        else {
            return Ok(None);
        };
        let index = if std::ptr::eq(keep, first) { 0 } else { 1 };
        let arm = rarms.remove(index);
        let b = match arm.body {
            Expr::Block(b) => b,
            other => block(vec![Stmt::Expr(other)], None),
        };
        Ok(Some(Stmt::WhileLet(arm.pat, *subject, b)))
    }

    /// The list a `for` loop reads: a reference to it, or the list itself if it is a temporary
    /// (then the loop variable owns each element).
    fn iterable(&mut self, list: &'p IrExpr) -> Result<(Expr, bool), Error> {
        match &list.kind {
            ExprKind::Local(_)
            | ExprKind::Field { .. }
            | ExprKind::SelfValue
            | ExprKind::Index { .. } => Ok((self.expr(list, Want::Read(true))?, false)),
            _ => {
                let v = self.expr(list, Want::Own)?;
                Ok((
                    match v {
                        Expr::Macro(n, items) if n == "vec" => Expr::Array(items),
                        other => other,
                    },
                    true,
                ))
            }
        }
    }

    fn assign(
        &mut self,
        target: &'p IrExpr,
        op: Option<BinaryOp>,
        value: &'p IrExpr,
    ) -> Result<Expr, Error> {
        let place = self.place(target)?;
        let ty = &target.ty;
        let Some(op) = op else {
            let v = self.expr(value, Want::Own)?;
            return Ok(bin(place, "=", v));
        };
        if matches!(ty, IrType::String) && op == BinaryOp::Add {
            // One character is pushed as a `char`: `s.push('?')`.
            if let ExprKind::Str(text) = &value.kind {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    return Ok(method(place, "push", vec![lit(&format!("{c:?}"))]));
                }
            }
            let v = self.expr(value, Want::Read(true))?;
            return Ok(method(place, "push_str", vec![v]));
        }
        let v = self.expr(value, Want::Op)?;
        let sym = match op {
            BinaryOp::Add => "+=",
            BinaryOp::Sub => "-=",
            BinaryOp::Mul => "*=",
            BinaryOp::Div => "/=",
            BinaryOp::Rem => "%=",
            BinaryOp::BitAnd => "&=",
            BinaryOp::BitOr => "|=",
            BinaryOp::BitXor => "^=",
            BinaryOp::Shl | BinaryOp::Shr => {
                let f = if op == BinaryOp::Shl { "shl" } else { "shr" };
                let current = self.expr(target, Want::Op)?;
                let nv = self.rt_call("int", f, vec![current, v]);
                return Ok(bin(place, "=", nv));
            }
            _ => return Err("a compound assignment".to_string()),
        };
        Ok(bin(place, sym, v))
    }

    /// A place that can be assigned or borrowed mutably.
    fn place(&mut self, e: &'p IrExpr) -> Result<Expr, Error> {
        Ok(match &e.kind {
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Val(n)) | Some(Local::Boxed(n)) => path(&n),
                Some(Local::Mut(n)) | Some(Local::Ref(n)) => deref(path(&n)),
                None => return Err(format!("unknown local {l:?}")),
            },
            ExprKind::SelfValue => match self.f.self_local.clone() {
                Some(Local::Mut(n)) | Some(Local::Ref(n)) => deref(path(&n)),
                Some(Local::Val(n)) | Some(Local::Boxed(n)) => path(&n),
                None => return Err("`self` outside a method".to_string()),
            },
            ExprKind::Field { base, name } => {
                let b = self.place_base(base)?;
                let f = Expr::Field(Box::new(b), value(name));
                if self.field_boxed(&base.ty, name) {
                    deref(f)
                } else {
                    f
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.place_base(base)?;
                if matches!(base.ty, IrType::Map(..)) {
                    let k = self.key(index)?;
                    deref(method(b, "at_mut", vec![k]))
                } else {
                    self.rt_trait("At", "At");
                    let i = self.expr(index, Want::Op)?;
                    deref(method(b, "at_mut", vec![i]))
                }
            }
            _ => self.expr(e, Want::Own)?,
        })
    }

    /// The base of a place: method calls and fields dereference it themselves.
    fn place_base(&mut self, e: &'p IrExpr) -> Result<Expr, Error> {
        let p = self.place(e)?;
        Ok(match p {
            Expr::Unary("*", inner) if matches!(*inner, Expr::Path(_)) => *inner,
            other => other,
        })
    }

    /// A key of a map: a reference.
    fn key(&mut self, k: &'p IrExpr) -> Result<Expr, Error> {
        if matches!(k.kind, ExprKind::Str(_)) {
            return self.expr(k, Want::Op);
        }
        let v = self.expr(k, Want::Read(true))?;
        Ok(v)
    }

    // ----- expressions ------------------------------------------------------------------------

    /// A value of type `t` in its form for `want`, from an owned value or a place.
    fn shape(&mut self, e: Expr, t: &IrType, want: Want, place: bool) -> Expr {
        let copy = self.copy(t);
        match want {
            Want::Own | Want::Op => e,
            Want::Read(by_ref) => {
                if copy && !by_ref {
                    e
                } else if place || !matches!(e, Expr::Str(_)) {
                    reference(e)
                } else {
                    e
                }
            }
            Want::Mut => Expr::Unary("&mut ", Box::new(e)),
        }
    }

    /// A value behind a reference (`&T`) in its form for `want`.
    fn shape_ref(&mut self, e: Expr, t: &IrType, want: Want) -> Expr {
        let copy = self.copy(t);
        match want {
            Want::Own => {
                if copy {
                    deref(e)
                } else {
                    owned_copy(e, t)
                }
            }
            Want::Op => {
                if copy {
                    deref(e)
                } else {
                    e
                }
            }
            Want::Read(by_ref) => {
                if copy && !by_ref {
                    deref(e)
                } else {
                    e
                }
            }
            Want::Mut => e,
        }
    }

    fn expr(&mut self, e: &'p IrExpr, want: Want) -> Result<Expr, Error> {
        let t = &e.ty;
        Ok(match &e.kind {
            ExprKind::Int(v) => {
                let text = if self.f.suffix {
                    format!("{v}i64")
                } else {
                    v.to_string()
                };
                self.shape(lit(&text), t, want, false)
            }
            ExprKind::Float(v) => self.shape(float_lit(*v), t, want, false),
            ExprKind::Bool(b) => self.shape(lit(&b.to_string()), t, want, false),
            ExprKind::Unit => Expr::Tuple(Vec::new()),
            ExprKind::Str(s) => match want {
                Want::Own => to_string(Expr::Str(s.clone())),
                _ => Expr::Str(s.clone()),
            },
            ExprKind::Local(l) => match self.f.locals.get(l).cloned() {
                Some(Local::Val(n)) => self.shape(path(&n), t, want, true),
                Some(Local::Ref(n)) => self.shape_ref(path(&n), t, want),
                Some(Local::Mut(n)) => match want {
                    Want::Mut => path(&n),
                    Want::Read(_) | Want::Op if !self.copy(t) => path(&n),
                    _ => self.shape(deref(path(&n)), t, want, true),
                },
                Some(Local::Boxed(n)) => match want {
                    Want::Own => deref(path(&n)),
                    _ => self.shape(path(&n), t, want, true),
                },
                None => return Err(format!("unknown local {l:?}")),
            },
            ExprKind::SelfValue => match self.f.self_local.clone() {
                Some(Local::Ref(n)) => self.shape_ref(path(&n), t, want),
                Some(Local::Mut(n)) => match want {
                    Want::Mut | Want::Read(_) => path(&n),
                    Want::Op if !self.copy(t) => path(&n),
                    _ => self.shape(deref(path(&n)), t, want, true),
                },
                Some(Local::Boxed(n)) if want == Want::Own => deref(path(&n)),
                Some(Local::Val(n)) | Some(Local::Boxed(n)) => self.shape(path(&n), t, want, true),
                None => return Err("`self` outside a method".to_string()),
            },
            ExprKind::Const(def) => {
                let p = path(&self.def_path(*def));
                match (t, want) {
                    (IrType::String, Want::Own) => to_string(p),
                    // A constant string is a `&str` already.
                    (IrType::String, _) => p,
                    _ => self.shape(p, t, want, false),
                }
            }
            ExprKind::FnValue(def) => {
                let f = self.fn_value(*def, t)?;
                match want {
                    Want::Read(_) => reference(f),
                    _ => call(path("Box::new"), vec![f]),
                }
            }
            ExprKind::Call { callee, args } => {
                let c = self.call(callee, args, t)?;
                self.shape(c, t, want, false)
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base, Want::Op)?;
                let f = Expr::Field(Box::new(b), value(name));
                let boxed = self.field_boxed(&base.ty, name);
                if boxed && want == Want::Own {
                    deref(f)
                } else if boxed && !matches!(t, IrType::Named(..)) && want != Want::Op {
                    // A pattern does not see through `Box<Option<Node>>`: `&*n.next`.
                    let inner = Expr::Unary("*", Box::new(f));
                    match want {
                        Want::Mut => Expr::Unary("&mut ", Box::new(inner)),
                        _ => Expr::Unary("&", Box::new(inner)),
                    }
                } else {
                    self.shape(f, t, want, true)
                }
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base, Want::Op)?;
                let at = if matches!(base.ty, IrType::Map(..)) {
                    let k = self.key(index)?;
                    method(b, "at", vec![k])
                } else {
                    self.rt_trait("At", "At");
                    let saved = self.f.suffix;
                    self.f.suffix = false;
                    let i = self.expr(index, Want::Op)?;
                    self.f.suffix = saved;
                    method(b, "at", vec![i])
                };
                match want {
                    Want::Mut => {
                        let Expr::Method(r, _, ta, a) = at else {
                            return Err("index".to_string());
                        };
                        Expr::Method(r, "at_mut".to_string(), ta, a)
                    }
                    _ => self.shape_ref(at, t, want),
                }
            }
            ExprKind::Slice {
                base,
                start,
                end,
                inclusive,
            } => {
                let b = self.expr(base, Want::Read(true))?;
                let s = match start {
                    Some(s) => self.expr(s, Want::Op)?,
                    None => lit("0"),
                };
                let x = match end {
                    Some(x) => {
                        let v = self.expr(x, Want::Op)?;
                        Some(if *inclusive { bin(v, "+", lit("1")) } else { v })
                    }
                    None => None,
                };
                let string = matches!(base.ty, IrType::String);
                let v = match (string, x) {
                    (true, Some(x)) => self.rt_call("slice_str", "slice_str", vec![b, s, x]),
                    (true, None) => self.rt_call("slice_str", "slice_str_from", vec![b, s]),
                    (false, Some(x)) => self.rt_call("slice", "slice", vec![b, s, x]),
                    (false, None) => self.rt_call("slice", "slice_from", vec![b, s]),
                };
                self.shape(v, t, want, false)
            }
            ExprKind::Struct { def, fields } => {
                let mut list = Vec::new();
                for (i, (n, v)) in fields.iter().enumerate() {
                    let mut x = self.expr(v, Want::Own)?;
                    if self.boxed.contains(&(*def, i)) {
                        x = call(path("Box::new"), vec![x]);
                    }
                    list.push((value(n), x));
                }
                let name = self.def_path(*def);
                self.shape(Expr::Struct(name, list), t, want, false)
            }
            ExprKind::Variant { def, values } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("a variant without an enum".to_string()),
                };
                let en = self.enums.get(&parent).copied().ok_or("enum")?;
                let vpath = self.variant_path(*def, parent);
                let mut first = 0;
                let mut variant = None;
                for v in &en.variants {
                    if v.def == *def {
                        variant = Some(v);
                        break;
                    }
                    first += v.payload.len();
                }
                let variant = variant.ok_or("variant")?;
                let mut vs = Vec::new();
                for (i, v) in values.iter().enumerate() {
                    let mut x = self.expr(v, Want::Own)?;
                    if self.boxed.contains(&(parent, first + i)) {
                        x = call(path("Box::new"), vec![x]);
                    }
                    vs.push(x);
                }
                let v = match &variant.payload {
                    Payload::None => path(&vpath),
                    Payload::Tuple(_) => call(path(&vpath), vs),
                    Payload::Struct(fs) => {
                        Expr::Struct(vpath, fs.iter().map(|f| value(&f.name)).zip(vs).collect())
                    }
                };
                self.shape(v, t, want, false)
            }
            ExprKind::Some(v) => {
                let x = self.expr(v, Want::Own)?;
                self.shape(call(path("Some"), vec![x]), t, want, false)
            }
            ExprKind::None => self.shape(path("None"), t, want, false),
            ExprKind::Ok(v) => {
                let x = self.expr(v, Want::Own)?;
                self.shape(call(path("Ok"), vec![x]), t, want, false)
            }
            ExprKind::Err(v) => {
                let x = self.expr(v, Want::Own)?;
                self.shape(call(path("Err"), vec![x]), t, want, false)
            }
            ExprKind::List(items) => {
                let mut vs = Vec::new();
                let elem = match t {
                    IrType::List(a) => (**a).clone(),
                    _ => IrType::Void,
                };
                let iface = matches!(&elem, IrType::Named(d, _) if self.ifaces.contains_key(d));
                for i in items {
                    let mut x = self.expr(i, Want::Own)?;
                    if iface && matches!(i.kind, ExprKind::Coerce { .. }) {
                        let et = self.ty(&elem)?;
                        x = Expr::Cast(Box::new(x), et);
                    }
                    vs.push(x);
                }
                match want {
                    Want::Read(_) => reference(Expr::Array(vs)),
                    _ => Expr::Macro("vec".to_string(), vs),
                }
            }
            ExprKind::Map(entries) => {
                let mut pairs = Vec::new();
                for (k, v) in entries {
                    let a = self.expr(k, Want::Own)?;
                    let b = self.expr(v, Want::Own)?;
                    pairs.push(Expr::Tuple(vec![a, b]));
                }
                let r = self.rt("Map");
                let m = if pairs.is_empty() {
                    call(path(&format!("{r}::Map::new")), Vec::new())
                } else {
                    call(path(&format!("{r}::Map::from")), vec![Expr::Array(pairs)])
                };
                self.shape(m, t, want, false)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.expr(operand, Want::Op)?;
                let x = match op {
                    UnaryOp::Not | UnaryOp::BitNot => not(v),
                    UnaryOp::Neg => match v {
                        Expr::Lit(t) if !t.starts_with('-') => lit(&format!("-{t}")),
                        v => Expr::Unary("-", Box::new(v)),
                    },
                };
                self.shape(x, t, want, false)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let x = self.binary(*op, lhs, rhs)?;
                self.shape(x, t, want, false)
            }
            ExprKind::Closure { params, body } => {
                let c = self.closure(params, body, t)?;
                match want {
                    Want::Read(_) => reference(c),
                    _ => call(path("Box::new"), vec![c]),
                }
            }
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Block(_)
                if !self.copy(t)
                    && matches!(want, Want::Op | Want::Read(_))
                    && !matches!(t, IrType::Void | IrType::Never) =>
            {
                // Branches agree on owned values; a read argument borrows the result.
                let v = self.expr(e, Want::Own)?;
                match want {
                    Want::Read(_) => reference(v),
                    _ => v,
                }
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.expr(cond, Want::Op)?;
                let tb = self.block(then, want)?;
                let o = match otherwise {
                    Some(o) => {
                        // `else if` stays a chain.
                        if o.stmts.is_empty() {
                            if let Some(tail) = &o.tail {
                                if matches!(tail.kind, ExprKind::If { .. }) {
                                    let inner = self.expr(tail, want)?;
                                    return Ok(Expr::If(Box::new(c), tb, Some(Box::new(inner))));
                                }
                            }
                        }
                        Some(Box::new(Expr::Block(self.block(o, want)?)))
                    }
                    None => None,
                };
                Expr::If(Box::new(c), tb, o)
            }
            ExprKind::Match {
                scrutinee,
                mode,
                arms,
            } => self.match_(scrutinee, *mode, arms, want)?,
            ExprKind::Block(b) => Expr::Block(self.block(b, want)?),
            ExprKind::Return(v) => match v {
                Some(v) => Expr::Return(Some(Box::new(self.expr(v, Want::Own)?))),
                None => Expr::Return(None),
            },
            ExprKind::Break => Expr::Break,
            ExprKind::Continue => Expr::Continue,
            ExprKind::Try { value, .. } => {
                let v = self.expr(value, Want::Own)?;
                let x = Expr::Try(Box::new(v));
                self.shape(x, t, want, false)
            }
            ExprKind::Coerce { value, .. } => match want {
                // A read argument of an interface type is a `&dyn Trait`.
                Want::Read(_) => self.expr(value, Want::Read(true))?,
                _ => {
                    let v = self.expr(value, Want::Own)?;
                    call(path("Box::new"), vec![v])
                }
            },
            ExprKind::Copy(v) => {
                let x = self.expr(v, Want::Op)?;
                let c = if self.copy(&v.ty) {
                    x
                } else if self.unsized_ref(v) {
                    owned_copy(x, &v.ty)
                } else {
                    method(x, "clone", Vec::new())
                };
                self.shape(c, t, want, false)
            }
            ExprKind::Panic { message, location } => {
                let m = self.expr(message, Want::Read(true))?;
                self.rt_call("Panic", "panic_at", vec![m, Expr::Str(location.clone())])
            }
            ExprKind::Assert {
                cond,
                text,
                location,
            } => {
                let c = self.expr(cond, Want::Op)?;
                self.rt_call(
                    "Panic",
                    "check",
                    vec![c, Expr::Str(text.clone()), Expr::Str(location.clone())],
                )
            }
        })
    }

    /// Whether an expression is a read parameter of type `String` or `List`, which is a `&str`
    /// or a `&[T]`.
    fn unsized_ref(&self, e: &IrExpr) -> bool {
        if !matches!(e.ty, IrType::String | IrType::List(_)) {
            return false;
        }
        match &e.kind {
            ExprKind::Local(l) => self.f.unsized_params.contains(l),
            _ => false,
        }
    }

    fn field_boxed(&self, base: &IrType, name: &str) -> bool {
        let IrType::Named(d, _) = base else {
            return false;
        };
        self.structs
            .get(d)
            .and_then(|s| s.fields.iter().position(|f| f.name == name))
            .is_some_and(|i| self.boxed.contains(&(*d, i)))
    }

    /// A declared function used as a value: a closure with the parameters of function types
    /// (read parameters by reference).
    fn fn_value(&mut self, def: DefRef, _ty: &IrType) -> Result<Expr, Error> {
        let info = self.fns.get(&def).cloned().ok_or("unknown function")?;
        let d = self.p.def(def).ok_or("unknown function")?.clone();
        if d.intrinsic.is_some() {
            return Err(format!("the intrinsic `{}` used as a value", d.name));
        }
        let f = match d.kind {
            DefKind::Method { owner, .. } => {
                let o = self.def_path(owner);
                format!("{o}::{}", self.name_of(def))
            }
            _ => self.def_path(def),
        };
        let mut params = Vec::new();
        let mut args = Vec::new();
        for (i, (m, t)) in info.params.iter().enumerate() {
            let n = format!("p{i}");
            if *m == Mode::Read && self.copy(t) {
                params.push((Pat::Ref(Box::new(Pat::Bind(n.clone(), false))), None));
            } else {
                params.push((Pat::Bind(n.clone(), false), None));
            }
            args.push(path(&n));
        }
        Ok(Expr::Closure(params, Box::new(call(path(&f), args)), false))
    }

    fn closure(
        &mut self,
        params: &'p [ir::Param],
        body: &'p Block,
        ty: &IrType,
    ) -> Result<Expr, Error> {
        let _ = ty;
        let mut ps = Vec::new();
        for p in params {
            let n = self.local_name(p.local);
            let (pat, local) = match p.mode {
                Mode::Read if self.copy(&p.ty) => (
                    Pat::Ref(Box::new(Pat::Bind(n.clone(), false))),
                    Local::Val(n.clone()),
                ),
                Mode::Read => (Pat::Bind(n.clone(), false), Local::Ref(n.clone())),
                Mode::Mut => (Pat::Bind(n.clone(), false), Local::Mut(n.clone())),
                Mode::Own => (
                    Pat::Bind(n.clone(), self.f.mutated.contains(&p.local)),
                    Local::Val(n.clone()),
                ),
            };
            self.f.locals.insert(p.local, local);
            ps.push((pat, None));
        }
        let mut scan = Scan::default();
        scan.block(body);
        let own: BTreeSet<LocalRef> = params.iter().map(|p| p.local).collect();
        let captures = scan
            .reads
            .iter()
            .any(|l| !own.contains(l) && self.f.locals.contains_key(l));
        let b = self.block(body, Want::Own)?;
        let body = match b {
            rs::Block {
                stmts,
                tail: Some(t),
            } if stmts.is_empty() => *t,
            other => Expr::Block(other),
        };
        // `|s| describe(s)` is `describe`, unless a parameter reaches the function through a
        // coercion (`&String` to `&str`, `&Box<dyn T>` to `&dyn T`).
        if let Expr::Call(f, args) = &body {
            let plain = params.iter().all(|p| {
                !matches!(p.ty, IrType::String | IrType::List(_) | IrType::Fn(..))
                    && !matches!(&p.ty, IrType::Named(d, _) if self.ifaces.contains_key(d))
            });
            let same = args.len() == ps.len()
                && ps.iter().zip(args).all(|((pat, _), a)| {
                    matches!((pat, a), (Pat::Bind(n, false), Expr::Path(m)) if n == m)
                });
            if plain && same && matches!(f.as_ref(), Expr::Path(_)) {
                return Ok(*f.clone());
            }
        }
        Ok(Expr::Closure(ps, Box::new(body), captures))
    }

    // ----- operators --------------------------------------------------------------------------

    fn binary(&mut self, op: BinaryOp, lhs: &'p IrExpr, rhs: &'p IrExpr) -> Result<Expr, Error> {
        use BinaryOp as B;
        let ty = &lhs.ty;
        if op == B::Add && matches!(ty, IrType::String) {
            return self.concat(lhs, rhs);
        }
        if matches!(op, B::Shl | B::Shr) {
            let l = self.expr(lhs, Want::Op)?;
            let r = self.expr(rhs, Want::Op)?;
            let f = if op == B::Shl { "shl" } else { "shr" };
            return Ok(self.rt_call("int", f, vec![l, r]));
        }
        if matches!(op, B::Eq | B::NotEq) {
            // `x == None` is `x.is_none()`.
            let other = match (&lhs.kind, &rhs.kind) {
                (_, ExprKind::None) => Some(lhs),
                (ExprKind::None, _) => Some(rhs),
                _ => None,
            };
            if let Some(x) = other {
                let x = self.expr(x, Want::Op)?;
                let f = if op == B::Eq { "is_none" } else { "is_some" };
                return Ok(method(x, f, Vec::new()));
            }
        }
        let mut l = self.expr(lhs, Want::Op)?;
        let mut r = self.expr(rhs, Want::Op)?;
        if matches!(op, B::Eq | B::NotEq | B::Lt | B::Le | B::Gt | B::Ge) && !self.copy(ty) {
            // Both sides at the same level of references.
            let (a, b) = (is_ref(&l), is_ref(&r));
            let lit_l = matches!(l, Expr::Str(_));
            let lit_r = matches!(r, Expr::Str(_));
            if a && !b && !lit_l {
                l = deref(l);
            } else if b && !a && !lit_r {
                r = deref(r);
            }
        }
        let sym = match op {
            B::Add => "+",
            B::Sub => "-",
            B::Mul => "*",
            B::Div => "/",
            B::Rem => "%",
            B::BitAnd => "&",
            B::BitOr => "|",
            B::BitXor => "^",
            B::Eq => "==",
            B::NotEq => "!=",
            B::Lt => "<",
            B::Le => "<=",
            B::Gt => ">",
            B::Ge => ">=",
            B::And => "&&",
            B::Or => "||",
            B::Shl | B::Shr => "<<",
        };
        Ok(bin(l, sym, r))
    }

    /// `a + b + c` of strings: `format!("{a}{b}{c}")`, literals written into the format.
    fn concat(&mut self, lhs: &'p IrExpr, rhs: &'p IrExpr) -> Result<Expr, Error> {
        let mut parts: Vec<&'p IrExpr> = Vec::new();
        fn flatten<'a>(e: &'a IrExpr, out: &mut Vec<&'a IrExpr>) {
            match &e.kind {
                ExprKind::Binary {
                    op: BinaryOp::Add,
                    lhs,
                    rhs,
                } if matches!(e.ty, IrType::String) => {
                    flatten(lhs, out);
                    flatten(rhs, out);
                }
                _ => out.push(e),
            }
        }
        flatten(lhs, &mut parts);
        flatten(rhs, &mut parts);
        let mut fmt = String::new();
        let mut args = Vec::new();
        for p in parts {
            if let ExprKind::Str(s) = &p.kind {
                fmt.push_str(&s.replace('{', "{{").replace('}', "}}"));
                continue;
            }
            let v = match displayed(p) {
                Some(inner) => self.expr(inner, Want::Op)?,
                None => self.expr(p, Want::Op)?,
            };
            match &v {
                Expr::Path(n) if !n.contains("::") && !n.starts_with("r#") => {
                    fmt.push_str(&format!("{{{n}}}"));
                }
                _ => {
                    fmt.push_str("{}");
                    args.push(v);
                }
            }
        }
        let mut all = vec![Expr::Str(fmt)];
        all.extend(args);
        Ok(Expr::Macro("format".to_string(), all))
    }

    // ----- calls ------------------------------------------------------------------------------

    /// An argument for a parameter of the declared type `declared` and mode `mode`.
    fn arg(&mut self, a: &'p IrExpr, mode: Mode, declared: &IrType) -> Result<Expr, Error> {
        match mode {
            Mode::Mut => self.expr(a, Want::Mut),
            Mode::Own => self.expr(a, Want::Own),
            Mode::Read => {
                let by_ref = !self.copy(declared);
                let generic = mentions_param(declared);
                let saved = self.f.suffix;
                if generic {
                    self.f.suffix = true;
                }
                let v = self.expr(a, Want::Read(by_ref));
                self.f.suffix = saved;
                v
            }
        }
    }

    fn call(
        &mut self,
        callee: &'p Callee,
        args: &'p [ir::Arg],
        ty: &IrType,
    ) -> Result<Expr, Error> {
        let (def, recv) = match callee {
            Callee::Fn(def) => (Some(*def), None),
            Callee::Method { method, recv } => (Some(*method), Some(recv.as_ref())),
            Callee::Interface { recv, .. } => (None, Some(recv.as_ref())),
            Callee::Value(_) => (None, None),
        };
        if let Some(def) = def {
            let d = self.p.def(def).ok_or("unknown function")?;
            if let Some(name) = d.intrinsic.clone() {
                return self.intrinsic(&name, recv, args, ty);
            }
        }
        let info = match callee {
            Callee::Fn(def) | Callee::Method { method: def, .. } => self.fns.get(def).cloned(),
            Callee::Interface {
                interface, name, ..
            } => self.iface_fns.get(&(*interface, name.clone())).cloned(),
            Callee::Value(f) => match &f.ty {
                IrType::Fn(params, ret) => Some(FnInfo {
                    params: params.clone(),
                    ret: (**ret).clone(),
                    owner: None,
                    self_mode: None,
                    boxed_self: false,
                }),
                _ => None,
            },
        }
        .ok_or("a call of an unknown function")?;
        let mut argv = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let (mode, declared) = info
                .params
                .get(i)
                .cloned()
                .unwrap_or((Mode::Read, a.value.ty.clone()));
            // Function values take every read argument by reference.
            let declared = if matches!(callee, Callee::Value(_)) && mode == Mode::Read {
                IrType::Param("T".to_string())
            } else {
                declared
            };
            argv.push(self.arg(&a.value, mode, &declared)?);
        }
        Ok(match callee {
            Callee::Fn(def) => {
                let d = self.p.def(*def).ok_or("function")?.clone();
                match d.kind {
                    DefKind::Method { owner, .. } => {
                        let o = self.def_path(owner);
                        call(path(&format!("{o}::{}", self.name_of(*def))), argv)
                    }
                    _ => {
                        let f = self.def_path(*def);
                        call(path(&f), argv)
                    }
                }
            }
            Callee::Method { method: m, recv } => {
                let mut r = self.receiver(recv, info.self_mode.unwrap_or(Mode::Read))?;
                if info.boxed_self {
                    r = call(path("Box::new"), vec![r]);
                }
                if let Some(owner) = info.owner {
                    self.trait_in_scope(*m, owner);
                }
                Expr::Method(Box::new(r), self.name_of(*m), Vec::new(), argv)
            }
            Callee::Interface {
                interface,
                name,
                recv,
                ..
            } => {
                let r = self.receiver(recv, info.self_mode.unwrap_or(Mode::Read))?;
                let param = matches!(recv.ty, IrType::Param(_));
                let dyn_value =
                    matches!(&recv.ty, IrType::Named(d, _) if self.ifaces.contains_key(d));
                if param || dyn_value {
                    // The bound or the trait object brings the method into scope.
                } else if Some(*interface) == self.comparable {
                    self.rt_trait("Comparable", "Comparable");
                } else if Some(*interface) == self.error_iface {
                    self.rt_trait("Error", "Error");
                } else {
                    let _ = self.def_path(*interface);
                }
                let r = if info.boxed_self && !dyn_value {
                    call(path("Box::new"), vec![r])
                } else {
                    r
                };
                Expr::Method(Box::new(r), value(name), Vec::new(), argv)
            }
            Callee::Value(f) => {
                let fv = self.expr(f, Want::Op)?;
                call(paren_callee(fv), argv)
            }
        })
    }

    /// A method of an interface implementation needs its trait in scope.
    fn trait_in_scope(&mut self, method: DefRef, owner: DefRef) {
        let iface = self
            .structs
            .get(&owner)
            .map(|s| &s.methods)
            .or_else(|| self.enums.get(&owner).map(|e| &e.methods))
            .and_then(|ms| ms.iter().find(|f| f.def == method))
            .and_then(|f| f.interface);
        if let Some(i) = iface {
            if Some(i) == self.error_iface {
                self.rt_trait("Error", "Error");
            } else if Some(i) == self.comparable {
                self.rt_trait("Comparable", "Comparable");
            } else if self.def_module(i) != Some(self.module) {
                let _ = self.def_path(i);
            }
        }
    }

    fn receiver(&mut self, recv: &'p IrExpr, mode: Mode) -> Result<Expr, Error> {
        // Methods dereference a box themselves.
        if let ExprKind::Local(l) = &recv.kind {
            if let Some(Local::Boxed(n)) = self.f.locals.get(l) {
                return Ok(path(n));
            }
        }
        match mode {
            Mode::Own => self.expr(recv, Want::Own),
            Mode::Mut => self.place_base(recv),
            Mode::Read => self.expr(recv, Want::Op),
        }
    }

    // ----- intrinsics -------------------------------------------------------------------------

    fn intrinsic(
        &mut self,
        name: &str,
        recv: Option<&'p IrExpr>,
        args: &'p [ir::Arg],
        _ty: &IrType,
    ) -> Result<Expr, Error> {
        let short = name.strip_prefix("std.").unwrap_or(name);
        let mut items: Vec<&'p IrExpr> = Vec::new();
        if let Some(r) = recv {
            items.push(r);
        }
        items.extend(args.iter().map(|a| &a.value));
        let first = items.first().copied();
        let first_ty = first.map(|e| e.ty.clone()).unwrap_or(IrType::Void);
        let elem = match &first_ty {
            IrType::List(t) => (**t).clone(),
            IrType::Map(_, v) => (**v).clone(),
            _ => IrType::Void,
        };
        let copy_elem = self.copy(&elem);
        let get = |i: usize| items.get(i).copied().ok_or("missing argument".to_string());
        let read = |s: &mut Self, i: usize| -> Result<Expr, Error> {
            let e = get(i)?;
            s.expr(e, Want::Read(true))
        };
        let op = |s: &mut Self, i: usize| -> Result<Expr, Error> {
            let e = get(i)?;
            let v = s.expr(e, Want::Op)?;
            // A method of a float literal needs its type: `2.5_f64.round()`.
            Ok(match v {
                Expr::Lit(t) if matches!(e.ty, IrType::Float) && i == 0 => {
                    if t.starts_with('-') {
                        Expr::Paren(Box::new(lit(&format!("{t}_f64"))))
                    } else {
                        lit(&format!("{t}_f64"))
                    }
                }
                other => other,
            })
        };
        let own = |s: &mut Self, i: usize| -> Result<Expr, Error> {
            let e = get(i)?;
            s.expr(e, Want::Own)
        };
        let place = |s: &mut Self, i: usize| -> Result<Expr, Error> {
            let e = get(i)?;
            s.place_base(e)
        };
        let fun = |s: &mut Self, i: usize| -> Result<Expr, Error> {
            let e = get(i)?;
            s.fn_arg(e)
        };
        let len = |x: Expr| Expr::Cast(Box::new(method(x, "len", Vec::new())), Type::name("i64"));
        let collect = |x: Expr| {
            Expr::Method(
                Box::new(x),
                "collect".to_string(),
                vec![Type::Path("Vec".to_string(), vec![Type::name("_")])],
                Vec::new(),
            )
        };
        let iter = |x: Expr, copy: bool| {
            let i = method(iter_recv(x), "iter", Vec::new());
            if copy {
                method(i, "copied", Vec::new())
            } else {
                i
            }
        };
        Ok(match short {
            "io.print" => self.print_macro("println", get(0)?)?,
            "io.write" => self.print_macro("print", get(0)?)?,
            "io.eprint" => self.print_macro("eprintln", get(0)?)?,
            "io.read_line" => self.rt_call("io", "read_line", Vec::new()),
            "io.args" => self.rt_call("io", "args", Vec::new()),
            "process.exit" => {
                let c = op(self, 0)?;
                self.rt_call("exit", "exit", vec![c])
            }
            "process.env" => {
                let n = read(self, 0)?;
                method(call(path("std::env::var"), vec![n]), "ok", Vec::new())
            }
            "prelude.replace" => {
                let p = get(0)?;
                let pl = self.expr(p, Want::Mut)?;
                let v = own(self, 1)?;
                call(path("std::mem::replace"), vec![pl, v])
            }
            "prelude.Bool.to_string" | "math.Int.to_string" => to_string(op(self, 0)?),
            "math.Int.to_float" => Expr::Cast(Box::new(op(self, 0)?), Type::name("f64")),
            "math.Int.abs" | "math.Float.abs" => method(op(self, 0)?, "abs", Vec::new()),
            "math.Int.pow" => {
                let a = op(self, 0)?;
                let b = op(self, 1)?;
                self.rt_call("int", "int_pow", vec![a, b])
            }
            "math.Int.min" | "math.Float.min" => {
                let a = op(self, 0)?;
                method(a, "min", vec![op(self, 1)?])
            }
            "math.Int.max" | "math.Float.max" => {
                let a = op(self, 0)?;
                method(a, "max", vec![op(self, 1)?])
            }
            "math.Int.compare" | "string.String.compare" => {
                self.rt_trait("Comparable", "Comparable");
                let a = op(self, 0)?;
                method(a, "compare", vec![read(self, 1)?])
            }
            "math.Float.to_string" => {
                let x = unparen(op(self, 0)?);
                self.rt_call("float_to_string", "float_to_string", vec![x])
            }
            "math.Float.to_int" => {
                let x = unparen(op(self, 0)?);
                self.rt_call("float_to_int", "float_to_int", vec![x])
            }
            "math.Float.pow" => {
                let a = op(self, 0)?;
                method(a, "powf", vec![op(self, 1)?])
            }
            "math.Float.sqrt" => method(op(self, 0)?, "sqrt", Vec::new()),
            "math.Float.floor" => method(op(self, 0)?, "floor", Vec::new()),
            "math.Float.ceil" => method(op(self, 0)?, "ceil", Vec::new()),
            "math.Float.round" => method(op(self, 0)?, "round", Vec::new()),
            "math.Float.is_nan" => method(op(self, 0)?, "is_nan", Vec::new()),
            "string.String.len" => {
                let s = read(self, 0)?;
                self.rt_call("str_len", "str_len", vec![s])
            }
            "string.String.is_empty" => method(op(self, 0)?, "is_empty", Vec::new()),
            "string.String.contains" => {
                let s = op(self, 0)?;
                method(s, "contains", vec![read(self, 1)?])
            }
            "string.String.starts_with" => {
                let s = op(self, 0)?;
                method(s, "starts_with", vec![read(self, 1)?])
            }
            "string.String.ends_with" => {
                let s = op(self, 0)?;
                method(s, "ends_with", vec![read(self, 1)?])
            }
            "string.String.find" => {
                let s = read(self, 0)?;
                let p = read(self, 1)?;
                self.rt_call("strings", "str_find", vec![s, p])
            }
            "string.String.split" => {
                let s = read(self, 0)?;
                let p = read(self, 1)?;
                self.rt_call("strings", "split", vec![s, p])
            }
            "string.String.lines" => {
                let s = read(self, 0)?;
                self.rt_call("strings", "lines", vec![s])
            }
            "string.String.chars" => {
                let s = read(self, 0)?;
                self.rt_call("strings", "chars", vec![s])
            }
            "string.String.trim" => {
                let s = read(self, 0)?;
                self.rt_call("strings", "trim", vec![s])
            }
            "string.String.replace" => {
                let s = read(self, 0)?;
                let a = read(self, 1)?;
                let b = read(self, 2)?;
                self.rt_call("strings", "replace", vec![s, a, b])
            }
            "string.String.repeat" => {
                let s = read(self, 0)?;
                let n = op(self, 1)?;
                self.rt_call("strings", "repeat", vec![s, n])
            }
            "string.String.to_upper_ascii" => {
                method(op(self, 0)?, "to_ascii_uppercase", Vec::new())
            }
            "string.String.to_lower_ascii" => {
                method(op(self, 0)?, "to_ascii_lowercase", Vec::new())
            }
            "string.String.parse_int" => {
                let s = read(self, 0)?;
                self.rt_call("parse", "parse_int", vec![s])
            }
            "string.String.parse_float" => {
                let s = read(self, 0)?;
                self.rt_call("parse", "parse_float", vec![s])
            }
            "string.join" => {
                let parts = op(self, 0)?;
                method(parts, "join", vec![read(self, 1)?])
            }
            "list.List.new" => call(path("Vec::new"), Vec::new()),
            "list.List.len" => len(op(self, 0)?),
            "list.List.is_empty" => method(op(self, 0)?, "is_empty", Vec::new()),
            "list.List.push" => {
                let l = place(self, 0)?;
                method(l, "push", vec![own(self, 1)?])
            }
            "list.List.pop" => method(place(self, 0)?, "pop", Vec::new()),
            "list.List.insert" => {
                let l = get(0)?;
                let l = self.expr(l, Want::Mut)?;
                let i = op(self, 1)?;
                let v = own(self, 2)?;
                self.rt_call("list", "insert", vec![l, i, v])
            }
            "list.List.remove" => {
                let l = get(0)?;
                let l = self.expr(l, Want::Mut)?;
                let i = op(self, 1)?;
                self.rt_call("list", "remove", vec![l, i])
            }
            "list.List.swap" => {
                let l = get(0)?;
                let l = self.expr(l, Want::Mut)?;
                let i = op(self, 1)?;
                let j = op(self, 2)?;
                self.rt_call("list", "swap", vec![l, i, j])
            }
            "list.List.reverse" => method(place(self, 0)?, "reverse", Vec::new()),
            "list.List.clear" => method(place(self, 0)?, "clear", Vec::new()),
            "list.List.append" => {
                let l = place(self, 0)?;
                method(l, "extend", vec![own(self, 1)?])
            }
            "list.List.map" => {
                let l = op(self, 0)?;
                let f = fun(self, 1)?;
                collect(method(iter(l, false), "map", vec![f]))
            }
            "list.List.filter" => {
                let l = own(self, 0)?;
                let f = fun(self, 1)?;
                collect(method(
                    method(l, "into_iter", Vec::new()),
                    "filter",
                    vec![f],
                ))
            }
            "list.List.any" => {
                let l = op(self, 0)?;
                let f = fun(self, 1)?;
                method(iter(l, false), "any", vec![f])
            }
            "list.List.all" => {
                let l = op(self, 0)?;
                let f = fun(self, 1)?;
                method(iter(l, false), "all", vec![f])
            }
            "list.List.count" => {
                let l = op(self, 0)?;
                let f = fun(self, 1)?;
                let c = method(
                    method(iter(l, copy_elem), "filter", vec![f]),
                    "count",
                    Vec::new(),
                );
                Expr::Cast(Box::new(c), Type::name("i64"))
            }
            "list.contains" => {
                let l = op(self, 0)?;
                let x = read(self, 1)?;
                method(l, "contains", vec![x])
            }
            "list.index_of" => {
                let l = read(self, 0)?;
                let x = read(self, 1)?;
                self.rt_call("list", "index_of", vec![l, x])
            }
            "list.get" => {
                let l = read(self, 0)?;
                let i = op(self, 1)?;
                self.rt_call("list", "get", vec![l, i])
            }
            "list.sort" => {
                let l = get(0)?;
                let l = self.expr(l, Want::Mut)?;
                self.rt_call("sort", "sort", vec![l])
            }
            "list.sort_by" => {
                let l = get(0)?;
                let l = self.expr(l, Want::Mut)?;
                let f = get(1)?;
                let f = self.expr(f, Want::Read(true))?;
                self.rt_call("sort", "sort_by", vec![l, f])
            }
            "list.min" | "list.max" => {
                let l = read(self, 0)?;
                let f = if short.ends_with("min") { "min" } else { "max" };
                self.rt_call("sort", f, vec![l])
            }
            "list.sum" => {
                let l = op(self, 0)?;
                Expr::Method(
                    Box::new(method(iter_recv(l), "iter", Vec::new())),
                    "sum".to_string(),
                    vec![Type::name("i64")],
                    Vec::new(),
                )
            }
            "list.range" => {
                let a = op(self, 0)?;
                let b = op(self, 1)?;
                collect(Expr::Paren(Box::new(Expr::Range(
                    Box::new(a),
                    Box::new(b),
                    false,
                ))))
            }
            "map.Map.new" => {
                let r = self.rt("Map");
                call(path(&format!("{r}::Map::new")), Vec::new())
            }
            "map.Map.len" => method(op(self, 0)?, "len", Vec::new()),
            "map.Map.is_empty" => method(op(self, 0)?, "is_empty", Vec::new()),
            "map.Map.contains_key" => {
                let m = op(self, 0)?;
                let k = self.key(get(1)?)?;
                method(m, "contains_key", vec![k])
            }
            "map.Map.insert" => {
                let m = place(self, 0)?;
                let k = own(self, 1)?;
                let v = own(self, 2)?;
                method(m, "insert", vec![k, v])
            }
            "map.Map.remove" => {
                let m = place(self, 0)?;
                let k = self.key(get(1)?)?;
                method(m, "remove", vec![k])
            }
            "map.Map.keys" => method(op(self, 0)?, "keys", Vec::new()),
            "map.Map.entries" => method(own(self, 0)?, "entries", Vec::new()),
            "map.Map.clear" => method(place(self, 0)?, "clear", Vec::new()),
            "map.get" => {
                let m = op(self, 0)?;
                let k = self.key(get(1)?)?;
                method(m, "get", vec![k])
            }
            "map.values" => method(op(self, 0)?, "values", Vec::new()),
            "fs.read_text" => {
                let p = read(self, 0)?;
                self.rt_call("fs", "read_text", vec![p])
            }
            "fs.write_text" => {
                let p = read(self, 0)?;
                let c = read(self, 1)?;
                self.rt_call("fs", "write_text", vec![p, c])
            }
            "fs.exists" => {
                let p = read(self, 0)?;
                self.rt_call("fs", "exists", vec![p])
            }
            other => return Err(format!("the intrinsic `{other}` for Rust")),
        })
    }

    /// `println!("...")` for `print(text)`: a concatenation is written into the format.
    fn print_macro(&mut self, name: &str, e: &'p IrExpr) -> Result<Expr, Error> {
        let args = match &e.kind {
            ExprKind::Str(s) => vec![Expr::Str(s.replace('{', "{{").replace('}', "}}"))],
            _ => match self.expr(displayed(e).unwrap_or(e), Want::Op)? {
                Expr::Macro(n, args) if n == "format" => args,
                Expr::Path(n) if !n.contains("::") && !n.starts_with("r#") => {
                    vec![Expr::Str(format!("{{{n}}}"))]
                }
                other => vec![Expr::Str("{}".to_string()), other],
            },
        };
        Ok(Expr::Macro(name.to_string(), args))
    }

    /// A function argument of an iterator adapter, which receives `&T`.
    fn fn_arg(&mut self, e: &'p IrExpr) -> Result<Expr, Error> {
        match &e.kind {
            ExprKind::Closure { params, body } => self.closure(params, body, &e.ty),
            ExprKind::FnValue(def) => self.fn_value(*def, &e.ty),
            _ => {
                let f = self.expr(e, Want::Op)?;
                Ok(f)
            }
        }
    }

    // ----- match ------------------------------------------------------------------------------

    fn match_(
        &mut self,
        scrutinee: &'p IrExpr,
        mode: Mode,
        arms: &'p [Arm],
        want: Want,
    ) -> Result<Expr, Error> {
        let place = matches!(
            scrutinee.kind,
            ExprKind::Local(_)
                | ExprKind::Field { .. }
                | ExprKind::SelfValue
                | ExprKind::Index { .. }
        );
        // How bindings reach the values they name.
        let (subject, binding) = match mode {
            Mode::Mut => {
                let p = self.expr(scrutinee, Want::Mut)?;
                (p, BindKind::Mut)
            }
            Mode::Own => (self.expr(scrutinee, Want::Own)?, BindKind::Val),
            Mode::Read if place => {
                let mut v = self.expr(scrutinee, Want::Op)?;
                if let ExprKind::Field { base, name } = &scrutinee.kind {
                    if self.field_boxed(&base.ty, name)
                        && !matches!(scrutinee.ty, IrType::Named(..))
                    {
                        v = Expr::Unary("&", Box::new(Expr::Unary("*", Box::new(v))));
                    }
                }
                if self.copy(&scrutinee.ty) {
                    (v, BindKind::Val)
                } else if is_ref_local(&v, &self.f.locals) || is_ref(&v) {
                    (v, BindKind::Ref)
                } else {
                    (reference(v), BindKind::Ref)
                }
            }
            Mode::Read => (self.expr(scrutinee, Want::Own)?, BindKind::Val),
        };
        let subject = if matches!(scrutinee.ty, IrType::String) && !self.unsized_ref(scrutinee) {
            method(subject, "as_str", Vec::new())
        } else {
            subject
        };
        let mut out = Vec::new();
        for arm in arms {
            let pat = self.pattern(&arm.pattern, &scrutinee.ty, binding)?;
            let guard = match &arm.guard {
                Some(g) => Some(self.expr(g, Want::Op)?),
                None => None,
            };
            let b = self.block(&arm.body, want)?;
            let body = match b {
                rs::Block {
                    stmts,
                    tail: Some(t),
                } if stmts.is_empty() => *t,
                rs::Block { stmts, tail: None } if stmts.is_empty() => {
                    Expr::Block(rs::Block::default())
                }
                other => Expr::Block(other),
            };
            out.push(rs::Arm { pat, guard, body });
        }
        // `match x { p => body, _ => () }` as `if let p = x { body }`.
        if let [first, second] = out.as_slice() {
            let empty = |e: &Expr| {
                matches!(e, Expr::Block(b) if b.stmts.is_empty() && b.tail.is_none())
                    || matches!(e, Expr::Tuple(t) if t.is_empty())
            };
            if second.pat == Pat::Wild
                && first.guard.is_none()
                && second.guard.is_none()
                && empty(&second.body)
            {
                let b = match &first.body {
                    Expr::Block(b) => b.clone(),
                    other => block(vec![Stmt::Expr(other.clone())], None),
                };
                return Ok(Expr::IfLet(first.pat.clone(), Box::new(subject), b, None));
            }
        }
        Ok(Expr::Match(Box::new(subject), out))
    }

    fn pattern(&mut self, p: &Pattern, ty: &IrType, kind: BindKind) -> Result<Pat, Error> {
        Ok(match p {
            Pattern::Wild => Pat::Wild,
            Pattern::Bind(l) => {
                let name = self.local_name(*l);
                let used = self.f.reads.contains(l) || self.f.mutated.contains(l);
                let copy = self.copy(ty);
                let local = match kind {
                    BindKind::Val => Local::Val(name.clone()),
                    BindKind::Ref if copy => Local::Ref(name.clone()),
                    BindKind::Ref => Local::Ref(name.clone()),
                    BindKind::Mut => Local::Mut(name.clone()),
                };
                self.f.locals.insert(*l, local);
                if used {
                    Pat::Bind(name, kind == BindKind::Val && self.f.mutated.contains(l))
                } else {
                    Pat::Wild
                }
            }
            Pattern::Int(v) => Pat::Lit(v.to_string()),
            Pattern::Str(s) => Pat::Lit(rs::quote(s)),
            Pattern::Bool(b) => Pat::Lit(b.to_string()),
            Pattern::Const(d) => Pat::Lit(self.def_path(*d)),
            Pattern::None => Pat::Lit("None".to_string()),
            Pattern::Some(i) => {
                let t = match ty {
                    IrType::Option(a) => (**a).clone(),
                    _ => IrType::Void,
                };
                Pat::Tuple("Some".to_string(), vec![self.pattern(i, &t, kind)?])
            }
            Pattern::Ok(i) | Pattern::Err(i) => {
                let (a, b) = match ty {
                    IrType::Result(a, b) => ((**a).clone(), (**b).clone()),
                    _ => (IrType::Void, IrType::Void),
                };
                let ok = matches!(p, Pattern::Ok(_));
                let t = if ok { a } else { b };
                Pat::Tuple(
                    if ok { "Ok" } else { "Err" }.to_string(),
                    vec![self.pattern(i, &t, kind)?],
                )
            }
            Pattern::Variant { def, fields } => {
                let parent = match self.p.def(*def).map(|d| d.kind) {
                    Some(DefKind::Variant { parent }) => parent,
                    _ => return Err("a pattern of an unknown variant".to_string()),
                };
                let en = self.enums.get(&parent).copied().ok_or("enum")?;
                let vpath = self.variant_path(*def, parent);
                let map: BTreeMap<String, IrType> = match ty {
                    IrType::Named(_, args) => en
                        .generics
                        .iter()
                        .map(|g| g.name.clone())
                        .zip(args.iter().cloned())
                        .collect(),
                    _ => BTreeMap::new(),
                };
                let mut first = 0;
                let mut variant = None;
                for v in &en.variants {
                    if v.def == *def {
                        variant = Some(v);
                        break;
                    }
                    first += v.payload.len();
                }
                let variant = variant.ok_or("variant")?;
                let types: Vec<IrType> = match &variant.payload {
                    Payload::None => Vec::new(),
                    Payload::Tuple(ts) => ts.clone(),
                    Payload::Struct(fs) => fs.iter().map(|f| f.ty.clone()).collect(),
                };
                let mut subs = Vec::new();
                for (i, (sp, t)) in fields.iter().zip(&types).enumerate() {
                    let t = subst(t, &map);
                    let boxed = self.boxed.contains(&(parent, first + i));
                    let sub = self.pattern(sp, &t, kind)?;
                    if boxed {
                        if let Pattern::Bind(l) = sp {
                            let name = self.local_name(*l);
                            let local = match kind {
                                BindKind::Val => Local::Boxed(name),
                                BindKind::Ref => Local::Ref(name),
                                BindKind::Mut => Local::Mut(name),
                            };
                            self.f.locals.insert(*l, local);
                        }
                    }
                    subs.push(sub);
                }
                match &variant.payload {
                    Payload::None => Pat::Lit(vpath),
                    Payload::Tuple(_) => Pat::Tuple(vpath, subs),
                    Payload::Struct(fs) => Pat::Struct(
                        vpath,
                        fs.iter().map(|f| value(&f.name)).zip(subs).collect(),
                        false,
                    ),
                }
            }
            Pattern::Struct { def, fields } => {
                let s = self.structs.get(def).copied().ok_or("struct")?;
                let spath = self.def_path(*def);
                let map: BTreeMap<String, IrType> = match ty {
                    IrType::Named(_, args) => s
                        .generics
                        .iter()
                        .map(|g| g.name.clone())
                        .zip(args.iter().cloned())
                        .collect(),
                    _ => BTreeMap::new(),
                };
                let mut subs = Vec::new();
                for (sp, f) in fields.iter().zip(&s.fields) {
                    let t = subst(&f.ty, &map);
                    subs.push((value(&f.name), self.pattern(sp, &t, kind)?));
                }
                Pat::Struct(spath, subs, false)
            }
            Pattern::Or(alts) => {
                let mut list = Vec::new();
                for a in alts {
                    list.push(self.pattern(a, ty, kind)?);
                }
                Pat::Or(list)
            }
        })
    }
}

/// How the bindings of a `match` hold their values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BindKind {
    Val,
    Ref,
    Mut,
}

fn is_ref_local(e: &Expr, locals: &BTreeMap<LocalRef, Local>) -> bool {
    let Expr::Path(n) = e else {
        return false;
    };
    n == "self"
        || locals
            .values()
            .any(|l| matches!(l, Local::Ref(m) | Local::Mut(m) if m == n))
}

/// A callee that is not a path is written in parentheses: `(self.run)(x)`.
fn paren_callee(e: Expr) -> Expr {
    match e {
        Expr::Path(_) | Expr::Paren(_) => e,
        other => Expr::Paren(Box::new(other)),
    }
}

/// Whether a call's result type involves a type parameter that Rust may not infer.
fn mentions_param_result(e: &IrExpr) -> bool {
    matches!(e.ty, IrType::List(_) | IrType::Map(..))
}

/// The text of a type, for bounds.
fn type_text(t: &Type) -> String {
    match t {
        Type::Path(p, args) if args.is_empty() => p.clone(),
        Type::Path(p, args) => format!(
            "{p}<{}>",
            args.iter().map(type_text).collect::<Vec<_>>().join(", ")
        ),
        _ => String::new(),
    }
}

/// The order of `use` declarations that `rustfmt` writes.
fn use_key(u: &str) -> String {
    u.to_string()
}

/// An owned copy of a value behind a reference: `to_string` for `&str`, `to_vec` for `&[T]`.
fn owned_copy(e: Expr, t: &IrType) -> Expr {
    match t {
        IrType::String => method(e, "to_string", Vec::new()),
        IrType::List(_) => method(e, "to_vec", Vec::new()),
        _ => method(e, "clone", Vec::new()),
    }
}

/// The value of `x.to_string()` for an `Int` or a `Bool`, which `format!` writes the same way.
/// A list literal that is only iterated is an array: `[1, 2, 3].iter()`.
fn iter_recv(e: Expr) -> Expr {
    match e {
        Expr::Macro(n, items) if n == "vec" && !items.is_empty() => Expr::Array(items),
        other => other,
    }
}

/// An argument needs no parentheses that a receiver needs: `f(-0.0_f64)`.
fn unparen(e: Expr) -> Expr {
    match e {
        Expr::Paren(x) => *x,
        other => other,
    }
}

fn displayed(e: &IrExpr) -> Option<&IrExpr> {
    let ExprKind::Call {
        callee: Callee::Method { recv, .. },
        ..
    } = &e.kind
    else {
        return None;
    };
    if matches!(recv.ty, IrType::Int | IrType::Bool) && is_to_string(e) {
        Some(recv)
    } else {
        None
    }
}

fn is_to_string(e: &IrExpr) -> bool {
    matches!(&e.kind, ExprKind::Call { callee: Callee::Method { .. }, args } if args.is_empty())
        && matches!(e.ty, IrType::String)
}

/// Functions of one `impl` block: the interface, the bounds and the functions.
type ImplGroup<'p> = (Option<DefRef>, Vec<Vec<IrType>>, Vec<&'p ir::Function>);
