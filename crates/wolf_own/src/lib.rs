//! The ownership check (spec chapter 5).
//!
//! It runs on the IR of a program without type errors. Each function body is walked once, in
//! order, with a *state*: which variables are moved at this point. Branches (`if`, `match`) are
//! walked from the same state and their results are joined; a variable moved in only some
//! branches is "maybe moved" after the join. Loops are walked once, and a variable declared
//! outside a loop that is still moved at the end of the body would be used moved by the next
//! iteration. Borrows of `for` loops and `match` arms are kept on a stack while their bodies are
//! walked, and every access is checked against them.

use std::collections::{BTreeMap, BTreeSet};

use wolf_diag::{Diagnostic, codes};
use wolf_ir::{
    Arg, Block, Callee, DefRef, Expr, ExprKind, Function, Item, LocalRef, Mode, Pattern, Program,
    Stmt, Type,
};
use wolf_span::Span;

/// Checks the ownership rules of a program and returns the diagnostics.
pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut c = Checker::new(program);
    for m in &program.modules {
        for item in &m.items {
            match item {
                Item::Fn(f) => c.function(f),
                Item::Struct(s) => s.methods.iter().for_each(|f| c.function(f)),
                Item::Enum(e) => e.methods.iter().for_each(|f| c.function(f)),
                Item::Builtin(b) => b.methods.iter().for_each(|f| c.function(f)),
                Item::Test(t) => c.body(&[], None, &t.body),
                _ => {}
            }
        }
    }
    c.diagnostics
}

/// The start of a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Root {
    Local(LocalRef),
    SelfValue,
}

/// A segment of a place after its root.
#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Field(String),
    /// Any index: two indices into one list always overlap.
    Index,
}

/// A place: a root and a path through fields and indices.
#[derive(Clone, Debug, PartialEq)]
struct Place {
    root: Root,
    segs: Vec<Seg>,
}

impl Place {
    fn overlaps(&self, other: &Place) -> bool {
        self.root == other.root && self.segs.iter().zip(&other.segs).all(|(a, b)| a == b)
    }
}

/// How a binding may be used.
#[derive(Clone, Copy, Debug)]
struct Facts {
    /// It may be changed.
    mutable: bool,
    /// It is borrowed: it cannot be moved out of (a read or `mut` parameter, a binding of a
    /// borrowing `for` or `match`).
    borrowed: bool,
    /// It is captured by the closure being checked.
    captured: bool,
    /// Where it is declared, for messages; `None` for `self`.
    name: Option<LocalRef>,
}

/// Where a variable was moved.
#[derive(Clone, Copy, Debug)]
struct Moved {
    span: Span,
    /// Moved on some paths only.
    maybe: bool,
}

/// What is known at a point of a body.
#[derive(Clone, Debug, Default)]
struct State {
    /// The point cannot be reached (after `return`, `break` or a panic).
    unreachable: bool,
    moved: BTreeMap<Root, Moved>,
}

impl State {
    fn join(a: State, b: State) -> State {
        if a.unreachable {
            return b;
        }
        if b.unreachable {
            return a;
        }
        let mut moved = BTreeMap::new();
        let roots: BTreeSet<Root> = a.moved.keys().chain(b.moved.keys()).copied().collect();
        for r in roots {
            match (a.moved.get(&r), b.moved.get(&r)) {
                (Some(x), Some(y)) => {
                    moved.insert(
                        r,
                        Moved {
                            span: x.span,
                            maybe: x.maybe || y.maybe,
                        },
                    );
                }
                (Some(x), None) | (None, Some(x)) => {
                    moved.insert(
                        r,
                        Moved {
                            span: x.span,
                            maybe: true,
                        },
                    );
                }
                (None, None) => {}
            }
        }
        State {
            unreachable: false,
            moved,
        }
    }
}

/// A borrow of a `for` loop or a `match` arm.
#[derive(Clone, Debug)]
struct Borrow {
    place: Place,
    mutable: bool,
    span: Span,
    what: &'static str,
}

/// Loops around the current point: the states at `break` and `continue`, and the locals
/// declared inside.
#[derive(Default)]
struct LoopCtx {
    label: Option<String>,
    breaks: Vec<State>,
    continues: Vec<State>,
}

/// How an expression is used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Use {
    /// Read: operands, read arguments, conditions.
    Read,
    /// An owning position: the value is moved if it is a place of a move type.
    Own,
}

/// What an access does to a place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    Read,
    Mutate,
    Move,
}

/// The mode of `self` and the parameters of a called function.
type CallSig = (Option<Mode>, Vec<(Mode, Type)>);

struct Sig {
    self_mode: Option<Mode>,
    params: Vec<(Mode, Type)>,
}

struct Checker<'p> {
    p: &'p Program,
    diagnostics: Vec<Diagnostic>,
    fns: BTreeMap<DefRef, Sig>,
    iface_fns: BTreeMap<(DefRef, String), Sig>,
    /// Enums whose variants have no payloads (copy types, ADR 0004).
    plain_enums: BTreeSet<DefRef>,
    // The body being checked.
    facts: BTreeMap<Root, Facts>,
    state: State,
    borrows: Vec<Borrow>,
    loops: Vec<LoopCtx>,
    /// Locals declared since the start of each enclosing loop.
    declared: Vec<BTreeSet<LocalRef>>,
    /// Reported moves, to report a use after a move once per place.
    reported: BTreeSet<(Root, u32)>,
}

