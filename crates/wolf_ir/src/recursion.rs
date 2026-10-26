//! Which functions can call themselves (spec chapter 8, section 4.3, D8.16).
//!
//! Go and Rust cannot catch their own stack overflow, so the code they generate counts the nested
//! calls of the functions that can call themselves and panics past a limit. Those functions are
//! the ones on a cycle of the call graph. Closures are functions of the graph too.
//!
//! A call that the IR does not resolve to one function counts as a call of every function that
//! could be behind it:
//!
//! - a call of a function value: every closure and every function used as a value whose type fits
//!   the call (a type parameter fits any type), or the one closure that an immutable local holds;
//! - a call of an interface method: every implementation and every default of a method of that
//!   name, and what the implementations of std call (`List.to_string` calls `to_string` and
//!   `message` of its elements);
//! - a call of std or of extern code: the function values it is given, as above, and when std
//!   calls the methods of its bounds (`List.sort` calls `compare`), those methods of the types its
//!   arguments mention.
//!
//! Some calls through type parameters cannot repeat without end, because neither Go nor Rust
//! allows a type to grow from call to call: `Holder<T>.label` that calls `label` of its `T` calls
//! it for a type nested in its own, and `Self` stays the same type. A cycle is counted unless all
//! its calls are of these two kinds and at least one of them goes to a nested type. This holds
//! only while `T` is a type written in the program: when the program uses `Holder<Named>` with an
//! interface, or `Holder<U>` with a type parameter of other code, which may be an interface, the
//! value in `T` may be another `Holder`, as deep as the data, and the call counts as any call.
//!
//! Counting a function that cannot call itself costs a counter and nothing else, so where the
//! graph cannot be precise it says too much rather than too little.

use std::collections::{BTreeMap, BTreeSet};

use crate::*;

/// The node of the bodies of tests: nothing calls them, so it is never on a cycle.
const ROOT: usize = 0;

/// Sets [`Function::recursive`] and the flag of each closure of the program.
pub(crate) fn mark(program: &mut Program) {
    let decls = Decls::new(program);
    let mut first = Walk::new(&decls, None);
    first.modules(&mut program.modules);
    let recursive = recursive(&first.edges());
    let mut second = Walk::new(&decls, Some(&recursive));
    second.modules(&mut program.modules);
}

/// How a call can repeat, from the most to the least dangerous.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    /// Any call: the cycle may repeat without end.
    Any,
    /// A call of a method of `Self`: the same type.
    Same,
    /// A call of a method of a type parameter of the type of the method (`Holder<T>` calling
    /// `T`): a type nested in the type of the caller.
    Inner,
}

/// The types whose implementations of a method a call may reach.
#[derive(Clone, Default)]
struct Reach {
    /// Types that the arguments mention.
    types: BTreeSet<DefRef>,
    /// Any type: an interface value or a type parameter of a function.
    any: bool,
    /// `Self` of a default.
    same: bool,
    /// A type parameter of the type of the method: the type.
    inner: Option<DefRef>,
}

/// What the declarations of the program say, found before the bodies are walked.
#[derive(Default)]
struct Decls {
    /// The node of each function and method with a body, defaults of interfaces aside.
    fns: BTreeMap<DefRef, usize>,
    /// The node of each default of an interface method, by interface and method name.
    defaults: BTreeMap<(DefRef, String), usize>,
    /// The number of nodes of declarations; the nodes of closures come after them.
    declared: usize,
    /// The implementations of interface methods by method name: the type that implements it
    /// (`None` for a default, which any type may get) and the node.
    impls: BTreeMap<String, Vec<(Option<DefRef>, usize)>>,
    /// Functions of std and of extern code, with the interface methods each may call.
    opaque: BTreeMap<DefRef, BTreeSet<String>>,
    /// The interface methods that the implementations in std of a method may call, by name.
    opaque_impls: BTreeMap<String, BTreeSet<String>>,
    /// Methods with neither a body nor code in std, by name.
    signatures: BTreeMap<DefRef, String>,
    /// Every interface: its methods and its super-interfaces.
    interfaces: BTreeMap<DefRef, (Vec<String>, Vec<DefRef>)>,
    /// `Display` of std: the show of std writes an error that is not `Display` by `message()`.
    display: Option<DefRef>,
}

