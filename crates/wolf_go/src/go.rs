//! A small tree of Go and its printer.
//!
//! The backend builds this tree from the IR and then prints it (spec chapter 8, section 1). The
//! printer produces what `gofmt` produces for the same tree: tabs, aligned struct fields and
//! constant specs, and the spacing of binary operators that `go/printer` derives from operator
//! precedence and nesting depth. Expressions are always printed on one line; only blocks span
//! several lines.

use std::collections::BTreeMap;

/// A Go type.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// A named type, optionally from another package (by import path), with type arguments.
    Name(Option<String>, String, Vec<Type>),
    /// `[]T`
    Slice(Box<Type>),
    /// `*T`
    Ptr(Box<Type>),
    /// `map[K]V`
    Map(Box<Type>, Box<Type>),
    /// `func(A, B) (R, S)`
    Func(Vec<Type>, Vec<Type>),
    /// `struct{}`
    EmptyStruct,
    /// `interface{ A; B }`, for constraints.
    Union(Vec<Type>),
}

impl Type {
    /// A predeclared or local type without arguments.
    pub fn name(name: &str) -> Type {
        Type::Name(None, name.to_string(), Vec::new())
    }
}

/// A Go expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A name.
    Ident(String),
    /// `pkg.Name`, with the package given by its import path.
    Qual(String, String),
    /// A literal as written: numbers, `true`, `false`, `nil`.
    Lit(String),
    /// A string literal.
    Str(String),
    /// `f[T](args)`, with `...` after the last argument if the flag is set.
    Call(Box<Expr>, Vec<Type>, Vec<Expr>, bool),
    /// `f[T]` as a value.
    Inst(Box<Expr>, Vec<Type>),
    /// `a.b`
    Sel(Box<Expr>, String),
    /// `a[b]`
    Index(Box<Expr>, Box<Expr>),
    /// `T{k: v, w}`
    Composite(Type, Vec<(Option<String>, Expr)>),
    /// `func(a A) R { ... }`
    FuncLit(Vec<(String, Type)>, Vec<Type>, Vec<Stmt>),
    /// `a op b`
    Binary(Box<Expr>, &'static str, Box<Expr>),
    /// `op a` for `-`, `!`, `^` and `&`.
    Unary(&'static str, Box<Expr>),
    /// `*a`
    Star(Box<Expr>),
    /// `a.(T)`
    Assert(Box<Expr>, Type),
    /// A type used as an expression: the callee of a conversion, the base of a method
    /// expression.
    Type(Type),
}

/// A Go statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `a, b := x, y`
    Define(Vec<String>, Vec<Expr>),
    /// `var x T` or `var x T = e`
    Var(String, Type, Option<Expr>),
    /// `a = b`, `a += b`...
    Assign(Vec<Expr>, &'static str, Vec<Expr>),
    /// `e`
    Expr(Expr),
    /// `e++` or `e--`
    Inc(Expr, &'static str),
    /// `if init; cond { ... } else { ... }`; an `else` holding a single `if` prints as `else if`.
    If(Option<Box<Stmt>>, Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    /// `for init; cond; post { ... }`, `for cond { ... }` or `for { ... }`.
    For(
        Option<Box<Stmt>>,
        Option<Expr>,
        Option<Box<Stmt>>,
        Vec<Stmt>,
    ),
    /// `for k, v := range e { ... }`; missing names are `_` or left out.
    Range(Option<String>, Option<String>, Expr, Vec<Stmt>),
    /// `switch tag { case a, b: ... default: ... }`
    Switch(Option<Expr>, Vec<(Option<Vec<Expr>>, Vec<Stmt>)>),
    /// `switch x := e.(type) { case *A: ... }`
    TypeSwitch(Option<String>, Expr, Vec<(Option<Vec<Type>>, Vec<Stmt>)>),
    /// `return a, b`
    Return(Vec<Expr>),
    /// `break` or `break label`
    Break(Option<String>),
    /// `continue` or `continue label`
    Continue(Option<String>),
    /// `label:` before a statement.
    Labeled(String, Box<Stmt>),
    /// `defer f(x)`
    Defer(Expr),
}

/// A field of a struct.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// Doc comment lines.
    pub docs: Vec<String>,
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: Type,
}

/// An element of an interface: a method or an embedded type.
#[derive(Clone, Debug, PartialEq)]
pub enum IfaceElem {
    /// An embedded interface, whose methods the interface has too.
    Embed(Type),
    /// A method: docs, name, parameters, results.
    Method(Vec<String>, String, Vec<(String, Type)>, Vec<Type>),
}

/// A function or method declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    /// Doc comment lines.
    pub docs: Vec<String>,
    /// The receiver of a method: name and type.
    pub recv: Option<(String, Type)>,
    /// Its name.
    pub name: String,
    /// Type parameters with constraints.
    pub tparams: Vec<(String, Type)>,
    /// Parameters.
    pub params: Vec<(String, Type)>,
    /// Results.
    pub results: Vec<Type>,
    /// The body.
    pub body: Vec<Stmt>,
}

/// A constant spec: docs, name, type and value.
pub type ConstSpec = (Vec<String>, String, Option<Type>, Option<Expr>);

/// A top-level declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum Decl {
    /// A function or method.
    Func(Func),
    /// `type Name[T any] struct { ... }`
    Struct {
        /// Doc comment lines.
        docs: Vec<String>,
        /// Its name.
        name: String,
        /// Type parameters with constraints.
        tparams: Vec<(String, Type)>,
        /// Fields.
        fields: Vec<Field>,
    },
    /// `type Name[T any] interface { ... }`
    Interface {
        /// Doc comment lines.
        docs: Vec<String>,
        /// Its name.
        name: String,
        /// Type parameters with constraints.
        tparams: Vec<(String, Type)>,
        /// Methods and embedded interfaces.
        elems: Vec<IfaceElem>,
    },
    /// `type Name T` (a defined type) or `type Name = T` (an alias).
    TypeDef {
        /// Doc comment lines.
        docs: Vec<String>,
        /// Its name.
        name: String,
        /// Type parameters with constraints.
        tparams: Vec<(String, Type)>,
        /// The underlying type.
        ty: Type,
        /// `=`
        alias: bool,
    },
    /// `const ( ... )`.
    Consts(Vec<ConstSpec>),
}