impl<'p> Checker<'p> {
    fn new(p: &'p Program) -> Checker<'p> {
        let mut c = Checker {
            p,
            diagnostics: Vec::new(),
            fns: BTreeMap::new(),
            iface_fns: BTreeMap::new(),
            plain_enums: BTreeSet::new(),
            facts: BTreeMap::new(),
            state: State::default(),
            borrows: Vec::new(),
            loops: Vec::new(),
            declared: Vec::new(),
            reported: BTreeSet::new(),
        };
        let sig = |f: &Function| Sig {
            self_mode: f.self_mode,
            params: f.params.iter().map(|x| (x.mode, x.ty.clone())).collect(),
        };
        for m in &p.modules {
            for item in &m.items {
                match item {
                    Item::Fn(f) => {
                        c.fns.insert(f.def, sig(f));
                    }
                    Item::Struct(s) => s.methods.iter().for_each(|f| {
                        c.fns.insert(f.def, sig(f));
                    }),
                    Item::Enum(e) => {
                        if e.generics.is_empty() && e.variants.iter().all(|v| v.payload.is_empty())
                        {
                            c.plain_enums.insert(e.def);
                        }
                        e.methods.iter().for_each(|f| {
                            c.fns.insert(f.def, sig(f));
                        });
                    }
                    Item::Builtin(b) => b.methods.iter().for_each(|f| {
                        c.fns.insert(f.def, sig(f));
                    }),
                    Item::Interface(i) => i.methods.iter().for_each(|f| {
                        c.iface_fns.insert((i.def, f.name.clone()), sig(f));
                    }),
                    Item::ExternFn(x) => {
                        c.fns.insert(
                            x.def,
                            Sig {
                                self_mode: None,
                                params: x.params.clone(),
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        c
    }

    fn report(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    /// Copy types are copied on every use (chapter 5, section 2).
    fn copy_type(&self, t: &Type) -> bool {
        match t {
            Type::Int | Type::Float | Type::Bool | Type::Void | Type::Never => true,
            Type::Named(d, args) => args.is_empty() && self.plain_enums.contains(d),
            _ => false,
        }
    }

    fn name(&self, root: Root) -> String {
        match root {
            Root::SelfValue => "self".to_string(),
            Root::Local(l) => self
                .p
                .locals
                .get(&l)
                .map(|x| x.name.clone())
                .unwrap_or_default(),
        }
    }

    fn place_text(&self, place: &Place) -> String {
        let mut s = self.name(place.root);
        for seg in &place.segs {
            match seg {
                Seg::Field(f) => {
                    s.push('.');
                    s.push_str(f);
                }
                Seg::Index => s.push_str("[...]"),
            }
        }
        s
    }

    // ----- bodies -----------------------------------------------------------------------------

    fn function(&mut self, f: &Function) {
        if let Some(body) = &f.body {
            self.body(&f.params, f.self_mode, body);
        }
    }

    fn body(&mut self, params: &[wolf_ir::Param], self_mode: Option<Mode>, body: &Block) {
        self.facts.clear();
        self.state = State::default();
        self.borrows.clear();
        self.loops.clear();
        self.declared.clear();
        self.reported.clear();
        for p in params {
            self.facts
                .insert(Root::Local(p.local), param_facts(p.mode, p.local));
        }
        if let Some(mode) = self_mode {
            let mut f = param_facts(mode, LocalRef(u32::MAX));
            f.name = None;
            self.facts.insert(Root::SelfValue, f);
        }
        // The final expression of a function body is an owning position.
        self.block(body, Use::Own);
    }

    fn declare(&mut self, local: LocalRef, facts: Facts) {
        self.facts.insert(Root::Local(local), facts);
        self.state.moved.remove(&Root::Local(local));
        for d in &mut self.declared {
            d.insert(local);
        }
    }

    fn block(&mut self, b: &Block, tail_use: Use) {
        for s in &b.stmts {
            self.stmt(s);
        }
        if let Some(t) = &b.tail {
            self.expr(t, tail_use);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let {
                local,
                mutable,
                value,
            } => {
                self.expr(value, Use::Own);
                self.declare(
                    *local,
                    Facts {
                        mutable: *mutable,
                        borrowed: false,
                        captured: false,
                        name: Some(*local),
                    },
                );
            }
            Stmt::LetPattern {
                pattern,
                mutable,
                value,
            } => {
                // An owning position, like `match own` (chapter 5, section 7.4, rule 6).
                self.expr(value, Use::Own);
                let mut binds = Vec::new();
                bound(pattern, &mut binds);
                for l in binds {
                    let mut f = owned_binding(l);
                    f.mutable = *mutable;
                    self.declare(l, f);
                }
            }
            Stmt::Assign { target, op, value } => {
                self.expr(value, if op.is_some() { Use::Read } else { Use::Own });
                self.assign(target, op.is_none());
            }
            Stmt::While { label, cond, body } => {
                let infinite = matches!(cond.kind, ExprKind::Bool(true));
                self.expr(cond, Use::Read);
                self.loop_body(label, body, infinite, |c, b| c.block(b, Use::Read));
            }
            Stmt::ForList {
                label,
                var,
                mode,
                list,
                body,
            } => {
                let place = self.place(list);
                let facts = match (mode, &place) {
                    (Mode::Own, _) => {
                        self.expr(list, Use::Own);
                        owned_binding(*var)
                    }
                    (Mode::Read, Some(p)) => {
                        self.access(p, Access::Read, list.span);
                        self.borrows.push(Borrow {
                            place: p.clone(),
                            mutable: false,
                            span: list.span,
                            what: "this `for` loop reads",
                        });
                        borrowed_binding(*var, false)
                    }
                    (Mode::Read, None) => {
                        self.expr(list, Use::Read);
                        borrowed_binding(*var, false)
                    }
                    (Mode::Mut, Some(p)) => {
                        self.access(p, Access::Mutate, list.span);
                        self.borrows.push(Borrow {
                            place: p.clone(),
                            mutable: true,
                            span: list.span,
                            what: "this `for` loop changes",
                        });
                        borrowed_binding(*var, true)
                    }
                    (Mode::Mut, None) => {
                        self.expr(list, Use::Read);
                        self.report(
                            Diagnostic::new(codes::E0506, "`for x in mut` needs a mutable place")
                                .primary(list.span, "this is a temporary value")
                                .help("store it in a `var` first, or loop over it without `mut`"),
                        );
                        borrowed_binding(*var, true)
                    }
                };
                let borrowed = matches!(mode, Mode::Read | Mode::Mut) && place.is_some();
                self.loop_body(label, body, false, |c, b| {
                    c.declare(*var, facts);
                    c.block(b, Use::Read);
                });
                if borrowed {
                    self.borrows.pop();
                }
            }
            Stmt::ForRange {
                label,
                var,
                start,
                end,
                body,
                ..
            } => {
                self.expr(start, Use::Read);
                self.expr(end, Use::Read);
                self.loop_body(label, body, false, |c, b| {
                    c.declare(*var, owned_binding(*var));
                    c.block(b, Use::Read);
                });
            }
            Stmt::Expr(e) => self.expr(e, Use::Read),
        }
    }

    /// A loop body: walked once; moves of outer variables that reach the next iteration are
    /// errors (chapter 5, rule 4.4).
    fn loop_body(
        &mut self,
        label: &Option<String>,
        body: &Block,
        infinite: bool,
        walk: impl FnOnce(&mut Self, &Block),
    ) {
        let entry = self.state.clone();
        self.loops.push(LoopCtx {
            label: label.clone(),
            ..LoopCtx::default()
        });
        self.declared.push(BTreeSet::new());
        walk(self, body);
        let ctx = self.loops.pop().unwrap_or_default();
        let inner = self.declared.pop().unwrap_or_default();
        let mut back = self.state.clone();
        for c in ctx.continues {
            back = State::join(back, c);
        }
        if !back.unreachable {
            let moved: Vec<(Root, Moved)> = back
                .moved
                .iter()
                .filter(|(r, _)| !entry.moved.contains_key(r))
                .filter(|(r, _)| !matches!(r, Root::Local(l) if inner.contains(l)))
                .map(|(r, m)| (*r, *m))
                .collect();
            for (root, m) in moved {
                let name = self.name(root);
                self.report(
                    Diagnostic::new(
                        codes::E0502,
                        format!("`{name}` is moved in one iteration and used in the next"),
                    )
                    .primary(m.span, "moved here, inside the loop")
                    .help(format!(
                        "assign `{name}` a new value before the next iteration, move a copy (`{name}.copy()`), or leave the loop after the move"
                    )),
                );
                // Report it once.
                back.moved.remove(&root);
            }
        }
        let mut after = if infinite {
            State {
                unreachable: true,
                ..State::default()
            }
        } else {
            State::join(entry, back)
        };
        for b in ctx.breaks {
            after = State::join(after, b);
        }
        self.state = after;
    }

    // ----- places -----------------------------------------------------------------------------

    /// The place an expression names, if it is one (through coercions).
    fn place(&self, e: &Expr) -> Option<Place> {
        match &e.kind {
            ExprKind::Local(l) => Some(Place {
                root: Root::Local(*l),
                segs: Vec::new(),
            }),
            ExprKind::SelfValue => Some(Place {
                root: Root::SelfValue,
                segs: Vec::new(),
            }),
            ExprKind::Field { base, name } => {
                let mut p = self.place(base)?;
                p.segs.push(Seg::Field(name.clone()));
                Some(p)
            }
            ExprKind::Index { base, .. } => {
                let mut p = self.place(base)?;
                p.segs.push(Seg::Index);
                Some(p)
            }
            ExprKind::Coerce { value, .. } => self.place(value),
            _ => None,
        }
    }

    /// Evaluates the index expressions inside a place (they are read).
    fn place_parts(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Field { base, .. } => self.place_parts(base),
            ExprKind::Index { base, index } => {
                self.place_parts(base);
                self.expr(index, Use::Read);
            }
            ExprKind::Coerce { value, .. } => self.place_parts(value),
            _ => {}
        }
    }

    /// Checks one access to a place: moved roots, borrows, and the rules of moving and changing.
    fn access(&mut self, place: &Place, access: Access, span: Span) {
        if let Some(m) = self.state.moved.get(&place.root).copied() {
            let key = (place.root, m.span.start);
            if !self.reported.contains(&key) {
                self.reported.insert(key);
                let name = self.name(place.root);
                let d = if m.maybe {
                    Diagnostic::new(
                        codes::E0502,
                        format!("use of `{name}`, which may have been moved"),
                    )
                    .primary(span, "used here")
                    .secondary(m.span, "moved here on some paths")
                } else {
                    Diagnostic::new(codes::E0501, format!("use of moved value `{name}`"))
                        .primary(span, "value used here after the move")
                        .secondary(m.span, "value moved here")
                };
                self.report(d.help(format!("pass a copy instead of moving it: `{name}.copy()`")));
            }
        }
        let conflict = self
            .borrows
            .iter()
            .rev()
            .find(|b| b.place.overlaps(place) && (b.mutable || access != Access::Read))
            .cloned();
        if let Some(b) = conflict {
            let what = match access {
                Access::Read => "use",
                Access::Mutate => "change",
                Access::Move => "move",
            };
            let text = self.place_text(place);
            self.report(
                Diagnostic::new(
                    codes::E0509,
                    format!("cannot {what} `{text}` while it is borrowed"),
                )
                .primary(span, format!("{what} here"))
                .secondary(b.span, b.what.to_string())
                .note(if b.mutable {
                    "inside the body, the place is reached only through the bindings"
                } else {
                    "a `for` loop or a `match` arm reads the place for its whole body"
                }),
            );
        }
        let facts = self.facts.get(&place.root).copied();
        match access {
            Access::Move => {
                let text = self.place_text(place);
                let borrowed = facts.is_some_and(|f| f.borrowed || f.captured);
                if borrowed {
                    let captured = facts.is_some_and(|f| f.captured);
                    let note = if captured {
                        "a closure can be called many times, so it cannot give away what it captured"
                    } else {
                        "the value is only borrowed here; its owner keeps it"
                    };
                    self.report(
                        Diagnostic::new(
                            codes::E0504,
                            format!("cannot move out of borrowed `{}`", self.name(place.root)),
                        )
                        .primary(span, format!("`{text}` is moved here"))
                        .note(note)
                        .help(format!("move a copy: `{text}.copy()`")),
                    );
                } else if !place.segs.is_empty() {
                    self.report(
                        Diagnostic::new(codes::E0503, format!("cannot move `{text}` out of `{}`", self.name(place.root)))
                            .primary(span, "only whole variables can be moved")
                            .help(format!("copy it (`{text}.copy()`), take the value apart with `match own`, or use `replace(mut {text}, ...)`")),
                    );
                } else {
                    self.state
                        .moved
                        .insert(place.root, Moved { span, maybe: false });
                }
            }
            Access::Mutate => {
                if let Some(f) = facts {
                    if f.captured {
                        let name = self.name(place.root);
                        self.report(
                            Diagnostic::new(
                                codes::E0511,
                                format!("cannot change the captured value `{name}`"),
                            )
                            .primary(span, "changed inside the closure")
                            .note("values captured by a closure cannot be changed inside it"),
                        );
                    } else if !f.mutable {
                        self.immutable(place, span, f);
                    }
                }
            }
            Access::Read => {}
        }
    }

    fn immutable(&mut self, place: &Place, span: Span, f: Facts) {
        let name = self.name(place.root);
        let text = self.place_text(place);
        let mut d = Diagnostic::new(
            codes::E0505,
            format!("cannot change `{text}`: `{name}` is not mutable"),
        )
        .primary(span, "changed here");
        d = match (place.root, f.borrowed) {
            (Root::SelfValue, _) => d.help("take `mut self` in the method"),
            (_, true) => d.note("parameters and bindings that read a value cannot change it; use a `mut` parameter, `for x in mut` or `match mut`"),
            _ => d.help(format!("declare it with `var {name}`")),
        };
        self.report(d);
    }

    /// An assignment target; `whole` for `=` (not `op=`).
    fn assign(&mut self, target: &Expr, whole: bool) {
        self.place_parts(target);
        let Some(place) = self.place(target) else {
            return;
        };
        if whole && place.segs.is_empty() {
            // `x = value` gives a moved variable a value again (chapter 5, rule 4.2).
            let facts = self.facts.get(&place.root).copied();
            let conflict = self
                .borrows
                .iter()
                .rev()
                .find(|b| b.place.overlaps(&place))
                .cloned();
            if let Some(b) = conflict {
                let text = self.place_text(&place);
                self.report(
                    Diagnostic::new(
                        codes::E0509,
                        format!("cannot change `{text}` while it is borrowed"),
                    )
                    .primary(target.span, "change here")
                    .secondary(b.span, b.what.to_string()),
                );
            }
            match facts {
                Some(f) if f.captured => {
                    self.access(&place, Access::Mutate, target.span);
                }
                Some(f) if !f.mutable => self.immutable(&place, target.span, f),
                _ => {
                    self.state.moved.remove(&place.root);
                }
            }
            return;
        }
        self.access(&place, Access::Mutate, target.span);
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr(&mut self, e: &Expr, u: Use) {
        if self.state.unreachable {
            // Code after `return` is not checked again.
        }
        match &e.kind {
            ExprKind::Local(_)
            | ExprKind::SelfValue
            | ExprKind::Field { .. }
            | ExprKind::Index { .. } => {
                self.place_parts(e);
                if let Some(p) = self.place(e) {
                    let access = if u == Use::Own && !self.copy_type(&e.ty) {
                        Access::Move
                    } else {
                        Access::Read
                    };
                    self.access(&p, access, e.span);
                } else {
                    self.subexprs(e);
                }
            }
            ExprKind::Coerce { value, .. } => self.expr(value, u),
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::FnValue(_)
            | ExprKind::Const(_)
            | ExprKind::None => {}
            ExprKind::Interpolate(parts) => {
                for p in parts {
                    self.expr(p, Use::Read);
                }
            }
            ExprKind::Call { callee, args } => self.call(e, callee, args),
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base, Use::Read);
                for b in [start, end].into_iter().flatten() {
                    self.expr(b, Use::Read);
                }
            }
            ExprKind::Struct { fields, base, .. } => {
                for (_, v) in fields {
                    self.expr(v, Use::Own);
                }
                // The base is moved as a whole (chapter 5, section 3, rule 2).
                if let Some(b) = base {
                    self.expr(b, Use::Own);
                }
            }
            ExprKind::Variant { values, .. } | ExprKind::List(values) => {
                for v in values {
                    self.expr(v, Use::Own);
                }
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.expr(k, Use::Own);
                    self.expr(v, Use::Own);
                }
            }
            ExprKind::Some(v) | ExprKind::Ok(v) | ExprKind::Err(v) => self.expr(v, Use::Own),
            ExprKind::Unary { operand, .. } => self.expr(operand, Use::Read),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, Use::Read);
                self.expr(rhs, Use::Read);
            }
            ExprKind::Closure { params, body } => self.closure(e, params, body),
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond, Use::Read);
                let entry = self.state.clone();
                self.block(then, u);
                let after_then = std::mem::replace(&mut self.state, entry);
                if let Some(o) = otherwise {
                    self.block(o, u);
                }
                let after_else = std::mem::take(&mut self.state);
                self.state = State::join(after_then, after_else);
            }
            ExprKind::Match {
                scrutinee,
                mode,
                arms,
            } => self.match_(scrutinee, *mode, arms, u),
            ExprKind::Block(b) => self.block(b, u),
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v, Use::Own);
                }
                self.state.unreachable = true;
            }
            ExprKind::Loop { label, body } => {
                self.loop_body(label, body, true, |c, b| c.block(b, Use::Read));
            }
            ExprKind::Break { label, value } => {
                if let Some(v) = value {
                    self.expr(v, Use::Own);
                }
                let s = self.state.clone();
                if let Some(l) = self.target_loop(label) {
                    l.breaks.push(s);
                }
                self.state.unreachable = true;
            }
            ExprKind::Continue { label } => {
                let s = self.state.clone();
                if let Some(l) = self.target_loop(label) {
                    l.continues.push(s);
                }
                self.state.unreachable = true;
            }
            ExprKind::Try { value, .. } => self.expr(value, Use::Own),
            ExprKind::Copy(v) => self.expr(v, Use::Read),
            ExprKind::Panic { message, .. } => {
                self.expr(message, Use::Read);
                self.state.unreachable = true;
            }
            ExprKind::Assert { cond, .. } => self.expr(cond, Use::Read),
        }
        if matches!(e.ty, Type::Never) {
            self.state.unreachable = true;
        }
    }

    /// The loop that a `break` or a `continue` with `label` leaves.
    fn target_loop(&mut self, label: &Option<String>) -> Option<&mut LoopCtx> {
        match label {
            None => self.loops.last_mut(),
            Some(name) => self
                .loops
                .iter_mut()
                .rev()
                .find(|l| l.label.as_deref() == Some(name.as_str())),
        }
    }

    /// Subexpressions of a field or index whose base is not a place (a temporary).
    fn subexprs(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Field { base, .. } => self.expr(base, Use::Read),
            ExprKind::Index { base, index } => {
                self.expr(base, Use::Read);
                self.expr(index, Use::Read);
            }
            _ => {}
        }
    }

    fn match_(&mut self, scrutinee: &Expr, mode: Mode, arms: &[wolf_ir::Arm], u: Use) {
        let place = self.place(scrutinee);
        // `match place` borrows the place; `match own place` and `match temporary` move it.
        let (binding, borrow) = match (mode, &place) {
            (Mode::Read, Some(p)) => {
                self.place_parts(scrutinee);
                self.access(p, Access::Read, scrutinee.span);
                (
                    borrowed_kind(false),
                    Some(Borrow {
                        place: p.clone(),
                        mutable: false,
                        span: scrutinee.span,
                        what: "this `match` reads",
                    }),
                )
            }
            (Mode::Mut, Some(p)) => {
                self.place_parts(scrutinee);
                self.access(p, Access::Mutate, scrutinee.span);
                (
                    borrowed_kind(true),
                    Some(Borrow {
                        place: p.clone(),
                        mutable: true,
                        span: scrutinee.span,
                        what: "this `match mut` changes",
                    }),
                )
            }
            (Mode::Mut, None) => {
                self.expr(scrutinee, Use::Read);
                self.report(
                    Diagnostic::new(codes::E0506, "`match mut` needs a mutable place")
                        .primary(scrutinee.span, "this is a temporary value")
                        .help("store it in a `var` first"),
                );
                (borrowed_kind(true), None)
            }
            _ => {
                self.expr(scrutinee, Use::Own);
                (owned_kind(), None)
            }
        };
        if let Some(b) = &borrow {
            self.borrows.push(b.clone());
        }
        let entry = self.state.clone();
        let mut joined = State {
            unreachable: true,
            ..State::default()
        };
        for arm in arms {
            self.state = entry.clone();
            let mut binds = Vec::new();
            bound(&arm.pattern, &mut binds);
            for l in binds {
                let mut f = binding;
                f.name = Some(l);
                self.declare(l, f);
            }
            if let Some(g) = &arm.guard {
                self.expr(g, Use::Read);
            }
            self.block(&arm.body, u);
            let s = std::mem::take(&mut self.state);
            joined = State::join(joined, s);
        }
        if borrow.is_some() {
            self.borrows.pop();
        }
        self.state = joined;
    }

    fn closure(&mut self, e: &Expr, params: &[wolf_ir::Param], body: &Block) {
        // Captured variables: used in the body, declared outside.
        let mut free = Free::default();
        for p in params {
            free.inner.insert(p.local);
        }
        free.block(body);
        let Free {
            used,
            inner,
            self_used,
        } = free;
        let mut captured: Vec<(Root, Type)> = used
            .into_iter()
            .filter(|l| !inner.contains(l))
            .map(|l| {
                let ty = self
                    .p
                    .locals
                    .get(&l)
                    .map(|x| x.ty.clone())
                    .unwrap_or(Type::Void);
                (Root::Local(l), ty)
            })
            .collect();
        if self_used {
            captured.push((Root::SelfValue, Type::Void));
        }
        for (root, ty) in &captured {
            let copy = matches!(root, Root::Local(_)) && self.copy_type(ty);
            let place = Place {
                root: *root,
                segs: Vec::new(),
            };
            if copy {
                self.access(&place, Access::Read, e.span);
                continue;
            }
            let facts = self.facts.get(root).copied();
            if facts.is_some_and(|f| f.borrowed) || *root == Root::SelfValue {
                let name = self.name(*root);
                self.report(
                    Diagnostic::new(
                        codes::E0510,
                        format!("the closure captures the borrowed value `{name}`"),
                    )
                    .primary(e.span, "this closure would take it")
                    .note("closures capture by move, and a borrowed value cannot be moved")
                    .help(format!(
                        "copy it into a local first: `let {name}_copy = {name}.copy()`"
                    )),
                );
            } else {
                self.access(&place, Access::Move, e.span);
            }
        }
        // The body: captured values are immutable and cannot be moved out.
        let saved_facts = self.facts.clone();
        let saved_state = self.state.clone();
        let saved_borrows = std::mem::take(&mut self.borrows);
        let saved_loops = std::mem::take(&mut self.loops);
        let saved_declared = std::mem::take(&mut self.declared);
        self.state = State::default();
        for (root, _) in &captured {
            if let Some(f) = self.facts.get_mut(root) {
                f.captured = true;
            }
        }
        for p in params {
            self.facts
                .insert(Root::Local(p.local), param_facts(p.mode, p.local));
        }
        self.block(body, Use::Own);
        self.facts = saved_facts;
        self.state = saved_state;
        self.borrows = saved_borrows;
        self.loops = saved_loops;
        self.declared = saved_declared;
    }

    // ----- calls ------------------------------------------------------------------------------

    fn call(&mut self, e: &Expr, callee: &Callee, args: &[Arg]) {
        let (recv, sig): (Option<&Expr>, Option<CallSig>) = match callee {
            Callee::Fn(def) => (None, self.fns.get(def).map(|s| (None, s.params.clone()))),
            Callee::Method { method, recv } => (
                Some(recv),
                self.fns
                    .get(method)
                    .map(|s| (s.self_mode, s.params.clone())),
            ),
            Callee::Interface {
                interface,
                name,
                recv,
            } => (
                Some(recv),
                self.iface_fns
                    .get(&(*interface, name.clone()))
                    .map(|s| (s.self_mode, s.params.clone())),
            ),
            Callee::Value(f) => {
                self.expr(f, Use::Read);
                let params = match &f.ty {
                    Type::Fn(ps, _) => Some((None, ps.clone())),
                    _ => None,
                };
                (None, params)
            }
        };
        let (self_mode, params) = sig.unwrap_or((None, Vec::new()));
        // Places used by the call, for the overlap rule (chapter 5, section 7.2).
        let mut used: Vec<(Place, Access, Span)> = Vec::new();
        if let Some(r) = recv {
            self.receiver(r, self_mode.unwrap_or(Mode::Read), &mut used);
        }
        for (i, a) in args.iter().enumerate() {
            let (mode, ty) = params
                .get(i)
                .cloned()
                .unwrap_or((Mode::Read, a.value.ty.clone()));
            self.argument(a, mode, &ty, &mut used);
        }
        self.overlap(&used, e.span);
    }

    fn receiver(&mut self, r: &Expr, mode: Mode, used: &mut Vec<(Place, Access, Span)>) {
        let place = self.place(r);
        match (mode, place) {
            (Mode::Mut, Some(p)) => {
                self.place_parts(r);
                self.access(&p, Access::Mutate, r.span);
                used.push((p, Access::Mutate, r.span));
            }
            (Mode::Mut, None) => {
                self.expr(r, Use::Read);
                self.report(
                    Diagnostic::new(
                        codes::E0506,
                        "a `mut self` method needs a mutable place as the receiver",
                    )
                    .primary(r.span, "this is a temporary value")
                    .help("store it in a `var` first"),
                );
            }
            (Mode::Own, Some(p)) => {
                self.expr(r, Use::Own);
                if !self.copy_type(&r.ty) {
                    used.push((p, Access::Move, r.span));
                }
            }
            (Mode::Read, Some(p)) => {
                self.place_parts(r);
                self.access(&p, Access::Read, r.span);
                used.push((p, Access::Read, r.span));
            }
            (_, None) => self.expr(
                r,
                if mode == Mode::Own {
                    Use::Own
                } else {
                    Use::Read
                },
            ),
        }
    }

    fn argument(&mut self, a: &Arg, mode: Mode, ty: &Type, used: &mut Vec<(Place, Access, Span)>) {
        let place = self.place(&a.value);
        let copy = self.copy_type(&a.value.ty) || self.copy_type(ty);
        // Markers (chapter 5, rule 5.4).
        let want = match mode {
            Mode::Mut => Mode::Mut,
            Mode::Own if place.is_some() && !copy => Mode::Own,
            _ => Mode::Read,
        };
        if a.mode != want {
            let d = match (want, a.mode) {
                (Mode::Mut, _) => Diagnostic::new(codes::E0507, "missing `mut` at the call site")
                    .primary(a.value.span, "this parameter is `mut`")
                    .help("write `mut` before the argument"),
                (Mode::Own, _) => Diagnostic::new(codes::E0507, "missing `own` at the call site")
                    .primary(
                        a.value.span,
                        "this argument is moved into an `own` parameter",
                    )
                    .help(
                        "write `own` before the argument, or pass a copy without `own`: `x.copy()`",
                    ),
                (_, Mode::Mut) => {
                    Diagnostic::new(codes::E0507, "unexpected `mut` at the call site")
                        .primary(a.value.span, "this parameter is not `mut`")
                        .help("remove `mut`")
                }
                _ => Diagnostic::new(codes::E0507, "unexpected `own` at the call site")
                    .primary(
                        a.value.span,
                        if mode == Mode::Own {
                            "a temporary or a copy type needs no `own`"
                        } else {
                            "this parameter is not `own`"
                        },
                    )
                    .help("remove `own`"),
            };
            self.report(d);
        }
        match (mode, place) {
            (Mode::Mut, Some(p)) => {
                self.place_parts(&a.value);
                self.access(&p, Access::Mutate, a.value.span);
                used.push((p, Access::Mutate, a.value.span));
            }
            (Mode::Mut, None) => {
                self.expr(&a.value, Use::Read);
                self.report(
                    Diagnostic::new(codes::E0506, "a `mut` argument must be a mutable place")
                        .primary(a.value.span, "this is a temporary value")
                        .help("store it in a `var` first and pass the variable"),
                );
            }
            (Mode::Own, Some(p)) => {
                self.expr(&a.value, Use::Own);
                used.push((
                    p,
                    if copy { Access::Read } else { Access::Move },
                    a.value.span,
                ));
            }
            (Mode::Read, Some(p)) => {
                self.place_parts(&a.value);
                self.access(&p, Access::Read, a.value.span);
                used.push((p, Access::Read, a.value.span));
            }
            (_, None) => self.expr(
                &a.value,
                if mode == Mode::Own {
                    Use::Own
                } else {
                    Use::Read
                },
            ),
        }
    }

    /// Places of one call must not overlap when one of them is changed or moved (E0508).
    fn overlap(&mut self, used: &[(Place, Access, Span)], _call: Span) {
        for (i, (a, ka, sa)) in used.iter().enumerate() {
            for (b, kb, sb) in used.iter().skip(i + 1) {
                let exclusive = *ka != Access::Read || *kb != Access::Read;
                if exclusive && a.overlaps(b) {
                    let (ta, tb) = (self.place_text(a), self.place_text(b));
                    let how = |k: Access| match k {
                        Access::Mutate => "passed `mut`",
                        Access::Move => "moved",
                        Access::Read => "read",
                    };
                    self.report(
                        Diagnostic::new(codes::E0508, format!("`{ta}` and `{tb}` overlap in one call"))
                            .primary(*sb, format!("`{tb}` is {} here", how(*kb)))
                            .secondary(*sa, format!("`{ta}` is {} here", how(*ka)))
                            .note("while a place is changed or moved by a call, the same call cannot use it elsewhere")
                            .help("pass a copy of one of them, or make two calls"),
                    );
                }
            }
        }
    }
}