impl Decls {
    fn new(program: &Program) -> Decls {
        let mut d = Decls {
            declared: ROOT + 1,
            ..Decls::default()
        };
        for m in &program.modules {
            for item in &m.items {
                if let Item::Interface(i) = item {
                    let supers = i
                        .supers
                        .iter()
                        .filter_map(|t| match t {
                            Type::Named(s, _) => Some(*s),
                            _ => None,
                        })
                        .collect();
                    let names = i.methods.iter().map(|f| f.name.clone()).collect();
                    d.interfaces.insert(i.def, (names, supers));
                    if m.role == ModuleRole::Std && i.name == "Display" {
                        d.display = Some(i.def);
                    }
                }
            }
        }
        for m in &program.modules {
            for item in &m.items {
                match item {
                    Item::Fn(f) => d.function(f, None),
                    Item::Struct(s) => s.methods.iter().for_each(|f| d.function(f, Some(s.def))),
                    Item::Enum(e) => e.methods.iter().for_each(|f| d.function(f, Some(e.def))),
                    Item::Builtin(b) => b.methods.iter().for_each(|f| d.function(f, Some(b.def))),
                    Item::Interface(i) => {
                        for f in i.methods.iter().filter(|f| f.body.is_some()) {
                            let node = d.node();
                            d.defaults.insert((i.def, f.name.clone()), node);
                            d.impls
                                .entry(f.name.clone())
                                .or_default()
                                .push((None, node));
                        }
                    }
                    Item::ExternFn(x) => {
                        d.opaque.entry(x.def).or_default();
                    }
                    Item::Const(_) | Item::ExternType(_) | Item::Test(_) => {}
                }
            }
        }
        for (def, info) in &program.defs {
            let known = d.fns.contains_key(def) || d.opaque.contains_key(def);
            match info.kind {
                DefKind::ExternFn => {
                    d.opaque.entry(*def).or_default();
                }
                DefKind::Fn | DefKind::Method { .. } if !known && info.intrinsic.is_some() => {
                    d.opaque.entry(*def).or_default();
                }
                DefKind::Method { .. } if !known => {
                    d.signatures.insert(*def, info.name.clone());
                }
                _ => {}
            }
        }
        d
    }

    fn node(&mut self) -> usize {
        let node = self.declared;
        self.declared += 1;
        node
    }

    /// A function or method; `owner` is the type of a method.
    fn function(&mut self, f: &Function, owner: Option<DefRef>) {
        if f.body.is_some() {
            let node = self.node();
            self.fns.insert(f.def, node);
            if f.interface.is_some() {
                self.impls
                    .entry(f.name.clone())
                    .or_default()
                    .push((owner, node));
            }
        } else {
            let calls = self.callbacks(f);
            if f.interface.is_some() {
                self.opaque_impls
                    .entry(f.name.clone())
                    .or_default()
                    .extend(calls.iter().cloned());
            }
            self.opaque.entry(f.def).or_default().extend(calls);
        }
    }