/// A Go file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// The header comment, without `//`.
    pub header: String,
    /// Package doc comment lines.
    pub docs: Vec<String>,
    /// The package name.
    pub package: String,
    /// The import path of this package, so that its own names are not qualified.
    pub path: String,
    /// Imports: path to the name used in this file.
    pub imports: BTreeMap<String, String>,
    /// The import path prefix of the packages of `native/go/`: `<module>/native/`.
    pub native: String,
    /// The declarations.
    pub decls: Vec<Decl>,
}

/// Prints a file.
pub fn print(file: &File) -> String {
    let p = Printer {
        imports: &file.imports,
        own: &file.path,
    };
    let mut out = String::new();
    out.push_str(&format!("// {}\n\n", file.header));
    for d in &file.docs {
        out.push_str(&comment(d));
        out.push('\n');
    }
    out.push_str(&format!("package {}\n", file.package));
    let imports: Vec<String> = file
        .imports
        .iter()
        .map(|(path, name)| {
            // A package of `native/go/` is always named: its `package` clause may differ from
            // its directory (spec chapter 8, section 8a).
            let native = path.starts_with(&file.native);
            if path.rsplit('/').next() == Some(name.as_str()) && !native {
                quote(path)
            } else {
                format!("{name} {}", quote(path))
            }
        })
        .collect();
    match imports.as_slice() {
        [] => {}
        [one] => out.push_str(&format!("\nimport {one}\n")),
        many => {
            out.push_str("\nimport (\n");
            for i in many {
                out.push_str(&format!("\t{i}\n"));
            }
            out.push_str(")\n");
        }
    }
    for d in &file.decls {
        out.push('\n');
        p.decl(&mut out, d);
    }
    out
}

/// A comment line: `//` and the text, without a trailing space for empty lines.
fn comment(text: &str) -> String {
    let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
    if text.is_empty() {
        "//".to_string()
    } else {
        format!("// {text}")
    }
}

/// A Go string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Precedence of a binary operator, as in `go/token`.
fn prec(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
        "+" | "-" | "|" | "^" => 4,
        _ => 5,
    }
}