fn param_facts(mode: Mode, local: LocalRef) -> Facts {
    Facts {
        mutable: mode != Mode::Read,
        borrowed: mode != Mode::Own,
        captured: false,
        name: Some(local),
    }
}

fn owned_binding(local: LocalRef) -> Facts {
    Facts {
        mutable: false,
        borrowed: false,
        captured: false,
        name: Some(local),
    }
}

fn borrowed_binding(local: LocalRef, mutable: bool) -> Facts {
    Facts {
        mutable,
        borrowed: true,
        captured: false,
        name: Some(local),
    }
}

fn borrowed_kind(mutable: bool) -> Facts {
    Facts {
        mutable,
        borrowed: true,
        captured: false,
        name: None,
    }
}

fn owned_kind() -> Facts {
    Facts {
        mutable: false,
        borrowed: false,
        captured: false,
        name: None,
    }
}

/// The locals a pattern binds.
fn bound(p: &Pattern, out: &mut Vec<LocalRef>) {
    match p {
        Pattern::Bind(l) => {
            if !out.contains(l) {
                out.push(*l);
            }
        }
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => bound(i, out),
        Pattern::Variant { fields, .. } | Pattern::Struct { fields, .. } => {
            fields.iter().for_each(|f| bound(f, out));
        }
        Pattern::Or(alts) => alts.iter().for_each(|a| bound(a, out)),
        _ => {}
    }
}