    /// The interface methods that a function of std may call: those of its bounds, with their
    /// super-interfaces.
    fn callbacks(&self, f: &Function) -> BTreeSet<String> {
        let mut stack: Vec<DefRef> = f
            .generics
            .iter()
            .flat_map(|g| g.bounds.iter())
            .chain(f.impl_bounds.iter().flatten())
            .filter_map(|t| match t {
                Type::Named(i, _) => Some(*i),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        while let Some(i) = stack.pop() {
            if !seen.insert(i) {
                continue;
            }
            if let Some((methods, supers)) = self.interfaces.get(&i) {
                names.extend(methods.iter().cloned());
                stack.extend(supers.iter().copied());
            }
        }
        if self.display.is_some_and(|d| seen.contains(&d)) {
            names.insert("message".to_string());
        }
        names
    }

    fn is_interface(&self, def: DefRef) -> bool {
        self.interfaces.contains_key(&def)
    }
}

/// What the body of a function or closure calls.
#[derive(Default)]
struct Calls {
    /// Functions, methods, defaults and closures called by name.
    nodes: BTreeSet<usize>,
    /// Interface methods that may be called, with the types whose implementations count.
    methods: Vec<(String, Reach)>,
    /// The types of the function values that may be called.
    values: Vec<Type>,
    /// Calls of function values that may be functions of std or of extern code: the type of the
    /// value, the function types its arguments hold and the types they mention.
    opaque_values: Vec<(Type, Vec<Type>, Reach)>,
}

/// A walk over every body of the program. The first walk finds the calls; the second, which
/// visits the closures in the same order, sets the flags.
struct Walk<'d> {
    decls: &'d Decls,
    /// For the second walk: whether each node is counted.
    recursive: Option<&'d [bool]>,
    /// The node whose body is walked.
    node: usize,
    /// The type parameters of the type of the method walked.
    owner_params: BTreeSet<String>,
    /// Whether `Self` is the type of the receiver of the function walked: in a default.
    in_default: bool,
    /// The node of the next closure.
    next_closure: usize,
    /// What each node calls (first walk).
    calls: Vec<Calls>,
    /// The node and the type of each closure (first walk).
    closures: Vec<(usize, Type)>,
    /// The functions used as values, with their types (first walk).
    taken: Vec<(DefRef, Type)>,
    /// Immutable locals that hold a closure, with the closure's node (first walk).
    known: BTreeMap<LocalRef, usize>,
    /// The type of the method walked.
    owner: Option<DefRef>,
    /// Generic types that the program uses with an interface, or with a type parameter of other
    /// code, among their type arguments (first walk).
    open: BTreeSet<DefRef>,
}

impl<'d> Walk<'d> {
    fn new(decls: &'d Decls, recursive: Option<&'d [bool]>) -> Walk<'d> {
        let mut calls = Vec::new();
        calls.resize_with(decls.declared, Calls::default);
        Walk {
            decls,
            recursive,
            node: ROOT,
            owner_params: BTreeSet::new(),
            in_default: false,
            next_closure: decls.declared,
            calls,
            closures: Vec::new(),
            taken: Vec::new(),
            known: BTreeMap::new(),
            owner: None,
            open: BTreeSet::new(),
        }
    }

    fn collecting(&self) -> bool {
        self.recursive.is_none()
    }

    fn counted(&self, node: usize) -> bool {
        self.recursive
            .and_then(|c| c.get(node))
            .copied()
            .unwrap_or(false)
    }

    fn here(&mut self) -> Option<&mut Calls> {
        self.calls.get_mut(self.node)
    }

    // ----- declarations -----------------------------------------------------------------------

    fn modules(&mut self, modules: &mut [Module]) {
        for m in modules {
            for item in &mut m.items {
                self.item(item);
            }
        }
    }

    fn item(&mut self, item: &mut Item) {
        match item {
            Item::Fn(f) => self.function(f, None, None),
            Item::Struct(s) => {
                let owner = Some((s.def, s.generics.as_slice()));
                self.enter_owner(owner, &[]);
                for f in &s.fields {
                    self.scan(&f.ty);
                }
                for f in &mut s.methods {
                    self.function(f, None, owner);
                }
            }
            Item::Enum(e) => {
                let owner = Some((e.def, e.generics.as_slice()));
                self.enter_owner(owner, &[]);
                for v in &e.variants {
                    match &v.payload {
                        Payload::None => {}
                        Payload::Tuple(types) => types.iter().for_each(|t| self.scan(t)),
                        Payload::Struct(fields) => fields.iter().for_each(|f| self.scan(&f.ty)),
                    }
                }
                for f in &mut e.methods {
                    self.function(f, None, owner);
                }
            }
            Item::Builtin(b) => b
                .methods
                .iter_mut()
                .for_each(|f| self.function(f, None, None)),
            Item::Interface(i) => {
                let def = i.def;
                i.methods
                    .iter_mut()
                    .for_each(|f| self.function(f, Some(def), None));
            }
            Item::Test(t) => {
                self.enter_owner(None, &[]);
                self.node = ROOT;
                self.block(&mut t.body);
            }
            Item::ExternFn(x) => {
                self.enter_owner(None, &[]);
                x.params.iter().for_each(|(_, t)| self.scan(t));
                self.scan(&x.ret);
            }
            Item::Const(_) | Item::ExternType(_) => {}
        }
        self.enter_owner(None, &[]);
    }