const UNARY: u8 = 6;
const HIGHEST: u8 = 7;

/// `walkBinary` of `go/printer`.
fn walk(e: &Expr) -> (bool, bool, u8) {
    let Expr::Binary(l, op, r) = e else {
        return (false, false, 0);
    };
    let p = prec(op);
    let (mut has4, mut has5, mut problem) = (p == 4, p == 5, 0);
    if let Expr::Binary(_, lop, _) = l.as_ref() {
        if prec(lop) >= p {
            let (a, b, m) = walk(l);
            has4 |= a;
            has5 |= b;
            problem = problem.max(m);
        }
    }
    match r.as_ref() {
        Expr::Binary(_, rop, _) if prec(rop) > p => {
            let (a, b, m) = walk(r);
            has4 |= a;
            has5 |= b;
            problem = problem.max(m);
        }
        Expr::Star(_) if *op == "/" => problem = 5,
        Expr::Unary(u, _) => match format!("{op}{u}").as_str() {
            "/*" | "&&" | "&^" => problem = 5,
            "++" | "--" => problem = problem.max(4),
            _ => {}
        },
        _ => {}
    }
    (has4, has5, problem)
}

fn cutoff(e: &Expr, depth: usize) -> u8 {
    let (has4, has5, problem) = walk(e);
    if problem > 0 {
        return problem + 1;
    }
    if has4 && has5 {
        return if depth == 1 { 5 } else { 4 };
    }
    if depth == 1 { 6 } else { 4 }
}

fn diff_prec(e: &Expr, p: u8) -> usize {
    match e {
        Expr::Binary(_, op, _) if prec(op) == p => 0,
        _ => 1,
    }
}

fn reduce(depth: usize) -> usize {
    depth.saturating_sub(1).max(1)
}

fn tabs(n: usize) -> String {
    "\t".repeat(n)
}

/// Aligns rows of cells the way `text/tabwriter` does for `gofmt`: every cell but the last of a
/// row is padded to the widest cell of its column among the neighbouring rows that have it.
fn align(rows: &[Vec<String>]) -> Vec<String> {
    let width = |s: &str| s.chars().count();
    let mut padded: Vec<Vec<String>> = rows.to_vec();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    for c in 0..columns {
        let mut i = 0;
        while i < rows.len() {
            let has = |r: &Vec<String>| r.len() > c + 1;
            if !rows.get(i).is_some_and(has) {
                i += 1;
                continue;
            }
            let start = i;
            while rows.get(i).is_some_and(has) {
                i += 1;
            }
            let w = (start..i)
                .filter_map(|r| rows.get(r).and_then(|row| row.get(c)))
                .map(|s| width(s))
                .max()
                .unwrap_or(0);
            for r in start..i {
                if let Some(cell) = padded.get_mut(r).and_then(|row| row.get_mut(c)) {
                    let pad = w + 1 - width(cell);
                    cell.push_str(&" ".repeat(pad));
                }
            }
        }
    }
    padded.into_iter().map(|r| r.concat()).collect()
}

struct Printer<'f> {
    imports: &'f BTreeMap<String, String>,
    own: &'f str,
}

