//! Compiles the IR of a program into bytecode (spec chapter 10, section 4).

use std::collections::{BTreeSet, HashMap, HashSet};

use wolf_ir::{
    Arg, BinaryOp, Block, Callee, ConstValue, DefKind, DefRef, Expr, ExprKind, Function, Item,
    LocalRef, Mode, ModuleRole, Pattern, Program, Stmt, Type, UnaryOp,
};

use crate::code::{BinOp, Chunk, Op, UnOp};
use crate::intrinsics::Intrinsic;
use crate::value::Value;

/// A function of the compiled program.
#[derive(Clone, Debug)]
pub enum Func {
    /// Bytecode.
    Code(Chunk),
    /// A function of std written in Rust.
    Intrinsic(Intrinsic),
    /// A function the VM cannot run: an extern function, or an intrinsic it does not know.
    Missing(String),
}

/// The tags of the variants and structs of std that the VM makes itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tags {
    /// `Some`
    pub some: u32,
    /// `None`
    pub none: u32,
    /// `Ok`
    pub ok: u32,
    /// `Err`
    pub err: u32,
    /// `Ordering.Less`
    pub less: u32,
    /// `Ordering.Equal`
    pub equal: u32,
    /// `Ordering.Greater`
    pub greater: u32,
    /// The struct `std.map.Entry`.
    pub entry: u32,
    /// The struct `std.string.ParseError`.
    pub parse_error: u32,
    /// `FsError.NotFound`
    pub not_found: u32,
    /// `FsError.PermissionDenied`
    pub permission_denied: u32,
    /// `FsError.Other`
    pub other: u32,
}

/// The built-in types of std, for the dispatch of interface methods.
#[derive(Clone, Copy, Debug, Default)]
pub struct Builtins {
    /// `Int`
    pub int: u32,
    /// `Float`
    pub float: u32,
    /// `Bool`
    pub bool: u32,
    /// `String`
    pub string: u32,
    /// `List`
    pub list: u32,
    /// `Map`
    pub map: u32,
    /// `Void`
    pub void: u32,
}

/// A test: its name in the report and its function.
#[derive(Clone, Debug)]
pub struct TestFn {
    /// `src/main.wolf: adds two numbers`
    pub name: String,
    /// The function.
    pub func: u32,
}

/// A compiled program.
#[derive(Clone, Debug)]
pub struct Image {
    /// Every function, by number.
    pub funcs: Vec<Func>,
    /// The methods of each type by name, for calls of interface methods.
    pub dispatch: HashMap<(u32, String), u32>,
    /// The enum of each variant.
    pub parent: HashMap<u32, u32>,
    /// Tags of std.
    pub tags: Tags,
    /// Built-in types.
    pub builtins: Builtins,
    /// The `main` of each module that has one, by module index.
    pub mains: HashMap<usize, (u32, bool, bool)>,
    /// The tests, sorted by file and in source order.
    pub tests: Vec<TestFn>,
}

impl Image {
    /// The function `main` of a module: its number, whether it takes the arguments and whether
    /// it returns a `Result`.
    pub fn main_of(&self, module: usize) -> Option<(u32, bool, bool)> {
        self.mains.get(&module).copied()
    }
}

/// A variable of a function: a local or `self`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Var {
    Local(LocalRef),
    SelfValue,
}

type Error = String;

/// Compiles a program.
pub fn compile(p: &Program) -> Result<Image, Error> {
    let mut c = Compiler::new(p)?;
    c.run()?;
    Ok(Image {
        funcs: c
            .funcs
            .into_iter()
            .map(|f| f.unwrap_or(Func::Missing("an unknown function".to_string())))
            .collect(),
        dispatch: c.dispatch,
        parent: c.parent,
        tags: c.tags,
        builtins: c.builtins,
        mains: c.mains,
        tests: c.tests,
    })
}

struct Compiler<'p> {
    p: &'p Program,
    funcs: Vec<Option<Func>>,
    ids: HashMap<DefRef, u32>,
    dispatch: HashMap<(u32, String), u32>,
    parent: HashMap<u32, u32>,
    tags: Tags,
    builtins: Builtins,
    mains: HashMap<usize, (u32, bool, bool)>,
    tests: Vec<TestFn>,
    /// The field names of each struct.
    fields: HashMap<DefRef, Vec<String>>,
    /// The mode of `self` of each method.
    self_modes: HashMap<DefRef, Mode>,
    /// The mode of `self` of each method of an interface, by interface and name.
    iface_modes: HashMap<(DefRef, String), Mode>,
    /// The value of each constant.
    consts: HashMap<DefRef, Value>,
}

fn find(p: &Program, module: &str, name: &str, kind: impl Fn(DefKind) -> bool) -> Option<u32> {
    p.defs
        .iter()
        .find(|(_, d)| {
            d.name == name
                && kind(d.kind)
                && p.modules
                    .get(d.module)
                    .is_some_and(|m| m.path.join(".") == module)
        })
        .map(|(r, _)| r.0)
}

fn const_value(v: &ConstValue) -> Value {
    match v {
        ConstValue::Int(i) => Value::Int(*i),
        ConstValue::Float(f) => Value::Float(*f),
        ConstValue::Bool(b) => Value::Bool(*b),
        ConstValue::Str(s) => Value::str(s),
    }
}