// ----- free variables of closures ---------------------------------------------------------------

/// The variables a closure body uses, and those it declares itself.
#[derive(Default)]
struct Free {
    used: BTreeSet<LocalRef>,
    inner: BTreeSet<LocalRef>,
    self_used: bool,
}

impl Free {
    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            match s {
                Stmt::Let { local, value, .. } => {
                    self.expr(value);
                    self.inner.insert(*local);
                }
                Stmt::LetPattern { pattern, value, .. } => {
                    self.expr(value);
                    let mut b = Vec::new();
                    bound(pattern, &mut b);
                    self.inner.extend(b);
                }
                Stmt::Assign { target, value, .. } => {
                    self.expr(target);
                    self.expr(value);
                }
                Stmt::While { cond, body, .. } => {
                    self.expr(cond);
                    self.block(body);
                }
                Stmt::ForList {
                    var, list, body, ..
                } => {
                    self.expr(list);
                    self.inner.insert(*var);
                    self.block(body);
                }
                Stmt::ForRange {
                    var,
                    start,
                    end,
                    body,
                    ..
                } => {
                    self.expr(start);
                    self.expr(end);
                    self.inner.insert(*var);
                    self.block(body);
                }
                Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Local(l) => {
                self.used.insert(*l);
            }
            ExprKind::SelfValue => self.self_used = true,
            ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Method { recv, .. } | Callee::Interface { recv, .. } => self.expr(recv),
                    Callee::Value(f) => self.expr(f),
                    Callee::Fn(_) => {}
                }
                for a in args {
                    self.expr(&a.value);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::Copy(base) => self.expr(base),
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Slice {
                base, start, end, ..
            } => {
                self.expr(base);
                for b in [start, end].into_iter().flatten() {
                    self.expr(b);
                }
            }
            ExprKind::Struct { fields, base, .. } => {
                fields.iter().for_each(|(_, v)| self.expr(v));
                if let Some(b) = base {
                    self.expr(b);
                }
            }
            ExprKind::Interpolate(parts) => parts.iter().for_each(|p| self.expr(p)),
            ExprKind::Loop { body, .. } => self.block(body),
            ExprKind::Break { value: Some(v), .. } => self.expr(v),
            ExprKind::Variant { values, .. } | ExprKind::List(values) => {
                values.iter().for_each(|v| self.expr(v));
            }
            ExprKind::Map(entries) => entries.iter().for_each(|(k, v)| {
                self.expr(k);
                self.expr(v);
            }),
            ExprKind::Some(v) | ExprKind::Ok(v) | ExprKind::Err(v) => self.expr(v),
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Closure { params, body } => {
                for p in params {
                    self.inner.insert(p.local);
                }
                self.block(body);
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
                scrutinee, arms, ..
            } => {
                self.expr(scrutinee);
                for arm in arms {
                    let mut b = Vec::new();
                    bound(&arm.pattern, &mut b);
                    self.inner.extend(b);
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.block(&arm.body);
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