    /// Starts the code of a method of the type `owner`, with its type parameters, or code outside
    /// a type; a parameter of the function itself with the name of one of the type's hides it.
    fn enter_owner(&mut self, owner: Option<(DefRef, &[Generic])>, own: &[Generic]) {
        self.owner = owner.map(|(d, _)| d);
        self.owner_params = owner
            .map(|(_, params)| params)
            .unwrap_or_default()
            .iter()
            .filter(|g| own.iter().all(|h| h.name != g.name))
            .map(|g| g.name.clone())
            .collect();
    }

    /// Notes the generic types that `t` uses with an interface or a type parameter of other code.
    fn scan(&mut self, t: &Type) {
        if !self.collecting() {
            return;
        }
        match t {
            Type::Named(d, args) => {
                if args.iter().any(|a| self.open_arg(*d, a)) {
                    self.open.insert(*d);
                }
                args.iter().for_each(|a| self.scan(a));
            }
            Type::List(x) | Type::Option(x) => self.scan(x),
            Type::Map(a, b) | Type::Result(a, b) => {
                self.scan(a);
                self.scan(b);
            }
            Type::Fn(params, ret) => {
                params.iter().for_each(|(_, p)| self.scan(p));
                self.scan(ret);
            }
            _ => {}
        }
    }

    /// Whether a type argument of `def` may hold a value of another type than the one written:
    /// an interface, or a type parameter that is not one of `def` in its own methods.
    fn open_arg(&self, def: DefRef, t: &Type) -> bool {
        match t {
            Type::Named(d, args) => {
                self.decls.is_interface(*d) || args.iter().any(|a| self.open_arg(def, a))
            }
            Type::List(x) | Type::Option(x) => self.open_arg(def, x),
            Type::Map(a, b) | Type::Result(a, b) => self.open_arg(def, a) || self.open_arg(def, b),
            Type::Fn(params, ret) => {
                params.iter().any(|(_, p)| self.open_arg(def, p)) || self.open_arg(def, ret)
            }
            Type::Param(p) => !(self.owner == Some(def) && self.owner_params.contains(p)),
            Type::SelfType => true,
            _ => false,
        }
    }

    /// A function, or a default of the interface `default_of`; `owner` is the type of a method
    /// with its type parameters.
    fn function(
        &mut self,
        f: &mut Function,
        default_of: Option<DefRef>,
        owner: Option<(DefRef, &[Generic])>,
    ) {
        let node = match default_of {
            Some(i) => self.decls.defaults.get(&(i, f.name.clone())),
            None => self.decls.fns.get(&f.def),
        }
        .copied()
        .unwrap_or(ROOT);
        let Some(body) = &mut f.body else {
            return;
        };
        if !self.collecting() {
            f.recursive = node != ROOT && self.counted(node);
        }
        self.enter_owner(owner, &f.generics);
        for p in &f.params {
            self.scan(&p.ty);
        }
        self.scan(&f.ret);
        self.in_default = default_of.is_some();
        self.node = node;
        self.block(body);
        self.node = ROOT;
        self.enter_owner(None, &[]);
        self.in_default = false;
    }

    // ----- bodies -----------------------------------------------------------------------------