impl Printer<'_> {
    fn qual(&self, path: &Option<String>, name: &str) -> String {
        match path {
            Some(p) if p != self.own => {
                let pkg = self
                    .imports
                    .get(p)
                    .cloned()
                    .unwrap_or_else(|| p.rsplit('/').next().unwrap_or(p).to_string());
                format!("{pkg}.{name}")
            }
            _ => name.to_string(),
        }
    }

    fn ty(&self, t: &Type) -> String {
        match t {
            Type::Name(path, name, args) => {
                let base = self.qual(path, name);
                if args.is_empty() {
                    base
                } else {
                    format!("{base}[{}]", self.types(args))
                }
            }
            Type::Slice(t) => format!("[]{}", self.ty(t)),
            Type::Ptr(t) => format!("*{}", self.ty(t)),
            Type::Map(k, v) => format!("map[{}]{}", self.ty(k), self.ty(v)),
            Type::Func(params, results) => {
                format!("func({}){}", self.types(params), self.results(results))
            }
            Type::EmptyStruct => "struct{}".to_string(),
            // A constraint of type parameters, which are declared at the top level: one element
            // per line, as gofmt writes it.
            Type::Union(parts) => {
                let parts: Vec<String> = parts
                    .iter()
                    .map(|p| format!("\t{}\n", self.ty(p)))
                    .collect();
                format!("interface {{\n{}}}", parts.concat())
            }
        }
    }

    fn types(&self, ts: &[Type]) -> String {
        ts.iter().map(|t| self.ty(t)).collect::<Vec<_>>().join(", ")
    }

    fn results(&self, results: &[Type]) -> String {
        match results {
            [] => String::new(),
            [one] => format!(" {}", self.ty(one)),
            many => format!(" ({})", self.types(many)),
        }
    }

    fn tparams(&self, tps: &[(String, Type)]) -> String {
        if tps.is_empty() {
            return String::new();
        }
        let list: Vec<String> = tps
            .iter()
            .map(|(n, c)| format!("{n} {}", self.ty(c)))
            .collect();
        format!("[{}]", list.join(", "))
    }

    fn params(&self, ps: &[(String, Type)]) -> String {
        ps.iter()
            .map(|(n, t)| format!("{n} {}", self.ty(t)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr0(&self, e: &Expr, depth: usize, ind: usize) -> String {
        self.expr1(e, 0, depth, ind)
    }

    fn expr1(&self, e: &Expr, prec1: u8, depth: usize, ind: usize) -> String {
        match e {
            Expr::Ident(n) | Expr::Lit(n) => n.clone(),
            Expr::Qual(path, name) => self.qual(&Some(path.clone()), name),
            Expr::Str(s) => quote(s),
            Expr::Binary(l, op, r) => {
                let p = prec(op);
                if p < prec1 {
                    let inner = self.expr0(e, reduce(depth), ind);
                    return format!("({inner})");
                }
                let blank = p < cutoff(e, depth);
                let left = self.expr1(l, p, depth + diff_prec(l, p), ind);
                let right = self.expr1(r, p + 1, depth + 1, ind);
                if blank {
                    format!("{left} {op} {right}")
                } else {
                    format!("{left}{op}{right}")
                }
            }
            Expr::Unary(_, x) | Expr::Star(x) if UNARY < prec1 => {
                let sym = if let Expr::Unary(op, _) = e { op } else { "*" };
                format!("({sym}{})", self.expr0(x, 1, ind))
            }
            Expr::Unary(op, x) => format!("{op}{}", self.expr1(x, UNARY, depth, ind)),
            Expr::Star(x) => format!("*{}", self.expr1(x, UNARY, depth, ind)),
            Expr::Call(f, targs, args, spread) => {
                let depth = if args.len() > 1 { depth + 1 } else { depth };
                let mut s = self.expr1(f, HIGHEST, depth, ind);
                if !targs.is_empty() {
                    s.push_str(&format!("[{}]", self.types(targs)));
                }
                let list: Vec<String> = args.iter().map(|a| self.expr0(a, depth, ind)).collect();
                s.push('(');
                s.push_str(&list.join(", "));
                if *spread {
                    s.push_str("...");
                }
                s.push(')');
                s
            }
            Expr::Inst(f, targs) => {
                format!(
                    "{}[{}]",
                    self.expr1(f, HIGHEST, depth, ind),
                    self.types(targs)
                )
            }
            Expr::Sel(b, n) => format!("{}.{n}", self.expr1(b, HIGHEST, depth, ind)),
            Expr::Index(b, i) => format!(
                "{}[{}]",
                self.expr1(b, HIGHEST, 1, ind),
                self.expr0(i, depth + 1, ind)
            ),
            Expr::Composite(t, elems) => format!("{}{}", self.ty(t), self.elems(elems, ind)),
            Expr::FuncLit(params, results, body) => {
                let head = format!("func({}){}", self.params(params), self.results(results));
                if body.is_empty() {
                    return format!("{head} {{}}");
                }
                let mut s = format!("{head} {{\n");
                for st in body {
                    self.stmt(&mut s, st, ind + 1);
                }
                s.push_str(&tabs(ind));
                s.push('}');
                s
            }
            Expr::Assert(x, t) => {
                format!("{}.({})", self.expr1(x, HIGHEST, depth, ind), self.ty(t))
            }
            Expr::Type(t) => match t {
                Type::Ptr(_) | Type::Func(..) => format!("({})", self.ty(t)),
                _ => self.ty(t),
            },
        }
    }

    fn elems(&self, elems: &[(Option<String>, Expr)], ind: usize) -> String {
        let list: Vec<String> = elems
            .iter()
            .map(|(k, v)| match k {
                Some(k) => format!("{k}: {}", self.expr0(v, 1, ind)),
                None => self.expr0(v, 1, ind),
            })
            .collect();
        format!("{{{}}}", list.join(", "))
    }

    fn list(&self, es: &[Expr], depth: usize, ind: usize) -> String {
        es.iter()
            .map(|e| self.expr0(e, depth, ind))
            .collect::<Vec<_>>()
            .join(", ")
    }

    // ----- statements -------------------------------------------------------------------------

    fn block(&self, out: &mut String, stmts: &[Stmt], ind: usize) {
        for s in stmts {
            self.stmt(out, s, ind);
        }
    }

    /// A statement without its indentation and line feed, for `if` and `for` headers.
    fn simple(&self, s: &Stmt, ind: usize) -> String {
        match s {
            Stmt::Define(names, values) => {
                let depth = if names.len() > 1 && values.len() > 1 {
                    2
                } else {
                    1
                };
                format!("{} := {}", names.join(", "), self.list(values, depth, ind))
            }
            Stmt::Var(n, t, v) => match v {
                Some(v) => format!("var {n} {} = {}", self.ty(t), self.expr0(v, 1, ind)),
                None => format!("var {n} {}", self.ty(t)),
            },
            Stmt::Assign(lhs, op, rhs) => {
                let depth = if lhs.len() > 1 && rhs.len() > 1 { 2 } else { 1 };
                format!(
                    "{} {op} {}",
                    self.list(lhs, depth, ind),
                    self.list(rhs, depth, ind)
                )
            }
            Stmt::Expr(e) => self.expr0(e, 1, ind),
            Stmt::Inc(e, op) => format!("{}{op}", self.expr0(e, 1, ind)),
            Stmt::Return(vs) if vs.is_empty() => "return".to_string(),
            Stmt::Return(vs) => format!("return {}", self.list(vs, 1, ind)),
            Stmt::Break(None) => "break".to_string(),
            Stmt::Break(Some(l)) => format!("break {l}"),
            Stmt::Continue(None) => "continue".to_string(),
            Stmt::Continue(Some(l)) => format!("continue {l}"),
            Stmt::Defer(e) => format!("defer {}", self.expr0(e, 1, ind)),
            _ => String::new(),
        }
    }

    fn stmt(&self, out: &mut String, s: &Stmt, ind: usize) {
        let t = tabs(ind);
        match s {
            Stmt::If(..) => {
                out.push_str(&t);
                self.if_(out, s, ind);
                out.push('\n');
            }
            Stmt::For(init, cond, post, body) => {
                let head = match (init, cond, post) {
                    (None, None, None) => "for {".to_string(),
                    (None, Some(c), None) => format!("for {} {{", self.expr0(c, 1, ind)),
                    _ => format!(
                        "for {}; {}; {} {{",
                        init.as_ref().map_or(String::new(), |s| self.simple(s, ind)),
                        cond.as_ref()
                            .map_or(String::new(), |c| self.expr0(c, 1, ind)),
                        post.as_ref().map_or(String::new(), |s| self.simple(s, ind)),
                    ),
                };
                out.push_str(&format!("{t}{head}\n"));
                self.block(out, body, ind + 1);
                out.push_str(&format!("{t}}}\n"));
            }
            Stmt::Range(k, v, e, body) => {
                let x = self.expr0(e, 1, ind);
                let head = match (k, v) {
                    (None, None) => format!("for range {x} {{"),
                    (Some(k), None) => format!("for {k} := range {x} {{"),
                    (k, Some(v)) => {
                        format!("for {}, {v} := range {x} {{", k.as_deref().unwrap_or("_"))
                    }
                };
                out.push_str(&format!("{t}{head}\n"));
                self.block(out, body, ind + 1);
                out.push_str(&format!("{t}}}\n"));
            }
            Stmt::Switch(tag, cases) => {
                match tag {
                    Some(tag) => {
                        out.push_str(&format!("{t}switch {} {{\n", self.expr0(tag, 1, ind)));
                    }
                    None => out.push_str(&format!("{t}switch {{\n")),
                }
                for (labels, body) in cases {
                    match labels {
                        Some(l) => out.push_str(&format!("{t}case {}:\n", self.list(l, 1, ind))),
                        None => out.push_str(&format!("{t}default:\n")),
                    }
                    self.block(out, body, ind + 1);
                }
                out.push_str(&format!("{t}}}\n"));
            }
            Stmt::TypeSwitch(bind, e, cases) => {
                let x = self.expr1(e, HIGHEST, 1, ind);
                match bind {
                    Some(b) => out.push_str(&format!("{t}switch {b} := {x}.(type) {{\n")),
                    None => out.push_str(&format!("{t}switch {x}.(type) {{\n")),
                }
                for (labels, body) in cases {
                    match labels {
                        Some(l) => out.push_str(&format!("{t}case {}:\n", self.types(l))),
                        None => out.push_str(&format!("{t}default:\n")),
                    }
                    self.block(out, body, ind + 1);
                }
                out.push_str(&format!("{t}}}\n"));
            }
            Stmt::Labeled(label, inner) => {
                out.push_str(&format!("{}{label}:\n", tabs(ind.saturating_sub(1))));
                self.stmt(out, inner, ind);
            }
            other => {
                let text = self.simple(other, ind);
                out.push_str(&format!("{t}{text}\n"));
            }
        }
    }

    /// `if ... { ... } else ...` without the leading indentation and the final line feed.
    fn if_(&self, out: &mut String, s: &Stmt, ind: usize) {
        let Stmt::If(init, cond, then, otherwise) = s else {
            return;
        };
        let c = self.expr0(cond, 1, ind);
        match init {
            Some(i) => out.push_str(&format!("if {}; {c} {{\n", self.simple(i, ind))),
            None => out.push_str(&format!("if {c} {{\n")),
        }
        self.block(out, then, ind + 1);
        out.push_str(&tabs(ind));
        out.push('}');
        match otherwise.as_deref() {
            None => {}
            Some([inner @ Stmt::If(..)]) => {
                out.push_str(" else ");
                self.if_(out, inner, ind);
            }
            Some(body) => {
                out.push_str(" else {\n");
                self.block(out, body, ind + 1);
                out.push_str(&tabs(ind));
                out.push('}');
            }
        }
    }

    // ----- declarations -----------------------------------------------------------------------

    fn docs(&self, out: &mut String, docs: &[String], ind: usize) {
        for d in docs {
            out.push_str(&format!("{}{}\n", tabs(ind), comment(d)));
        }
    }

    fn decl(&self, out: &mut String, d: &Decl) {
        match d {
            Decl::Func(f) => {
                self.docs(out, &f.docs, 0);
                let recv = match &f.recv {
                    Some((n, t)) if n.is_empty() => format!("({}) ", self.ty(t)),
                    Some((n, t)) => format!("({n} {}) ", self.ty(t)),
                    None => String::new(),
                };
                let head = format!(
                    "func {recv}{}{}({}){}",
                    f.name,
                    self.tparams(&f.tparams),
                    self.params(&f.params),
                    self.results(&f.results)
                );
                if f.body.is_empty() {
                    out.push_str(&format!("{head} {{}}\n"));
                    return;
                }
                out.push_str(&format!("{head} {{\n"));
                self.block(out, &f.body, 1);
                out.push_str("}\n");
            }
            Decl::Struct {
                docs,
                name,
                tparams,
                fields,
            } => {
                self.docs(out, docs, 0);
                let head = format!("type {name}{} struct", self.tparams(tparams));
                if fields.is_empty() {
                    out.push_str(&format!("{head}{{}}\n"));
                    return;
                }
                out.push_str(&format!("{head} {{\n"));
                let mut rows: Vec<Vec<String>> = Vec::new();
                let mut kinds: Vec<bool> = Vec::new();
                for f in fields {
                    for d in &f.docs {
                        rows.push(vec![comment(d)]);
                        kinds.push(false);
                    }
                    rows.push(vec![f.name.clone(), self.ty(&f.ty)]);
                    kinds.push(true);
                }
                for line in align(&rows) {
                    out.push_str(&format!("\t{line}\n"));
                }
                out.push_str("}\n");
            }
            Decl::Interface {
                docs,
                name,
                tparams,
                elems,
            } => {
                self.docs(out, docs, 0);
                let head = format!("type {name}{} interface", self.tparams(tparams));
                if elems.is_empty() {
                    out.push_str(&format!("{head}{{}}\n"));
                    return;
                }
                out.push_str(&format!("{head} {{\n"));
                for e in elems {
                    match e {
                        IfaceElem::Embed(t) => out.push_str(&format!("\t{}\n", self.ty(t))),
                        IfaceElem::Method(docs, n, params, results) => {
                            self.docs(out, docs, 1);
                            out.push_str(&format!(
                                "\t{n}({}){}\n",
                                self.params(params),
                                self.results(results)
                            ));
                        }
                    }
                }
                out.push_str("}\n");
            }
            Decl::TypeDef {
                docs,
                name,
                tparams,
                ty,
                alias,
            } => {
                self.docs(out, docs, 0);
                let eq = if *alias { "= " } else { "" };
                out.push_str(&format!(
                    "type {name}{} {eq}{}\n",
                    self.tparams(tparams),
                    self.ty(ty)
                ));
            }
            Decl::Consts(specs) => {
                out.push_str("const (\n");
                let mut rows: Vec<Vec<String>> = Vec::new();
                for (docs, name, ty, value) in specs {
                    for d in docs {
                        rows.push(vec![comment(d)]);
                    }
                    let mut row = vec![name.clone()];
                    if let Some(t) = ty {
                        row.push(self.ty(t));
                    }
                    if let Some(v) = value {
                        row.push(format!("= {}", self.expr0(v, 1, 1)));
                    }
                    rows.push(row);
                }
                for line in align(&rows) {
                    out.push_str(&format!("\t{line}\n"));
                }
                out.push_str(")\n");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: &str) -> Box<Expr> {
        Box::new(Expr::Ident(n.to_string()))
    }

    fn bin(a: Box<Expr>, op: &'static str, b: Box<Expr>) -> Box<Expr> {
        Box::new(Expr::Binary(a, op, b))
    }

    fn show(e: &Expr) -> String {
        let imports = BTreeMap::new();
        let p = Printer {
            imports: &imports,
            own: "",
        };
        p.expr0(e, 1, 0)
    }

    #[test]
    fn binary_spacing_follows_gofmt() {
        // a + b*c
        let e = bin(id("a"), "+", bin(id("b"), "*", id("c")));
        assert_eq!(show(&e), "a + b*c");
        // (a + b) * c
        let e = bin(bin(id("a"), "+", id("b")), "*", id("c"));
        assert_eq!(show(&e), "(a + b) * c");
        // a+b > c && d
        let e = bin(bin(bin(id("a"), "+", id("b")), ">", id("c")), "&&", id("d"));
        assert_eq!(show(&e), "a+b > c && d");
        // g(a+b, c)
        let e = Expr::Call(
            id("g"),
            Vec::new(),
            vec![*bin(id("a"), "+", id("b")), *id("c")],
            false,
        );
        assert_eq!(show(&e), "g(a+b, c)");
        // f(a + b)
        let e = Expr::Call(
            id("f"),
            Vec::new(),
            vec![*bin(id("a"), "+", id("b"))],
            false,
        );
        assert_eq!(show(&e), "f(a + b)");
        // a - (b - c)
        let e = bin(id("a"), "-", bin(id("b"), "-", id("c")));
        assert_eq!(show(&e), "a - (b - c)");
    }

    #[test]
    fn fields_are_aligned_in_groups() {
        let rows = vec![
            vec!["ID".to_string(), "int64".to_string()],
            vec!["// The title.".to_string()],
            vec!["Title".to_string(), "string".to_string()],
            vec!["priority".to_string(), "Priority".to_string()],
        ];
        assert_eq!(
            align(&rows),
            vec![
                "ID int64",
                "// The title.",
                "Title    string",
                "priority Priority"
            ]
        );
    }

    #[test]
    fn strings_are_quoted() {
        assert_eq!(quote("a\"b\\c\n😀"), "\"a\\\"b\\\\c\\n😀\"");
    }
}