impl<'p> Compiler<'p> {
    fn new(p: &'p Program) -> Result<Compiler<'p>, Error> {
        let variant = |d: DefKind| matches!(d, DefKind::Variant { .. });
        let structure = |d: DefKind| d == DefKind::Struct;
        let need = |v: Option<u32>, what: &str| v.ok_or_else(|| format!("std has no {what}"));
        let tags = Tags {
            some: need(find(p, "std.prelude", "Some", variant), "Some")?,
            none: need(find(p, "std.prelude", "None", variant), "None")?,
            ok: need(find(p, "std.prelude", "Ok", variant), "Ok")?,
            err: need(find(p, "std.prelude", "Err", variant), "Err")?,
            less: need(find(p, "std.prelude", "Less", variant), "Less")?,
            equal: need(find(p, "std.prelude", "Equal", variant), "Equal")?,
            greater: need(find(p, "std.prelude", "Greater", variant), "Greater")?,
            entry: find(p, "std.map", "Entry", structure).unwrap_or(u32::MAX),
            parse_error: find(p, "std.string", "ParseError", structure).unwrap_or(u32::MAX),
            not_found: find(p, "std.fs", "NotFound", variant).unwrap_or(u32::MAX),
            permission_denied: find(p, "std.fs", "PermissionDenied", variant).unwrap_or(u32::MAX),
            other: find(p, "std.fs", "Other", variant).unwrap_or(u32::MAX),
        };
        let builtin = |name: &str| {
            p.defs
                .iter()
                .find(|(_, d)| d.kind == DefKind::BuiltinType && d.name == name)
                .map_or(u32::MAX, |(r, _)| r.0)
        };
        let builtins = Builtins {
            int: builtin("Int"),
            float: builtin("Float"),
            bool: builtin("Bool"),
            string: builtin("String"),
            list: builtin("List"),
            map: builtin("Map"),
            void: builtin("Void"),
        };
        let mut parent = HashMap::new();
        for (r, d) in &p.defs {
            if let DefKind::Variant { parent: e } = d.kind {
                parent.insert(r.0, e.0);
            }
        }
        Ok(Compiler {
            p,
            funcs: Vec::new(),
            ids: HashMap::new(),
            dispatch: HashMap::new(),
            parent,
            tags,
            builtins,
            mains: HashMap::new(),
            tests: Vec::new(),
            fields: HashMap::new(),
            self_modes: HashMap::new(),
            iface_modes: HashMap::new(),
            consts: HashMap::new(),
        })
    }

    fn new_func(&mut self) -> u32 {
        self.funcs.push(None);
        u32::try_from(self.funcs.len() - 1).unwrap_or(u32::MAX)
    }

    fn set_func(&mut self, id: u32, f: Func) {
        if let Some(slot) = self.funcs.get_mut(id as usize) {
            *slot = Some(f);
        }
    }

    /// Numbers every function, gathers the tables, then compiles the bodies.
    fn run(&mut self) -> Result<(), Error> {
        let p = self.p;
        // Numbers and tables.
        for m in &p.modules {
            for item in &m.items {
                match item {
                    Item::Fn(f) => self.number(f, None),
                    Item::Struct(s) => {
                        self.fields
                            .insert(s.def, s.fields.iter().map(|f| f.name.clone()).collect());
                        for f in &s.methods {
                            self.number(f, Some(s.def));
                        }
                    }
                    Item::Enum(e) => {
                        for f in &e.methods {
                            self.number(f, Some(e.def));
                        }
                    }
                    Item::Builtin(b) => {
                        for f in &b.methods {
                            self.number(f, Some(b.def));
                        }
                    }
                    Item::Interface(i) => {
                        for f in &i.methods {
                            if let Some(mode) = f.self_mode {
                                self.iface_modes.insert((i.def, f.name.clone()), mode);
                            }
                        }
                    }
                    Item::ExternFn(x) => {
                        let id = self.new_func();
                        self.ids.insert(x.def, id);
                        self.set_func(
                            id,
                            Func::Missing(format!("the extern function `{}`", x.name)),
                        );
                    }
                    Item::Const(k) => {
                        self.consts.insert(k.def, const_value(&k.value));
                    }
                    Item::ExternType(_) | Item::Test(_) => {}
                }
            }
        }
        // Bodies.
        let mut tests: Vec<(String, TestFn)> = Vec::new();
        for (mi, m) in p.modules.iter().enumerate() {
            for item in &m.items {
                match item {
                    Item::Fn(f) => {
                        self.function(f)?;
                        if f.name == "main"
                            && f.self_mode.is_none()
                            && matches!(m.role, ModuleRole::Entry | ModuleRole::Example)
                        {
                            if let Some(&id) = self.ids.get(&f.def) {
                                let result = matches!(f.ret, Type::Result(..));
                                self.mains.insert(mi, (id, !f.params.is_empty(), result));
                            }
                        }
                    }
                    Item::Struct(s) => {
                        for f in &s.methods {
                            self.function(f)?;
                        }
                    }
                    Item::Enum(e) => {
                        for f in &e.methods {
                            self.function(f)?;
                        }
                    }
                    Item::Builtin(b) => {
                        for f in &b.methods {
                            self.function(f)?;
                        }
                    }
                    Item::Test(t)
                        if matches!(
                            m.role,
                            ModuleRole::Entry | ModuleRole::Src | ModuleRole::Test
                        ) =>
                    {
                        let id = self.new_func();
                        let mut fc = FnC::new(format!("test {}", t.name));
                        fc.block(self, &t.body)?;
                        fc.emit(Op::Return);
                        let chunk = fc.finish(0, 0);
                        self.set_func(id, Func::Code(chunk));
                        tests.push((
                            m.file.clone(),
                            TestFn {
                                name: format!("{}: {}", m.file, t.name),
                                func: id,
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }
        // Sorted by file; the sort is stable, so tests of a file keep their order.
        tests.sort_by(|a, b| a.0.cmp(&b.0));
        self.tests = tests.into_iter().map(|(_, t)| t).collect();
        Ok(())
    }

    fn number(&mut self, f: &Function, owner: Option<DefRef>) {
        let id = self.new_func();
        self.ids.insert(f.def, id);
        if let Some(mode) = f.self_mode {
            self.self_modes.insert(f.def, mode);
        }
        if let Some(owner) = owner {
            self.dispatch.insert((owner.0, f.name.clone()), id);
        }
    }

    fn function(&mut self, f: &Function) -> Result<(), Error> {
        let Some(&id) = self.ids.get(&f.def) else {
            return Ok(());
        };
        let info = self.p.def(f.def);
        if let Some(name) = info.and_then(|d| d.intrinsic.clone()) {
            let func = match Intrinsic::from_name(&name) {
                Some(i) => Func::Intrinsic(i),
                None => Func::Missing(format!("the intrinsic `{name}`")),
            };
            self.set_func(id, func);
            return Ok(());
        }
        let Some(body) = &f.body else {
            self.set_func(
                id,
                Func::Missing(format!("the function `{}` without a body", f.name)),
            );
            return Ok(());
        };
        let mut fc = FnC::new(f.name.clone());
        let mut params = 0;
        if let Some(mode) = f.self_mode {
            let s = fc.slot(Var::SelfValue);
            if mode == Mode::Mut {
                fc.refs.insert(s);
            }
            params += 1;
        }
        for prm in &f.params {
            let s = fc.slot(Var::Local(prm.local));
            if prm.mode == Mode::Mut {
                fc.refs.insert(s);
            }
            params += 1;
        }
        fc.block(self, body)?;
        fc.emit(Op::Return);
        let chunk = fc.finish(params, 0);
        self.set_func(id, Func::Code(chunk));
        Ok(())
    }

    fn func_id(&self, def: DefRef) -> Result<u32, Error> {
        self.ids
            .get(&def)
            .copied()
            .ok_or_else(|| format!("no function for declaration {}", def.0))
    }

    fn field_index(&self, ty: &Type, name: &str) -> Result<u32, Error> {
        let Type::Named(def, _) = ty else {
            return Err(format!(
                "the field `{name}` of a value that is not a struct"
            ));
        };
        self.fields
            .get(def)
            .and_then(|f| f.iter().position(|n| n == name))
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| format!("no field `{name}`"))
    }
}

/// A loop being compiled.
struct Loop {
    label: Option<String>,
    /// The operand depth at its start.
    depth: u32,
    /// Where the value of a `loop` expression goes.
    result: Option<u32>,
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

/// Where a part of a matched value is: a slot and a path of fields.
#[derive(Clone)]
struct Access {
    root: u32,
    /// The slot holds a reference (`match mut`).
    by_ref: bool,
    path: Vec<u32>,
}

/// The state of one function being compiled.
struct FnC {
    name: String,
    code: Vec<Op>,
    consts: Vec<Value>,
    slots: HashMap<Var, u32>,
    /// Bindings of an alternative of an or-pattern that share the slot of the first alternative.
    alias: HashMap<LocalRef, LocalRef>,
    /// Slots that hold references.
    refs: HashSet<u32>,
    count: u32,
    depth: u32,
    loops: Vec<Loop>,
}

fn effect(op: &Op, depth: u32) -> u32 {
    let d = i64::from(depth);
    let n = |x: u32| i64::from(x);
    let after = match op {
        Op::Const(_) | Op::Unit | Op::Int(_) | Op::Bool(_) | Op::Load(_) | Op::LoadRef(_) => d + 1,
        Op::RefSlot(_) => d + 1,
        Op::Store(_) | Op::StoreRef(_) | Op::Pop => d - 1,
        Op::PopTo(k) => n(*k),
        Op::RefField(_) | Op::Field(_) | Op::Copy | Op::Unary(_) | Op::IsTag(_) => d,
        Op::RefIndex | Op::RefKey | Op::Index | Op::IndexKey | Op::Binary(_) => d - 1,
        Op::SetField(_) => d - 2,
        Op::SetIndex | Op::SetKey => d - 3,
        Op::Slice { from, to, .. } => d - i64::from(*from) - i64::from(*to),
        Op::Make { n: k, .. } | Op::List(k) | Op::Closure { n: k, .. } | Op::Concat(k) => {
            d + 1 - n(*k)
        }
        // `Update` pops the base and the written fields; their number is in the constant, so the
        // compiler sets the depth itself.
        Op::Update { .. } => d,
        Op::Map(k) => d + 1 - 2 * n(*k),
        Op::Jump(_) | Op::Fail(_) | Op::Unreachable => d,
        Op::JumpIf(_) | Op::JumpUnless(_) | Op::Return | Op::Panic(_) => d - 1,
        Op::Call { argc, .. } | Op::CallMethod { argc, .. } => d + 1 - n(*argc),
        Op::CallValue(argc) => d - n(*argc),
    };
    u32::try_from(after.max(0)).unwrap_or(0)
}

fn count(n: usize) -> Result<u32, Error> {
    u32::try_from(n).map_err(|_| "too many values".to_string())
}

impl FnC {
    fn new(name: String) -> FnC {
        FnC {
            name,
            code: Vec::new(),
            consts: Vec::new(),
            slots: HashMap::new(),
            alias: HashMap::new(),
            refs: HashSet::new(),
            count: 0,
            depth: 0,
            loops: Vec::new(),
        }
    }

    fn finish(self, params: u32, captures: u32) -> Chunk {
        Chunk {
            name: self.name,
            params,
            captures,
            slots: self.count,
            code: self.code,
            consts: self.consts,
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        self.depth = effect(&op, self.depth);
        self.code.push(op);
        self.code.len() - 1
    }

    fn here(&self) -> u32 {
        u32::try_from(self.code.len()).unwrap_or(u32::MAX)
    }

    fn patch(&mut self, at: usize) {
        let to = self.here();
        if let Some(Op::Jump(t) | Op::JumpIf(t) | Op::JumpUnless(t)) = self.code.get_mut(at) {
            *t = to;
        }
    }

    fn constant(&mut self, v: Value) -> u32 {
        self.consts.push(v);
        u32::try_from(self.consts.len() - 1).unwrap_or(u32::MAX)
    }

    fn temp(&mut self) -> u32 {
        self.count += 1;
        self.count - 1
    }

    fn slot(&mut self, v: Var) -> u32 {
        let v = match v {
            Var::Local(l) => Var::Local(self.alias.get(&l).copied().unwrap_or(l)),
            other => other,
        };
        if let Some(&s) = self.slots.get(&v) {
            return s;
        }
        let s = self.temp();
        self.slots.insert(v, s);
        s
    }

    fn load_var(&mut self, v: Var) {
        let s = self.slot(v);
        if self.refs.contains(&s) {
            self.emit(Op::LoadRef(s));
        } else {
            self.emit(Op::Load(s));
        }
    }

    fn store_var(&mut self, v: Var) {
        let s = self.slot(v);
        if self.refs.contains(&s) {
            self.emit(Op::StoreRef(s));
        } else {
            self.emit(Op::Store(s));
        }
    }

    // ----- blocks and statements --------------------------------------------------------------

    fn block(&mut self, c: &mut Compiler<'_>, b: &Block) -> Result<(), Error> {
        for s in &b.stmts {
            self.stmt(c, s)?;
        }
        match &b.tail {
            Some(e) => self.expr(c, e),
            None => {
                self.emit(Op::Unit);
                Ok(())
            }
        }
    }

    fn stmt(&mut self, c: &mut Compiler<'_>, s: &Stmt) -> Result<(), Error> {
        let start = self.depth;
        match s {
            Stmt::Let { local, value, .. } => {
                self.expr(c, value)?;
                let slot = self.slot(Var::Local(*local));
                self.emit(Op::Store(slot));
            }
            Stmt::LetPattern { pattern, value, .. } => {
                self.expr(c, value)?;
                let t = self.temp();
                self.emit(Op::Store(t));
                let access = Access {
                    root: t,
                    by_ref: false,
                    path: Vec::new(),
                };
                let mut fails = Vec::new();
                self.pattern(c, pattern, &access, false, &mut fails)?;
                // The pattern always matches; a failure would be a bug of the type checker.
                if !fails.is_empty() {
                    let skip = self.emit(Op::Jump(0));
                    for f in fails {
                        self.patch(f);
                    }
                    self.emit(Op::Unreachable);
                    self.patch(skip);
                }
            }
            Stmt::Assign { target, op, value } => self.assign(c, target, *op, value)?,
            Stmt::While { label, cond, body } => {
                let top = self.here();
                self.loops.push(Loop {
                    label: label.clone(),
                    depth: start,
                    result: None,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                self.expr(c, cond)?;
                let exit = self.emit(Op::JumpUnless(0));
                self.block(c, body)?;
                self.emit(Op::Pop);
                self.emit(Op::Jump(top));
                self.patch(exit);
                self.end_loop(top)?;
            }
            Stmt::ForList {
                label,
                var,
                mode,
                list,
                body,
            } => {
                self.expr(c, list)?;
                let items = self.temp();
                self.emit(Op::Store(items));
                let i = self.temp();
                self.emit(Op::Int(0));
                self.emit(Op::Store(i));
                let top = self.here();
                self.loops.push(Loop {
                    label: label.clone(),
                    depth: start,
                    result: None,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                // `i < len(items)`
                self.emit(Op::Load(i));
                self.emit(Op::Load(items));
                let len = c.list_len()?;
                self.emit(Op::Call { func: len, argc: 1 });
                self.emit(Op::Binary(BinOp::Lt));
                let exit = self.emit(Op::JumpUnless(0));
                let v = self.slot(Var::Local(*var));
                self.emit(Op::Load(items));
                self.emit(Op::Load(i));
                if *mode == Mode::Mut {
                    self.refs.insert(v);
                    self.emit(Op::RefIndex);
                } else {
                    self.emit(Op::Index);
                }
                self.emit(Op::Store(v));
                self.block(c, body)?;
                self.emit(Op::Pop);
                let cont = self.here();
                self.emit(Op::Load(i));
                self.emit(Op::Int(1));
                self.emit(Op::Binary(BinOp::Add));
                self.emit(Op::Store(i));
                self.emit(Op::Jump(top));
                self.patch(exit);
                self.end_loop(cont)?;
            }
            Stmt::ForRange {
                label,
                var,
                start: from,
                end,
                inclusive,
                body,
            } => {
                let i = self.slot(Var::Local(*var));
                let n = self.temp();
                self.expr(c, from)?;
                let cur = self.temp();
                self.emit(Op::Store(cur));
                self.expr(c, end)?;
                self.emit(Op::Store(n));
                let top = self.here();
                self.loops.push(Loop {
                    label: label.clone(),
                    depth: start,
                    result: None,
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                self.emit(Op::Load(cur));
                self.emit(Op::Load(n));
                self.emit(Op::Binary(if *inclusive { BinOp::Le } else { BinOp::Lt }));
                let exit = self.emit(Op::JumpUnless(0));
                self.emit(Op::Load(cur));
                self.emit(Op::Store(i));
                self.block(c, body)?;
                self.emit(Op::Pop);
                let cont = self.here();
                // An inclusive range ends after its last value, before the increment could
                // overflow.
                let mut last = None;
                if *inclusive {
                    self.emit(Op::Load(cur));
                    self.emit(Op::Load(n));
                    self.emit(Op::Binary(BinOp::Eq));
                    last = Some(self.emit(Op::JumpIf(0)));
                }
                self.emit(Op::Load(cur));
                self.emit(Op::Int(1));
                self.emit(Op::Binary(BinOp::Add));
                self.emit(Op::Store(cur));
                self.emit(Op::Jump(top));
                self.patch(exit);
                if let Some(l) = last {
                    self.patch(l);
                }
                self.end_loop(cont)?;
            }
            Stmt::Expr(e) => {
                self.expr(c, e)?;
                self.emit(Op::Pop);
            }
        }
        self.depth = start;
        Ok(())
    }

    /// Ends the innermost loop: `continue` goes to `cont`, `break` to here.
    fn end_loop(&mut self, cont: u32) -> Result<(), Error> {
        let l = self.loops.pop().ok_or("no loop to end")?;
        for b in l.breaks {
            self.patch(b);
        }
        for at in l.continues {
            if let Some(Op::Jump(t)) = self.code.get_mut(at) {
                *t = cont;
            }
        }
        Ok(())
    }

    fn assign(
        &mut self,
        c: &mut Compiler<'_>,
        target: &Expr,
        op: Option<BinaryOp>,
        value: &Expr,
    ) -> Result<(), Error> {
        match &target.kind {
            ExprKind::Local(_) | ExprKind::SelfValue => {
                let var = match &target.kind {
                    ExprKind::Local(l) => Var::Local(*l),
                    _ => Var::SelfValue,
                };
                if let Some(op) = op {
                    self.load_var(var);
                    self.expr(c, value)?;
                    self.binary(op, &target.ty)?;
                } else {
                    self.expr(c, value)?;
                }
                self.store_var(var);
            }
            ExprKind::Field { base, name } => {
                let i = c.field_index(&base.ty, name)?;
                self.expr(c, base)?;
                if let Some(op) = op {
                    let t = self.temp();
                    self.emit(Op::Store(t));
                    self.emit(Op::Load(t));
                    self.emit(Op::Load(t));
                    self.emit(Op::Field(i));
                    self.expr(c, value)?;
                    self.binary(op, &target.ty)?;
                } else {
                    self.expr(c, value)?;
                }
                self.emit(Op::SetField(i));
            }
            ExprKind::Index { base, index } => {
                let map = matches!(base.ty, Type::Map(..));
                self.expr(c, base)?;
                self.expr(c, index)?;
                if let Some(op) = op {
                    let (tb, ti) = (self.temp(), self.temp());
                    self.emit(Op::Store(ti));
                    self.emit(Op::Store(tb));
                    self.emit(Op::Load(tb));
                    self.emit(Op::Load(ti));
                    self.emit(Op::Load(tb));
                    self.emit(Op::Load(ti));
                    self.emit(if map { Op::IndexKey } else { Op::Index });
                    self.expr(c, value)?;
                    self.binary(op, &target.ty)?;
                } else {
                    self.expr(c, value)?;
                }
                self.emit(if map { Op::SetKey } else { Op::SetIndex });
            }
            _ => return Err("an assignment to something that is not a place".to_string()),
        }
        Ok(())
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr(&mut self, c: &mut Compiler<'_>, e: &Expr) -> Result<(), Error> {
        let start = self.depth;
        self.expr_kind(c, e)?;
        if e.ty == Type::Never {
            self.depth = start + 1;
        } else if self.depth != start + 1 {
            return Err(format!(
                "the stack is unbalanced after an expression in `{}`",
                self.name
            ));
        }
        Ok(())
    }

    fn expr_kind(&mut self, c: &mut Compiler<'_>, e: &Expr) -> Result<(), Error> {
        match &e.kind {
            ExprKind::Int(i) => {
                self.emit(Op::Int(*i));
            }
            ExprKind::Float(f) => {
                let k = self.constant(Value::Float(*f));
                self.emit(Op::Const(k));
            }
            ExprKind::Str(s) => {
                let k = self.constant(Value::str(s));
                self.emit(Op::Const(k));
            }
            ExprKind::Bool(b) => {
                self.emit(Op::Bool(*b));
            }
            ExprKind::Unit => {
                self.emit(Op::Unit);
            }
            ExprKind::Interpolate(parts) => {
                for p in parts {
                    self.expr(c, p)?;
                }
                self.emit(Op::Concat(count(parts.len())?));
            }
            ExprKind::Local(l) => self.load_var(Var::Local(*l)),
            ExprKind::SelfValue => self.load_var(Var::SelfValue),
            ExprKind::FnValue(def) => {
                let func = c.func_id(*def)?;
                self.emit(Op::Closure { func, n: 0 });
            }
            ExprKind::Const(def) => {
                let v = c
                    .consts
                    .get(def)
                    .cloned()
                    .ok_or_else(|| format!("no value for constant {}", def.0))?;
                let k = self.constant(v);
                self.emit(Op::Const(k));
            }
            ExprKind::Call { callee, args } => self.call(c, callee, args)?,
            ExprKind::Field { base, name } => {
                let i = c.field_index(&base.ty, name)?;
                self.expr(c, base)?;
                self.emit(Op::Field(i));
            }
            ExprKind::Index { base, index } => {
                self.expr(c, base)?;
                self.expr(c, index)?;
                self.emit(if matches!(base.ty, Type::Map(..)) {
                    Op::IndexKey
                } else {
                    Op::Index
                });
            }
            ExprKind::Slice {
                base,
                start,
                end,
                inclusive,
            } => {
                self.expr(c, base)?;
                if let Some(s) = start {
                    self.expr(c, s)?;
                }
                if let Some(t) = end {
                    self.expr(c, t)?;
                }
                self.emit(Op::Slice {
                    from: start.is_some(),
                    to: end.is_some(),
                    inclusive: *inclusive,
                    string: base.ty == Type::String,
                });
            }
            ExprKind::Struct { def, fields, base } => match base {
                None => {
                    for (_, v) in fields {
                        self.expr(c, v)?;
                    }
                    self.emit(Op::Make {
                        tag: def.0,
                        n: count(fields.len())?,
                    });
                }
                Some(b) => {
                    self.expr(c, b)?;
                    let names = c.fields.get(def).cloned().unwrap_or_default();
                    let mut positions = Vec::new();
                    for (name, v) in fields {
                        let i = names
                            .iter()
                            .position(|n| n == name)
                            .ok_or_else(|| format!("no field `{name}`"))?;
                        positions.push(Value::Int(i64::try_from(i).unwrap_or(0)));
                        self.expr(c, v)?;
                    }
                    let start = self.depth;
                    let k = self.constant(Value::list(positions));
                    self.emit(Op::Update {
                        tag: def.0,
                        fields: k,
                    });
                    self.depth = start.saturating_sub(count(fields.len())?);
                }
            },
            ExprKind::Variant { def, values } => {
                for v in values {
                    self.expr(c, v)?;
                }
                self.emit(Op::Make {
                    tag: def.0,
                    n: count(values.len())?,
                });
            }
            ExprKind::Some(v) | ExprKind::Ok(v) | ExprKind::Err(v) => {
                self.expr(c, v)?;
                let tag = match &e.kind {
                    ExprKind::Some(_) => c.tags.some,
                    ExprKind::Ok(_) => c.tags.ok,
                    _ => c.tags.err,
                };
                self.emit(Op::Make { tag, n: 1 });
            }
            ExprKind::None => {
                self.emit(Op::Make {
                    tag: c.tags.none,
                    n: 0,
                });
            }
            ExprKind::List(items) => {
                for i in items {
                    self.expr(c, i)?;
                }
                self.emit(Op::List(count(items.len())?));
            }
            ExprKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.expr(c, k)?;
                    self.expr(c, v)?;
                }
                self.emit(Op::Map(count(pairs.len())?));
            }
            ExprKind::Unary { op, operand } => {
                self.expr(c, operand)?;
                let u = match op {
                    UnaryOp::Neg if operand.ty == Type::Float => UnOp::FNeg,
                    UnaryOp::Neg => UnOp::Neg,
                    UnaryOp::Not => UnOp::Not,
                    UnaryOp::BitNot => UnOp::BitNot,
                };
                self.emit(Op::Unary(u));
            }
            ExprKind::Binary { op, lhs, rhs } => match op {
                BinaryOp::And | BinaryOp::Or => {
                    self.expr(c, lhs)?;
                    let start = self.depth;
                    let short = if *op == BinaryOp::And {
                        self.emit(Op::JumpUnless(0))
                    } else {
                        self.emit(Op::JumpIf(0))
                    };
                    self.expr(c, rhs)?;
                    let end = self.emit(Op::Jump(0));
                    self.patch(short);
                    self.depth = start - 1;
                    self.emit(Op::Bool(*op == BinaryOp::Or));
                    self.patch(end);
                }
                _ => {
                    self.expr(c, lhs)?;
                    self.expr(c, rhs)?;
                    self.binary(*op, &lhs.ty)?;
                }
            },
            ExprKind::Closure { params, body } => self.closure(c, params, body)?,
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(c, cond)?;
                let start = self.depth;
                let to_else = self.emit(Op::JumpUnless(0));
                self.block(c, then)?;
                let to_end = self.emit(Op::Jump(0));
                self.patch(to_else);
                self.depth = start - 1;
                match otherwise {
                    Some(b) => self.block(c, b)?,
                    None => {
                        self.emit(Op::Unit);
                    }
                }
                self.patch(to_end);
                self.depth = start;
            }
            ExprKind::Match {
                scrutinee,
                mode,
                arms,
            } => {
                let by_ref = *mode == Mode::Mut;
                let t = self.temp();
                if by_ref {
                    self.place_ref(c, scrutinee)?;
                    self.refs.insert(t);
                } else {
                    self.expr(c, scrutinee)?;
                }
                self.emit(Op::Store(t));
                let start = self.depth;
                let access = Access {
                    root: t,
                    by_ref,
                    path: Vec::new(),
                };
                let mut ends = Vec::new();
                for arm in arms {
                    self.depth = start;
                    let mut fails = Vec::new();
                    self.pattern(c, &arm.pattern, &access, by_ref, &mut fails)?;
                    if let Some(g) = &arm.guard {
                        self.expr(c, g)?;
                        fails.push(self.emit(Op::JumpUnless(0)));
                    }
                    self.block(c, &arm.body)?;
                    ends.push(self.emit(Op::Jump(0)));
                    for f in fails {
                        self.patch(f);
                    }
                }
                self.depth = start;
                self.emit(Op::Unreachable);
                for end in ends {
                    self.patch(end);
                }
                self.depth = start + 1;
            }
            ExprKind::Block(b) => self.block(c, b)?,
            ExprKind::Return(v) => {
                match v {
                    Some(v) => self.expr(c, v)?,
                    None => {
                        self.emit(Op::Unit);
                    }
                }
                self.emit(Op::Return);
            }
            ExprKind::Loop { label, body } => {
                let start = self.depth;
                let result = self.temp();
                self.emit(Op::Unit);
                self.emit(Op::Store(result));
                let top = self.here();
                self.loops.push(Loop {
                    label: label.clone(),
                    depth: start,
                    result: Some(result),
                    breaks: Vec::new(),
                    continues: Vec::new(),
                });
                self.block(c, body)?;
                self.emit(Op::Pop);
                self.emit(Op::Jump(top));
                self.end_loop(top)?;
                self.depth = start;
                self.emit(Op::Load(result));
            }
            ExprKind::Break { label, value } => {
                let i = self.find_loop(label.as_deref())?;
                let (depth, result) = self
                    .loops
                    .get(i)
                    .map(|l| (l.depth, l.result))
                    .ok_or("no loop")?;
                if let Some(r) = result {
                    match value {
                        Some(v) => self.expr(c, v)?,
                        None => {
                            self.emit(Op::Unit);
                        }
                    }
                    self.emit(Op::Store(r));
                }
                self.emit(Op::PopTo(depth));
                let j = self.emit(Op::Jump(0));
                if let Some(l) = self.loops.get_mut(i) {
                    l.breaks.push(j);
                }
            }
            ExprKind::Continue { label } => {
                let i = self.find_loop(label.as_deref())?;
                let depth = self.loops.get(i).map(|l| l.depth).ok_or("no loop")?;
                self.emit(Op::PopTo(depth));
                let j = self.emit(Op::Jump(0));
                if let Some(l) = self.loops.get_mut(i) {
                    l.continues.push(j);
                }
            }
            ExprKind::Try { value, .. } => {
                let option = matches!(value.ty, Type::Option(_));
                self.expr(c, value)?;
                let t = self.temp();
                self.emit(Op::Store(t));
                self.emit(Op::Load(t));
                self.emit(Op::IsTag(if option { c.tags.none } else { c.tags.err }));
                let ok = self.emit(Op::JumpUnless(0));
                self.emit(Op::Load(t));
                self.emit(Op::Return);
                self.patch(ok);
                self.emit(Op::Load(t));
                self.emit(Op::Field(0));
            }
            ExprKind::Coerce { value, .. } => self.expr(c, value)?,
            ExprKind::Copy(v) => {
                self.expr(c, v)?;
                self.emit(Op::Copy);
            }
            ExprKind::Panic { message, location } => {
                self.expr(c, message)?;
                let k = self.constant(Value::str(location));
                self.emit(Op::Panic(k));
            }
            ExprKind::Assert {
                cond,
                text,
                location,
            } => {
                self.expr(c, cond)?;
                let ok = self.emit(Op::JumpIf(0));
                let k = self.constant(Value::str(&format!(
                    "panic at {location}: assertion failed: {text}"
                )));
                self.emit(Op::Fail(k));
                self.patch(ok);
                self.emit(Op::Unit);
            }
        }
        Ok(())
    }

    fn find_loop(&self, label: Option<&str>) -> Result<usize, Error> {
        let found = match label {
            None => self.loops.len().checked_sub(1),
            Some(name) => self
                .loops
                .iter()
                .rposition(|l| l.label.as_deref() == Some(name)),
        };
        found.ok_or_else(|| "a `break` or `continue` outside a loop".to_string())
    }

    fn binary(&mut self, op: BinaryOp, ty: &Type) -> Result<(), Error> {
        let float = *ty == Type::Float;
        let b = match op {
            BinaryOp::Add if *ty == Type::String => BinOp::Concat,
            BinaryOp::Add if float => BinOp::FAdd,
            BinaryOp::Sub if float => BinOp::FSub,
            BinaryOp::Mul if float => BinOp::FMul,
            BinaryOp::Div if float => BinOp::FDiv,
            BinaryOp::Rem if float => BinOp::FRem,
            BinaryOp::Add => BinOp::Add,
            BinaryOp::Sub => BinOp::Sub,
            BinaryOp::Mul => BinOp::Mul,
            BinaryOp::Div => BinOp::Div,
            BinaryOp::Rem => BinOp::Rem,
            BinaryOp::Shl => BinOp::Shl,
            BinaryOp::Shr => BinOp::Shr,
            BinaryOp::BitAnd => BinOp::BitAnd,
            BinaryOp::BitOr => BinOp::BitOr,
            BinaryOp::BitXor => BinOp::BitXor,
            BinaryOp::Eq => BinOp::Eq,
            BinaryOp::NotEq => BinOp::Ne,
            BinaryOp::Lt => BinOp::Lt,
            BinaryOp::Le => BinOp::Le,
            BinaryOp::Gt => BinOp::Gt,
            BinaryOp::Ge => BinOp::Ge,
            BinaryOp::And | BinaryOp::Or => {
                return Err("`&&` or `||` as a compound assignment".to_string());
            }
        };
        self.emit(Op::Binary(b));
        Ok(())
    }

    /// Pushes a reference to the place of `e`; a temporary gets a slot of its own.
    fn place_ref(&mut self, c: &mut Compiler<'_>, e: &Expr) -> Result<(), Error> {
        match &e.kind {
            ExprKind::Local(l) => {
                let s = self.slot(Var::Local(*l));
                self.emit(Op::RefSlot(s));
            }
            ExprKind::SelfValue => {
                let s = self.slot(Var::SelfValue);
                self.emit(Op::RefSlot(s));
            }
            ExprKind::Field { base, name } => {
                let i = c.field_index(&base.ty, name)?;
                self.expr(c, base)?;
                self.emit(Op::RefField(i));
            }
            ExprKind::Index { base, index } => {
                self.expr(c, base)?;
                self.expr(c, index)?;
                self.emit(if matches!(base.ty, Type::Map(..)) {
                    Op::RefKey
                } else {
                    Op::RefIndex
                });
            }
            _ => {
                self.expr(c, e)?;
                let t = self.temp();
                self.emit(Op::Store(t));
                self.emit(Op::RefSlot(t));
            }
        }
        Ok(())
    }

    fn args(&mut self, c: &mut Compiler<'_>, args: &[Arg]) -> Result<(), Error> {
        for a in args {
            if a.mode == Mode::Mut {
                self.place_ref(c, &a.value)?;
            } else {
                self.expr(c, &a.value)?;
            }
        }
        Ok(())
    }

    fn call(&mut self, c: &mut Compiler<'_>, callee: &Callee, args: &[Arg]) -> Result<(), Error> {
        match callee {
            Callee::Fn(def) => {
                let func = c.func_id(*def)?;
                self.args(c, args)?;
                self.emit(Op::Call {
                    func,
                    argc: count(args.len())?,
                });
            }
            Callee::Method { method, recv } => {
                let func = c.func_id(*method)?;
                let mode = c.self_modes.get(method).copied().unwrap_or(Mode::Read);
                if mode == Mode::Mut {
                    self.place_ref(c, recv)?;
                } else {
                    self.expr(c, recv)?;
                }
                self.args(c, args)?;
                self.emit(Op::Call {
                    func,
                    argc: count(args.len() + 1)?,
                });
            }
            Callee::Interface {
                interface,
                name,
                recv,
            } => {
                let mode = c
                    .iface_modes
                    .get(&(*interface, name.clone()))
                    .copied()
                    .unwrap_or(Mode::Read);
                if mode == Mode::Mut {
                    self.place_ref(c, recv)?;
                } else {
                    self.expr(c, recv)?;
                }
                self.args(c, args)?;
                let k = self.constant(Value::str(name));
                self.emit(Op::CallMethod {
                    name: k,
                    argc: count(args.len() + 1)?,
                });
            }
            Callee::Value(f) => {
                self.expr(c, f)?;
                self.args(c, args)?;
                self.emit(Op::CallValue(count(args.len())?));
            }
        }
        Ok(())
    }

    fn closure(
        &mut self,
        c: &mut Compiler<'_>,
        params: &[wolf_ir::Param],
        body: &Block,
    ) -> Result<(), Error> {
        let mut used = BTreeSet::new();
        let mut bound = BTreeSet::new();
        for p in params {
            bound.insert(Var::Local(p.local));
        }
        walk_block(body, &mut used, &mut bound);
        let captures: Vec<Var> = used.difference(&bound).copied().collect();
        let id = c.new_func();
        let mut fc = FnC::new(format!("{} closure", self.name));
        for p in params {
            let s = fc.slot(Var::Local(p.local));
            if p.mode == Mode::Mut {
                fc.refs.insert(s);
            }
        }
        for v in &captures {
            fc.slot(*v);
        }
        fc.block(c, body)?;
        fc.emit(Op::Return);
        let chunk = fc.finish(count(params.len())?, count(captures.len())?);
        c.set_func(id, Func::Code(chunk));
        for v in &captures {
            self.load_var(*v);
        }
        self.emit(Op::Closure {
            func: id,
            n: count(captures.len())?,
        });
        Ok(())
    }

    // ----- patterns ---------------------------------------------------------------------------

    fn load_access(&mut self, a: &Access) {
        if a.by_ref {
            self.emit(Op::LoadRef(a.root));
        } else {
            self.emit(Op::Load(a.root));
        }
        for &f in &a.path {
            self.emit(Op::Field(f));
        }
    }

    /// Tests `pat` against the value at `a` and binds its names; a failed test jumps to one of
    /// `fails`. With `by_ref` (`match mut`), names are bound to references into the value.
    fn pattern(
        &mut self,
        c: &mut Compiler<'_>,
        pat: &Pattern,
        a: &Access,
        by_ref: bool,
        fails: &mut Vec<usize>,
    ) -> Result<(), Error> {
        let sub = |a: &Access, i: usize| {
            let mut s = a.clone();
            s.path.push(u32::try_from(i).unwrap_or(0));
            s
        };
        match pat {
            Pattern::Wild => {}
            Pattern::Bind(l) => {
                let slot = self.slot(Var::Local(*l));
                if by_ref {
                    match a.path.split_last() {
                        None => {
                            self.emit(Op::RefSlot(a.root));
                        }
                        Some((last, init)) => {
                            let parent = Access {
                                root: a.root,
                                by_ref: a.by_ref,
                                path: init.to_vec(),
                            };
                            self.load_access(&parent);
                            self.emit(Op::RefField(*last));
                        }
                    }
                    self.refs.insert(slot);
                } else {
                    self.load_access(a);
                }
                self.emit(Op::Store(slot));
            }
            Pattern::Int(i) => {
                self.load_access(a);
                self.emit(Op::Int(*i));
                self.emit(Op::Binary(BinOp::Eq));
                fails.push(self.emit(Op::JumpUnless(0)));
            }
            Pattern::Str(s) => {
                self.load_access(a);
                let k = self.constant(Value::str(s));
                self.emit(Op::Const(k));
                self.emit(Op::Binary(BinOp::Eq));
                fails.push(self.emit(Op::JumpUnless(0)));
            }
            Pattern::Bool(b) => {
                self.load_access(a);
                self.emit(Op::Bool(*b));
                self.emit(Op::Binary(BinOp::Eq));
                fails.push(self.emit(Op::JumpUnless(0)));
            }
            Pattern::Const(def) => {
                let v = c
                    .consts
                    .get(def)
                    .cloned()
                    .ok_or_else(|| format!("no value for constant {}", def.0))?;
                self.load_access(a);
                let k = self.constant(v);
                self.emit(Op::Const(k));
                self.emit(Op::Binary(BinOp::Eq));
                fails.push(self.emit(Op::JumpUnless(0)));
            }
            Pattern::Variant { def, fields } => {
                self.tag(a, def.0, fails);
                for (i, f) in fields.iter().enumerate() {
                    self.pattern(c, f, &sub(a, i), by_ref, fails)?;
                }
            }
            Pattern::Some(p) | Pattern::Ok(p) | Pattern::Err(p) => {
                let tag = match pat {
                    Pattern::Some(_) => c.tags.some,
                    Pattern::Ok(_) => c.tags.ok,
                    _ => c.tags.err,
                };
                self.tag(a, tag, fails);
                self.pattern(c, p, &sub(a, 0), by_ref, fails)?;
            }
            Pattern::None => self.tag(a, c.tags.none, fails),
            Pattern::Struct { fields, .. } => {
                for (i, f) in fields.iter().enumerate() {
                    self.pattern(c, f, &sub(a, i), by_ref, fails)?;
                }
            }
            Pattern::Or(alts) => {
                // The names of every alternative share the slots of the first one.
                if let Some(first) = alts.first() {
                    let mut names = HashMap::new();
                    binds(first, &mut |l| {
                        if let Some(info) = c.p.locals.get(&l) {
                            names.insert(info.name.clone(), l);
                        }
                    });
                    for alt in alts.iter().skip(1) {
                        binds(alt, &mut |l| {
                            if let Some(first) = c.p.locals.get(&l).and_then(|i| names.get(&i.name))
                            {
                                self.alias.insert(l, *first);
                            }
                        });
                    }
                }
                let mut matched = Vec::new();
                let n = alts.len();
                for (k, alt) in alts.iter().enumerate() {
                    if k + 1 == n {
                        self.pattern(c, alt, a, by_ref, fails)?;
                    } else {
                        let mut local = Vec::new();
                        self.pattern(c, alt, a, by_ref, &mut local)?;
                        matched.push(self.emit(Op::Jump(0)));
                        for f in local {
                            self.patch(f);
                        }
                    }
                }
                for m in matched {
                    self.patch(m);
                }
            }
        }
        Ok(())
    }

    fn tag(&mut self, a: &Access, tag: u32, fails: &mut Vec<usize>) {
        self.load_access(a);
        self.emit(Op::IsTag(tag));
        fails.push(self.emit(Op::JumpUnless(0)));
    }
}

impl Compiler<'_> {
    /// The function `List.len`, used by `for` loops.
    fn list_len(&self) -> Result<u32, Error> {
        let def = self
            .p
            .defs
            .iter()
            .find(|(_, d)| d.intrinsic.as_deref() == Some("std.list.List.len"))
            .map(|(r, _)| *r)
            .ok_or("std has no `List.len`")?;
        self.func_id(def)
    }
}

/// Calls `f` with every local that a pattern binds.
fn binds(p: &Pattern, f: &mut impl FnMut(LocalRef)) {
    match p {
        Pattern::Bind(l) => f(*l),
        Pattern::Variant { fields, .. } | Pattern::Struct { fields, .. } => {
            for x in fields {
                binds(x, f);
            }
        }
        Pattern::Some(x) | Pattern::Ok(x) | Pattern::Err(x) => binds(x, f),
        Pattern::Or(alts) => {
            for x in alts {
                binds(x, f);
            }
        }
        _ => {}
    }
}

// ----- the variables a closure uses ---------------------------------------------------------------

fn walk_block(b: &Block, used: &mut BTreeSet<Var>, bound: &mut BTreeSet<Var>) {
    for s in &b.stmts {
        walk_stmt(s, used, bound);
    }
    if let Some(t) = &b.tail {
        walk_expr(t, used, bound);
    }
}

fn walk_stmt(s: &Stmt, used: &mut BTreeSet<Var>, bound: &mut BTreeSet<Var>) {
    match s {
        Stmt::Let { local, value, .. } => {
            bound.insert(Var::Local(*local));
            walk_expr(value, used, bound);
        }
        Stmt::LetPattern { pattern, value, .. } => {
            binds(pattern, &mut |l| {
                bound.insert(Var::Local(l));
            });
            walk_expr(value, used, bound);
        }
        Stmt::Assign { target, value, .. } => {
            walk_expr(target, used, bound);
            walk_expr(value, used, bound);
        }
        Stmt::While { cond, body, .. } => {
            walk_expr(cond, used, bound);
            walk_block(body, used, bound);
        }
        Stmt::ForList {
            var, list, body, ..
        } => {
            bound.insert(Var::Local(*var));
            walk_expr(list, used, bound);
            walk_block(body, used, bound);
        }
        Stmt::ForRange {
            var,
            start,
            end,
            body,
            ..
        } => {
            bound.insert(Var::Local(*var));
            walk_expr(start, used, bound);
            walk_expr(end, used, bound);
            walk_block(body, used, bound);
        }
        Stmt::Expr(e) => walk_expr(e, used, bound),
    }
}

fn walk_expr(e: &Expr, used: &mut BTreeSet<Var>, bound: &mut BTreeSet<Var>) {
    let mut go = |x: &Expr| walk_expr(x, used, bound);
    match &e.kind {
        ExprKind::Local(l) => {
            used.insert(Var::Local(*l));
        }
        ExprKind::SelfValue => {
            used.insert(Var::SelfValue);
        }
        ExprKind::Interpolate(parts) | ExprKind::List(parts) => parts.iter().for_each(go),
        ExprKind::Call { callee, args } => {
            match callee {
                Callee::Fn(_) => {}
                Callee::Method { recv, .. } | Callee::Interface { recv, .. } => go(recv),
                Callee::Value(f) => go(f),
            }
            for a in args {
                walk_expr(&a.value, used, bound);
            }
        }
        ExprKind::Field { base, .. } => go(base),
        ExprKind::Index { base, index } => {
            go(base);
            go(index);
        }
        ExprKind::Slice {
            base, start, end, ..
        } => {
            go(base);
            if let Some(s) = start {
                walk_expr(s, used, bound);
            }
            if let Some(t) = end {
                walk_expr(t, used, bound);
            }
        }
        ExprKind::Struct { fields, base, .. } => {
            for (_, v) in fields {
                walk_expr(v, used, bound);
            }
            if let Some(b) = base {
                walk_expr(b, used, bound);
            }
        }
        ExprKind::Variant { values, .. } => values.iter().for_each(go),
        ExprKind::Some(v)
        | ExprKind::Ok(v)
        | ExprKind::Err(v)
        | ExprKind::Copy(v)
        | ExprKind::Coerce { value: v, .. }
        | ExprKind::Try { value: v, .. } => go(v),
        ExprKind::Map(pairs) => {
            for (k, v) in pairs {
                walk_expr(k, used, bound);
                walk_expr(v, used, bound);
            }
        }
        ExprKind::Unary { operand, .. } => go(operand),
        ExprKind::Binary { lhs, rhs, .. } => {
            go(lhs);
            go(rhs);
        }
        ExprKind::Closure { params, body } => {
            for p in params {
                bound.insert(Var::Local(p.local));
            }
            walk_block(body, used, bound);
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            walk_expr(cond, used, bound);
            walk_block(then, used, bound);
            if let Some(b) = otherwise {
                walk_block(b, used, bound);
            }
        }
        ExprKind::Match {
            scrutinee, arms, ..
        } => {
            walk_expr(scrutinee, used, bound);
            for arm in arms {
                binds(&arm.pattern, &mut |l| {
                    bound.insert(Var::Local(l));
                });
                if let Some(g) = &arm.guard {
                    walk_expr(g, used, bound);
                }
                walk_block(&arm.body, used, bound);
            }
        }
        ExprKind::Block(b) | ExprKind::Loop { body: b, .. } => walk_block(b, used, bound),
        ExprKind::Return(v) | ExprKind::Break { value: v, .. } => {
            if let Some(v) = v {
                walk_expr(v, used, bound);
            }
        }
        ExprKind::Panic { message, .. } => go(message),
        ExprKind::Assert { cond, .. } => go(cond),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Unit
        | ExprKind::FnValue(_)
        | ExprKind::Const(_)
        | ExprKind::None
        | ExprKind::Continue { .. } => {}
    }
}