    fn block(&mut self, b: &mut Block) {
        for s in &mut b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &mut b.tail {
            self.expr(t);
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let {
                local,
                mutable,
                value,
            } => {
                let closure = self.next_closure;
                let holds = !*mutable && matches!(value.kind, ExprKind::Closure { .. });
                self.expr(value);
                if holds && self.collecting() {
                    self.known.insert(*local, closure);
                }
            }
            Stmt::LetPattern { value, .. } | Stmt::Expr(value) => self.expr(value),
            Stmt::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::ForList { list, body, .. } => {
                self.expr(list);
                self.block(body);
            }
            Stmt::ForRange {
                start, end, body, ..
            } => {
                self.expr(start);
                self.expr(end);
                self.block(body);
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        let Expr { kind, ty, .. } = e;
        self.scan(ty);
        match kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Local(_)
            | ExprKind::Const(_)
            | ExprKind::SelfValue
            | ExprKind::None
            | ExprKind::Continue { .. } => {}
            ExprKind::FnValue(def) => {
                if self.collecting() {
                    self.taken.push((*def, ty.clone()));
                }
            }
            ExprKind::Interpolate(items) | ExprKind::List(items) => {
                items.iter_mut().for_each(|x| self.expr(x));
            }
            ExprKind::Variant { values, .. } => values.iter_mut().for_each(|x| self.expr(x)),
            ExprKind::Call { callee, args, .. } => self.call(callee, args),
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Index { base, index, .. } => {
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
                if let Some(x) = end {
                    self.expr(x);
                }
            }
            ExprKind::Struct { fields, base, .. } => {
                fields.iter_mut().for_each(|(_, x)| self.expr(x));
                if let Some(b) = base {
                    self.expr(b);
                }
            }
            ExprKind::Some(x)
            | ExprKind::Ok(x)
            | ExprKind::Err(x)
            | ExprKind::Copy(x)
            | ExprKind::Unary { operand: x, .. }
            | ExprKind::Try { value: x, .. }
            | ExprKind::Coerce { value: x, .. }
            | ExprKind::Panic { message: x, .. }
            | ExprKind::Assert { cond: x, .. } => self.expr(x),
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Closure {
                body, recursive, ..
            } => self.closure(ty, body, recursive),
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
            ExprKind::IfChain {
                items,
                then,
                otherwise,
            } => {
                for item in items {
                    match item {
                        CondItem::Let { value, .. } | CondItem::Bool(value) => self.expr(value),
                    }
                }
                self.block(then);
                if let Some(o) = otherwise {
                    self.block(o);
                }
            }
            ExprKind::Match {
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for arm in arms {
                    if let Some(g) = &mut arm.guard {
                        self.expr(g);
                    }
                    self.block(&mut arm.body);
                }
            }
            ExprKind::Block(b) | ExprKind::Loop { body: b, .. } => self.block(b),
            ExprKind::Return(value) | ExprKind::Break { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
        }
    }

    /// A closure: a function of its own, called only through function values, so the calls in
    /// its body keep no relation to the type of the function around it.
    fn closure(&mut self, ty: &Type, body: &mut Block, recursive: &mut bool) {
        let node = self.next_closure;
        self.next_closure += 1;
        if self.collecting() {
            self.calls.push(Calls::default());
            self.closures.push((node, ty.clone()));
        } else {
            *recursive = self.counted(node);
        }
        let outer = std::mem::replace(&mut self.node, node);
        let params = std::mem::take(&mut self.owner_params);
        let owner = self.owner.take();
        let in_default = std::mem::replace(&mut self.in_default, false);
        self.block(body);
        self.node = outer;
        self.owner_params = params;
        self.owner = owner;
        self.in_default = in_default;
    }

    fn call(&mut self, callee: &mut Callee, args: &mut [Arg]) {
        if self.collecting() {
            match &*callee {
                Callee::Fn(def) => self.direct(*def, None, args),
                Callee::Method { method, recv } => self.direct(*method, Some(&recv.ty), args),
                Callee::Interface { name, recv, .. } => {
                    let mut reach = Reach::default();
                    self.reach_of(&recv.ty, &mut reach);
                    if reach.types.is_empty() && !reach.same && reach.inner.is_none() {
                        reach.any = true;
                    }
                    self.by_name(name, &reach);
                }
                Callee::Default {
                    interface, name, ..
                } => match self.decls.defaults.get(&(*interface, name.clone())) {
                    Some(&node) => self.add_node(node),
                    None => self.by_name(
                        name,
                        &Reach {
                            any: true,
                            ..Reach::default()
                        },
                    ),
                },
                Callee::Value(f) => match &f.kind {
                    ExprKind::Local(l) if self.known.contains_key(l) => {
                        if let Some(&node) = self.known.get(l) {
                            self.add_node(node);
                        }
                    }
                    _ => {
                        let value = f.ty.clone();
                        let types: Vec<Type> = args.iter().map(|a| a.value.ty.clone()).collect();
                        let mut fns = Vec::new();
                        let mut reach = Reach::default();
                        for t in &types {
                            fn_types(t, &mut fns);
                            self.reach_of(t, &mut reach);
                        }
                        if let Some(c) = self.here() {
                            c.values.push(value.clone());
                            c.opaque_values.push((value, fns, reach));
                        }
                    }
                },
            }
        }
        match callee {
            Callee::Fn(_) => {}
            Callee::Method { recv, .. }
            | Callee::Interface { recv, .. }
            | Callee::Default { recv, .. }
            | Callee::Value(recv) => self.expr(recv),
        }
        for a in args {
            self.expr(&mut a.value);
        }
    }

    fn add_node(&mut self, node: usize) {
        if let Some(c) = self.here() {
            c.nodes.insert(node);
        }
    }

    /// A call of a declared function or method.
    fn direct(&mut self, def: DefRef, recv: Option<&Type>, args: &[Arg]) {
        let decls = self.decls;
        if let Some(&node) = decls.fns.get(&def) {
            self.add_node(node);
            return;
        }
        if let Some(name) = decls.signatures.get(&def) {
            let any = Reach {
                any: true,
                ..Reach::default()
            };
            self.by_name(name, &any);
            return;
        }
        // Code of std or of extern code, or a declaration the walk does not know: it may call
        // what it is given.
        let mut values = Vec::new();
        let mut reach = Reach::default();
        for t in recv.into_iter().chain(args.iter().map(|a| &a.value.ty)) {
            fn_types(t, &mut values);
            self.reach_of(t, &mut reach);
        }
        let methods: Vec<(String, Reach)> = decls
            .opaque
            .get(&def)
            .into_iter()
            .flatten()
            .map(|n| (n.clone(), reach.clone()))
            .collect();
        if let Some(c) = self.here() {
            c.values.extend(values);
            c.methods.extend(methods);
        }
    }

    /// A call of an interface method that the IR leaves to the type of the value.
    fn by_name(&mut self, name: &str, reach: &Reach) {
        let decls = self.decls;
        if let Some(c) = self.here() {
            c.methods.push((name.to_string(), reach.clone()));
            for also in decls.opaque_impls.get(name).into_iter().flatten() {
                c.methods.push((also.clone(), reach.clone()));
            }
        }
    }

    /// Adds the types that a value of type `t` may hold to `reach`.
    fn reach_of(&self, t: &Type, reach: &mut Reach) {
        match t {
            Type::Named(d, args) => {
                if self.decls.is_interface(*d) {
                    reach.any = true;
                } else {
                    reach.types.insert(*d);
                }
                args.iter().for_each(|a| self.reach_of(a, reach));
            }
            Type::List(x) | Type::Option(x) => self.reach_of(x, reach),
            Type::Map(a, b) | Type::Result(a, b) => {
                self.reach_of(a, reach);
                self.reach_of(b, reach);
            }
            Type::Param(p) if self.owner.is_some() && self.owner_params.contains(p) => {
                reach.inner = self.owner;
            }
            Type::SelfType if self.in_default => reach.same = true,
            Type::Param(_) | Type::SelfType => reach.any = true,
            _ => {}
        }
    }

    // ----- the graph --------------------------------------------------------------------------

    /// The calls of each node as edges to the nodes they may call.
    fn edges(&self) -> Vec<Vec<(usize, Step)>> {
        let decls = self.decls;
        self.calls
            .iter()
            .map(|c| {
                let mut values = c.values.clone();
                let mut methods = c.methods.clone();
                for (value, fns, reach) in &c.opaque_values {
                    for (def, ty) in &self.taken {
                        let Some(names) = decls.opaque.get(def) else {
                            continue;
                        };
                        if fits(decls, value, ty) {
                            values.extend(fns.iter().cloned());
                            methods.extend(names.iter().map(|n| (n.clone(), reach.clone())));
                        }
                    }
                }
                let mut out: BTreeMap<usize, Step> =
                    c.nodes.iter().map(|&n| (n, Step::Any)).collect();
                let mut add = |node: usize, step: Step| {
                    let s = out.entry(node).or_insert(step);
                    *s = (*s).min(step);
                };
                let any = Reach {
                    any: true,
                    ..Reach::default()
                };
                for value in &values {
                    for (node, ty) in &self.closures {
                        if fits(decls, value, ty) {
                            add(*node, Step::Any);
                        }
                    }
                    for (def, ty) in &self.taken {
                        if !fits(decls, value, ty) {
                            continue;
                        }
                        if let Some(&node) = decls.fns.get(def) {
                            add(node, Step::Any);
                        } else if let Some(name) = decls.signatures.get(def) {
                            methods.push((name.clone(), any.clone()));
                        }
                    }
                }
                for (name, reach) in &methods {
                    for (owner, node) in decls.impls.get(name).into_iter().flatten() {
                        let named = match owner {
                            Some(o) => reach.types.contains(o),
                            None => !reach.types.is_empty(),
                        };
                        let step = match reach.inner {
                            _ if reach.any || named => Some(Step::Any),
                            _ if reach.same => Some(Step::Same),
                            Some(d) if self.open.contains(&d) => Some(Step::Any),
                            Some(_) => Some(Step::Inner),
                            None => None,
                        };
                        if let Some(step) = step {
                            add(*node, step);
                        }
                    }
                }
                out.into_iter().collect()
            })
            .collect()
    }
}

/// The function types in `t`.
fn fn_types(t: &Type, out: &mut Vec<Type>) {
    match t {
        Type::Fn(params, ret) => {
            out.push(t.clone());
            for (_, p) in params {
                fn_types(p, out);
            }
            fn_types(ret, out);
        }
        Type::List(x) | Type::Option(x) => fn_types(x, out),
        Type::Map(a, b) | Type::Result(a, b) => {
            fn_types(a, out);
            fn_types(b, out);
        }
        Type::Named(_, args) => args.iter().for_each(|a| fn_types(a, out)),
        _ => {}
    }
}

/// Whether a function of type `target` may be the function value of type `value` that a call
/// calls. Only function types are compared; anything else may be anything.
fn fits(decls: &Decls, value: &Type, target: &Type) -> bool {
    match (value, target) {
        (Type::Fn(..), Type::Fn(..)) => same_shape(decls, value, target),
        _ => true,
    }
}

/// Whether two types may be the same type: a type parameter, `Self`, `Never` and an interface
/// stand for any type.
fn same_shape(decls: &Decls, a: &Type, b: &Type) -> bool {
    let any = |t: &Type| match t {
        Type::Param(_) | Type::SelfType | Type::Never => true,
        Type::Named(d, _) => decls.is_interface(*d),
        _ => false,
    };
    if any(a) || any(b) {
        return true;
    }
    match (a, b) {
        (Type::List(x), Type::List(y)) | (Type::Option(x), Type::Option(y)) => {
            same_shape(decls, x, y)
        }
        (Type::Map(k, v), Type::Map(k2, v2)) | (Type::Result(k, v), Type::Result(k2, v2)) => {
            same_shape(decls, k, k2) && same_shape(decls, v, v2)
        }
        (Type::Named(d, xs), Type::Named(e, ys)) => {
            d == e
                && xs.len() == ys.len()
                && xs.iter().zip(ys).all(|(x, y)| same_shape(decls, x, y))
        }
        (Type::Fn(ps, r), Type::Fn(qs, s)) => {
            ps.len() == qs.len()
                && ps
                    .iter()
                    .zip(qs)
                    .all(|((_, p), (_, q))| same_shape(decls, p, q))
                && same_shape(decls, r, s)
        }
        _ => a == b,
    }
}

/// Which nodes are counted: those of a strongly connected component of the call graph with a
/// cycle that can repeat without end. A cycle cannot when all its calls are [`Step::Same`] or
/// [`Step::Inner`] and one of them is [`Step::Inner`]: the type of the receiver never grows and
/// shrinks once on every turn.
fn recursive(edges: &[Vec<(usize, Step)>]) -> Vec<bool> {
    let all: Vec<Vec<usize>> = edges
        .iter()
        .map(|e| e.iter().map(|&(w, _)| w).collect())
        .collect();
    let (component, _) = components(&all);
    let same =
        |u: usize, w: usize| component.get(u).is_some() && component.get(u) == component.get(w);
    // The calls of `Self` inside each component: a cycle of only those repeats on one type.
    let selves: Vec<Vec<usize>> = edges
        .iter()
        .enumerate()
        .map(|(u, e)| {
            e.iter()
                .filter(|&&(w, step)| step == Step::Same && same(u, w))
                .map(|&(w, _)| w)
                .collect()
        })
        .collect();
    let (_, on_self_cycle) = components(&selves);
    let mut endless = BTreeSet::new();
    for (u, e) in edges.iter().enumerate() {
        let any = e.iter().any(|&(w, step)| step == Step::Any && same(u, w));
        if any || on_self_cycle.get(u).copied().unwrap_or(false) {
            endless.extend(component.get(u).copied());
        }
    }
    component.iter().map(|c| endless.contains(c)).collect()
}

/// Tarjan's algorithm for strongly connected components, without recursion, so that a long chain
/// of calls cannot overflow the stack of the compiler: the component of each node, and whether
/// the node lies on a cycle (a component of several nodes, or an edge to itself).
fn components(edges: &[Vec<usize>]) -> (Vec<usize>, Vec<bool>) {
    const UNSEEN: usize = usize::MAX;
    #[derive(Clone)]
    struct State {
        index: usize,
        low: usize,
        on_stack: bool,
    }
    let fresh = State {
        index: UNSEEN,
        low: UNSEEN,
        on_stack: false,
    };
    let mut states = vec![fresh; edges.len()];
    let mut component = vec![UNSEEN; edges.len()];
    let mut cyclic = vec![false; edges.len()];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter = 0;
    let mut components = 0;
    let mut visit = |states: &mut [State], stack: &mut Vec<usize>, v: usize| {
        if let Some(s) = states.get_mut(v) {
            *s = State {
                index: counter,
                low: counter,
                on_stack: true,
            };
            counter += 1;
            stack.push(v);
        }
    };
    for root in 0..edges.len() {
        if states.get(root).is_none_or(|s| s.index != UNSEEN) {
            continue;
        }
        visit(&mut states, &mut stack, root);
        // The nodes being visited, each with the position of its next edge.
        let mut work = vec![(root, 0)];
        while let Some(&(v, i)) = work.last() {
            if let Some(&w) = edges.get(v).and_then(|e| e.get(i)) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                match states.get(w).map(|s| (s.index, s.on_stack)) {
                    Some((UNSEEN, _)) => {
                        visit(&mut states, &mut stack, w);
                        work.push((w, 0));
                    }
                    Some((index, true)) => {
                        if let Some(s) = states.get_mut(v) {
                            s.low = s.low.min(index);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            work.pop();
            let (index, low) = states.get(v).map_or((UNSEEN, UNSEEN), |s| (s.index, s.low));
            if let Some(s) = work.last().and_then(|&(u, _)| states.get_mut(u)) {
                s.low = s.low.min(low);
            }
            if low != index {
                continue;
            }
            let mut members = Vec::new();
            while let Some(w) = stack.pop() {
                if let Some(s) = states.get_mut(w) {
                    s.on_stack = false;
                }
                members.push(w);
                if w == v {
                    break;
                }
            }
            let calls_itself = edges.get(v).is_some_and(|e| e.contains(&v));
            let on_cycle = members.len() > 1 || calls_itself;
            for m in members {
                if let Some(c) = component.get_mut(m) {
                    *c = components;
                }
                if let Some(c) = cyclic.get_mut(m) {
                    *c = on_cycle;
                }
            }
            components += 1;
        }
    }
    (component, cyclic)
}

#[cfg(test)]
mod tests {
    use super::{Step, components, recursive};

    #[test]
    fn cycles_of_the_call_graph() {
        // 0 -> 1 -> 2 -> 1, 3 -> 3, 4 -> 0, 5 alone.
        let edges = vec![vec![1], vec![2], vec![1], vec![3], vec![0], vec![]];
        let (component, cyclic) = components(&edges);
        assert_eq!(cyclic, vec![false, true, true, true, false, false]);
        assert_eq!(component[1], component[2]);
        assert_ne!(component[0], component[1]);
    }

    #[test]
    fn a_long_chain_needs_no_deep_stack() {
        let n = 200_000;
        let mut edges: Vec<Vec<usize>> = (0..n).map(|i| vec![i + 1]).collect();
        edges.push(vec![0]);
        assert!(components(&edges).1.iter().all(|&c| c));
    }

    #[test]
    fn calls_into_nested_types_end() {
        use Step::*;
        // 0: `Holder<T>.label` calls `label` of `T`, maybe itself.
        // 1 -> 2 by `Self`, 2 -> 1 into `T`: a default and a wrapper.
        // 3 <-> 4 by `Self` only: a default and an implementation that call each other.
        // 5 -> 6 into `T`, 6 -> 5 by any call.
        let edges = vec![
            vec![(0, Inner)],
            vec![(2, Same)],
            vec![(1, Inner)],
            vec![(4, Same)],
            vec![(3, Same)],
            vec![(6, Inner)],
            vec![(5, Any)],
        ];
        assert_eq!(
            recursive(&edges),
            vec![false, false, false, true, true, true, true]
        );
    }
}
