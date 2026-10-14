//! A small tree of Rust and its printer.
//!
//! The backend builds this tree from the IR and then prints it (spec chapter 8, section 1). The
//! printer follows `rustfmt` with its default settings: four spaces, a width of 100, and the
//! "small heuristics" that decide when a call, a chain, a struct literal or an array is written
//! on one line (`fn_call_width` 60, `chain_width` 60, `struct_lit_width` 18, `array_width` 60,
//! `single_line_if_else_max_width` 50).

const MAX_WIDTH: usize = 100;
const CALL_WIDTH: usize = 60;
const CHAIN_WIDTH: usize = 60;
const STRUCT_LIT_WIDTH: usize = 18;
const STRUCT_VARIANT_WIDTH: usize = 35;
const ARRAY_WIDTH: usize = 60;
const IF_ELSE_WIDTH: usize = 50;

/// A Rust type.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// A path with generic arguments: `i64`, `Vec<T>`, `wolf_rt::Map<K, V>`.
    Path(String, Vec<Type>),
    /// `&T`
    Ref(Box<Type>),
    /// `&mut T`
    RefMut(Box<Type>),
    /// `()`
    Unit,
    /// `dyn Fn(A, B) -> R`
    DynFn(Vec<Type>, Box<Type>),
    /// `dyn Trait<A>`
    Dyn(String, Vec<Type>),
    /// `[T]`
    Slice(Box<Type>),
    /// `!`
    Never,
}

impl Type {
    /// A type without arguments.
    pub fn name(name: &str) -> Type {
        Type::Path(name.to_string(), Vec::new())
    }
}

/// A pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum Pat {
    /// `_`
    Wild,
    /// `x` or `mut x`
    Bind(String, bool),
    /// A literal or a path: `3`, `"a"`, `None`, `Ordering::Less`.
    Lit(String),
    /// `Path(a, b)`
    Tuple(String, Vec<Pat>),
    /// `Path { a: x, b }`, with `..` if the flag is set.
    Struct(String, Vec<(String, Pat)>, bool),
    /// `a | b`
    Or(Vec<Pat>),
    /// `&x`
    Ref(Box<Pat>),
}

/// An arm of a `match`.
#[derive(Clone, Debug, PartialEq)]
pub struct Arm {
    /// The pattern.
    pub pat: Pat,
    /// The guard.
    pub guard: Option<Expr>,
    /// The body.
    pub body: Expr,
}

/// A block: statements and an optional final expression.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Block {
    /// The statements.
    pub stmts: Vec<Stmt>,
    /// The value.
    pub tail: Option<Box<Expr>>,
}

/// A Rust expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A path or a name: `x`, `wolf_rt::print`, `Shape::Circle`.
    Path(String),
    /// A literal as written.
    Lit(String),
    /// A string literal.
    Str(String),
    /// `f(args)`
    Call(Box<Expr>, Vec<Expr>),
    /// `recv.name::<T>(args)`
    Method(Box<Expr>, String, Vec<Type>, Vec<Expr>),
    /// `name!(args)`; the first argument of `format!` and friends is a format string.
    Macro(String, Vec<Expr>),
    /// `base.name`
    Field(Box<Expr>, String),
    /// `op x` for `-`, `!`, `*`, `&` and `&mut `.
    Unary(&'static str, Box<Expr>),
    /// `a op b`, also assignments such as `a += b`.
    Binary(Box<Expr>, &'static str, Box<Expr>),
    /// `Path { a: x, b }`
    Struct(String, Vec<(String, Expr)>),
    /// `()` or `(a, b)`
    Tuple(Vec<Expr>),
    /// `[a, b]`
    Array(Vec<Expr>),
    /// `|a, b| body` (`move` if the flag is set), with optional parameter types.
    Closure(Vec<(Pat, Option<Type>)>, Box<Expr>, bool),
    /// `{ ... }`
    Block(Block),
    /// `if c { ... } else ...`
    If(Box<Expr>, Block, Option<Box<Expr>>),
    /// `match x { ... }`
    Match(Box<Expr>, Vec<Arm>),
    /// `if let pat = x { ... } else ...`
    IfLet(Pat, Box<Expr>, Block, Option<Box<Expr>>),
    /// `return x`
    Return(Option<Box<Expr>>),
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `x?`
    Try(Box<Expr>),
    /// `x as T`
    Cast(Box<Expr>, Type),
    /// `a..b` or `a..=b`
    Range(Box<Expr>, Box<Expr>, bool),
    /// `(x)`
    Paren(Box<Expr>),
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `let pat: T = e;`
    Let(Pat, Option<Type>, Option<Expr>),
    /// `e;`; block-like expressions (`if`, `match`) print without the semicolon.
    Expr(Expr),
    /// `while c { ... }`
    While(Expr, Block),
    /// `loop { ... }`
    Loop(Block),
    /// `for pat in e { ... }`
    For(Pat, Expr, Block),
    /// `while let pat = e { ... }`
    WhileLet(Pat, Expr, Block),
}

/// A function.
#[derive(Clone, Debug, PartialEq)]
pub struct Fn {
    /// Doc comment lines.
    pub docs: Vec<String>,
    /// Attributes such as `#[test]`.
    pub attrs: Vec<String>,
    /// `pub ` or nothing.
    pub vis: String,
    /// Its name.
    pub name: String,
    /// Type parameters with their bounds.
    pub generics: Vec<(String, Vec<String>)>,
    /// `&self`, `&mut self`, `self` or `mut self`.
    pub self_param: Option<String>,
    /// Parameters: pattern and type.
    pub params: Vec<(Pat, Type)>,
    /// The result type.
    pub ret: Option<Type>,
    /// The body; `None` for a signature of a trait.
    pub body: Option<Block>,
}

/// A variant of an enum.
#[derive(Clone, Debug, PartialEq)]
pub enum Variant {
    /// `Name`
    Unit,
    /// `Name(A, B)`
    Tuple(Vec<Type>),
    /// `Name { a: A }`
    Struct(Vec<(String, Type)>),
}

/// A top-level item.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// `use path;`
    Use(String),
    /// `mod name;`
    Mod(String, String),
    /// A function.
    Fn(Fn),
    /// A struct.
    Struct {
        /// Doc comment lines.
        docs: Vec<String>,
        /// Derived traits.
        derives: Vec<String>,
        /// `pub ` or nothing.
        vis: String,
        /// Its name.
        name: String,
        /// Type parameters with bounds.
        generics: Vec<(String, Vec<String>)>,
        /// Fields: docs, visibility, name, type.
        fields: Vec<(Vec<String>, String, String, Type)>,
    },
    /// An enum.
    Enum {
        /// Doc comment lines.
        docs: Vec<String>,
        /// Derived traits.
        derives: Vec<String>,
        /// `pub ` or nothing.
        vis: String,
        /// Its name.
        name: String,
        /// Type parameters with bounds.
        generics: Vec<(String, Vec<String>)>,
        /// Variants: docs, name, payload.
        variants: Vec<(Vec<String>, String, Variant)>,
    },
    /// A trait.
    Trait {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `pub ` or nothing.
        vis: String,
        /// Its name.
        name: String,
        /// Type parameters with bounds.
        generics: Vec<(String, Vec<String>)>,
        /// Method signatures.
        methods: Vec<Fn>,
    },
    /// `impl<G> Trait for Type { ... }`
    Impl {
        /// Type parameters with bounds.
        generics: Vec<(String, Vec<String>)>,
        /// The trait.
        trait_: Option<Type>,
        /// The type.
        ty: Type,
        /// The functions.
        fns: Vec<Fn>,
    },
    /// `const NAME: T = value;`
    Const {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `pub ` or nothing.
        vis: String,
        /// Its name.
        name: String,
        /// Its type.
        ty: Type,
        /// Its value.
        value: Expr,
    },
    /// `#[cfg(test)] mod tests { use super::*; ... }`
    Tests(Vec<Item>),
    /// Source text copied as it is.
    Raw(String),
}

/// A file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// The header comment, without `//`.
    pub header: String,
    /// Inner doc comment lines (`//!`).
    pub docs: Vec<String>,
    /// Inner attributes such as `#![allow(...)]`.
    pub attrs: Vec<String>,
    /// The items.
    pub items: Vec<Item>,
}

/// Prints a file.
pub fn print(file: &File) -> String {
    let mut out = String::new();
    out.push_str(&format!("// {}\n", file.header));
    if !file.docs.is_empty() {
        out.push('\n');
        for d in &file.docs {
            out.push_str(&doc_line("//!", d));
            out.push('\n');
        }
    }
    if !file.attrs.is_empty() {
        out.push('\n');
        for a in &file.attrs {
            out.push_str(a);
            out.push('\n');
        }
    }
    let p = Printer;
    let mut previous: Option<&Item> = None;
    for item in &file.items {
        let together = matches!(
            (previous, item),
            (Some(Item::Use(_)), Item::Use(_)) | (Some(Item::Mod(..)), Item::Mod(..))
        );
        if !together {
            out.push('\n');
        }
        p.item(&mut out, item, 0);
        previous = Some(item);
    }
    out
}

fn doc_line(prefix: &str, text: &str) -> String {
    let text = text.strip_prefix(' ').unwrap_or(text).trim_end();
    if text.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} {text}")
    }
}

/// A Rust string literal.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn spaces(ind: usize) -> String {
    " ".repeat(ind * 4)
}

fn width(s: &str) -> usize {
    s.chars().count()
}

/// Precedence of a binary operator (higher binds tighter).
fn prec(op: &str) -> u8 {
    match op {
        "*" | "/" | "%" => 10,
        "+" | "-" => 9,
        "<<" | ">>" => 8,
        "&" => 7,
        "^" => 6,
        "|" => 5,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 4,
        "&&" => 3,
        "||" => 2,
        _ => 1,
    }
}

/// Precedence of an expression as an operand.
fn expr_prec(e: &Expr) -> u8 {
    match e {
        Expr::Binary(_, op, _) => prec(op),
        Expr::Range(..) => 1,
        Expr::Cast(..) => 11,
        Expr::Unary(..) => 12,
        Expr::Closure(..) | Expr::Return(_) => 0,
        _ => 13,
    }
}

/// Whether a block can stand on one line: only a final expression that can.
fn simple_block(b: &Block) -> Option<&Expr> {
    if b.stmts.is_empty() {
        b.tail.as_deref()
    } else {
        None
    }
}

/// Expressions that are written as blocks and end a statement without `;`.
pub fn block_like(e: &Expr) -> bool {
    matches!(
        e,
        Expr::If(..) | Expr::IfLet(..) | Expr::Match(..) | Expr::Block(_)
    )
}

struct Printer;

impl Printer {
    // ----- types ------------------------------------------------------------------------------

    fn ty(&self, t: &Type) -> String {
        match t {
            Type::Path(p, args) if args.is_empty() => p.clone(),
            Type::Path(p, args) => format!("{p}<{}>", self.types(args)),
            Type::Ref(t) => format!("&{}", self.ty(t)),
            Type::RefMut(t) => format!("&mut {}", self.ty(t)),
            Type::Unit => "()".to_string(),
            Type::DynFn(params, ret) => {
                let r = match ret.as_ref() {
                    Type::Unit => String::new(),
                    r => format!(" -> {}", self.ty(r)),
                };
                format!("dyn Fn({}){r}", self.types(params))
            }
            Type::Dyn(name, args) if args.is_empty() => format!("dyn {name}"),
            Type::Dyn(name, args) => format!("dyn {name}<{}>", self.types(args)),
            Type::Slice(t) => format!("[{}]", self.ty(t)),
            Type::Never => "!".to_string(),
        }
    }

    fn types(&self, ts: &[Type]) -> String {
        ts.iter().map(|t| self.ty(t)).collect::<Vec<_>>().join(", ")
    }

    fn generics(&self, g: &[(String, Vec<String>)]) -> String {
        if g.is_empty() {
            return String::new();
        }
        let list: Vec<String> = g
            .iter()
            .map(|(n, b)| {
                if b.is_empty() {
                    n.clone()
                } else {
                    format!("{n}: {}", b.join(" + "))
                }
            })
            .collect();
        format!("<{}>", list.join(", "))
    }

    // ----- patterns ---------------------------------------------------------------------------

    fn pat(&self, p: &Pat) -> String {
        match p {
            Pat::Wild => "_".to_string(),
            Pat::Bind(n, false) => n.clone(),
            Pat::Bind(n, true) => format!("mut {n}"),
            Pat::Lit(l) => l.clone(),
            Pat::Tuple(path, ps) => {
                let list: Vec<String> = ps.iter().map(|p| self.pat(p)).collect();
                format!("{path}({})", list.join(", "))
            }
            Pat::Struct(path, fields, rest) => {
                let mut list: Vec<String> = fields
                    .iter()
                    .map(|(n, p)| match p {
                        Pat::Bind(b, false) if b == n => n.clone(),
                        p => format!("{n}: {}", self.pat(p)),
                    })
                    .collect();
                if *rest {
                    list.push("..".to_string());
                }
                if list.is_empty() {
                    format!("{path} {{}}")
                } else {
                    format!("{path} {{ {} }}", list.join(", "))
                }
            }
            Pat::Or(ps) => ps
                .iter()
                .map(|p| self.pat(p))
                .collect::<Vec<_>>()
                .join(" | "),
            Pat::Ref(p) => format!("&{}", self.pat(p)),
        }
    }

    // ----- expressions on one line ------------------------------------------------------------

    /// The expression on one line, if it can be written so within the width heuristics.
    fn flat(&self, e: &Expr) -> Option<String> {
        Some(match e {
            Expr::Path(p) | Expr::Lit(p) => p.clone(),
            Expr::Str(s) => quote(s),
            Expr::Call(f, args) => {
                let a = self.flat_list(args, CALL_WIDTH)?;
                format!("{}({a})", self.flat_operand(f, 13)?)
            }
            Expr::Macro(name, args) => {
                let a = self.flat_list(args, CALL_WIDTH)?;
                if name == "vec" {
                    if width(&a) > ARRAY_WIDTH {
                        return None;
                    }
                    format!("vec![{a}]")
                } else {
                    format!("{name}!({a})")
                }
            }
            Expr::Method(..) | Expr::Field(..) | Expr::Try(_) => {
                let s = self.flat_chain(e)?;
                if self.chain_len(e) > 2 && width(&s) > CHAIN_WIDTH {
                    return None;
                }
                s
            }
            Expr::Unary(op, x) => format!("{op}{}", self.flat_operand(x, 12)?),
            Expr::Binary(l, op, r) => {
                let p = prec(op);
                let assign = p == 1;
                let left = if assign {
                    self.flat(l)?
                } else {
                    self.flat_operand(l, p)?
                };
                let right = if assign {
                    self.flat(r)?
                } else {
                    self.flat_operand(r, p + 1)?
                };
                format!("{left} {op} {right}")
            }
            Expr::Struct(path, fields) => {
                if fields.is_empty() {
                    return Some(format!("{path} {{}}"));
                }
                let mut list = Vec::new();
                for (n, v) in fields {
                    let v = self.flat(v)?;
                    list.push(if v == *n { v } else { format!("{n}: {v}") });
                }
                let body = list.join(", ");
                if width(&body) > STRUCT_LIT_WIDTH {
                    return None;
                }
                format!("{path} {{ {body} }}")
            }
            Expr::Tuple(items) => {
                let a = self.flat_list(items, CALL_WIDTH)?;
                if items.len() == 1 {
                    format!("({a},)")
                } else {
                    format!("({a})")
                }
            }
            Expr::Array(items) => {
                let a = self.flat_list(items, ARRAY_WIDTH)?;
                format!("[{a}]")
            }
            Expr::Closure(params, body, mv) => {
                let ps = self.closure_params(params);
                let b = match body.as_ref() {
                    Expr::Block(b) if b.stmts.is_empty() && b.tail.is_none() => "{}".to_string(),
                    Expr::Block(_) => return None,
                    other => self.flat(other)?,
                };
                let m = if *mv { "move " } else { "" };
                format!("{m}|{ps}| {b}")
            }
            Expr::Block(b) if b.stmts.is_empty() && b.tail.is_none() => "{}".to_string(),
            Expr::Block(_) | Expr::Match(..) | Expr::IfLet(..) => return None,
            Expr::If(c, then, otherwise) => {
                let o = otherwise.as_ref()?;
                let t = simple_block(then)?;
                let Expr::Block(ob) = o.as_ref() else {
                    return None;
                };
                let ob = simple_block(ob)?;
                if block_like(t) || block_like(ob) {
                    return None;
                }
                let s = format!(
                    "if {} {{ {} }} else {{ {} }}",
                    self.flat(c)?,
                    self.flat(t)?,
                    self.flat(ob)?
                );
                if width(&s) > IF_ELSE_WIDTH {
                    return None;
                }
                s
            }
            Expr::Return(None) => "return".to_string(),
            Expr::Return(Some(x)) => format!("return {}", self.flat(x)?),
            Expr::Break => "break".to_string(),
            Expr::Continue => "continue".to_string(),
            Expr::Cast(x, t) => format!("{} as {}", self.flat_operand(x, 11)?, self.ty(t)),
            Expr::Paren(x) => format!("({})", self.flat(x)?),
            Expr::Range(a, b, inclusive) => format!(
                "{}{}{}",
                self.flat_operand(a, 2)?,
                if *inclusive { "..=" } else { ".." },
                self.flat_operand(b, 2)?
            ),
        })
    }

    fn closure_params(&self, params: &[(Pat, Option<Type>)]) -> String {
        params
            .iter()
            .map(|(p, t)| match t {
                Some(t) => format!("{}: {}", self.pat(p), self.ty(t)),
                None => self.pat(p),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn flat_list(&self, items: &[Expr], limit: usize) -> Option<String> {
        let mut list = Vec::new();
        for i in items {
            list.push(self.flat(i)?);
        }
        let s = list.join(", ");
        if width(&s) > limit {
            return None;
        }
        Some(s)
    }

    /// An operand on one line, in parentheses if its precedence is lower than `min`.
    fn flat_operand(&self, e: &Expr, min: u8) -> Option<String> {
        let s = self.flat(e)?;
        Some(if expr_prec(e) < min {
            format!("({s})")
        } else {
            s
        })
    }

    /// The number of elements of a chain: the root and each `.field`, `.method()` and `?`.
    fn chain_len(&self, e: &Expr) -> usize {
        match e {
            Expr::Method(r, ..) | Expr::Field(r, _) | Expr::Try(r) => 1 + self.chain_len(r),
            _ => 1,
        }
    }

    fn flat_chain(&self, e: &Expr) -> Option<String> {
        Some(match e {
            Expr::Method(r, name, targs, args) => {
                let a = self.flat_list(args, CALL_WIDTH)?;
                let t = if targs.is_empty() {
                    String::new()
                } else {
                    format!("::<{}>", self.types(targs))
                };
                format!("{}.{name}{t}({a})", self.flat_chain_root(r)?)
            }
            Expr::Field(r, name) => format!("{}.{name}", self.flat_chain_root(r)?),
            Expr::Try(r) => format!("{}?", self.flat_chain_root(r)?),
            other => self.flat(other)?,
        })
    }

    fn flat_chain_root(&self, r: &Expr) -> Option<String> {
        match r {
            Expr::Method(..) | Expr::Field(..) | Expr::Try(_) => self.flat_chain(r),
            other => self.flat_operand(other, 13),
        }
    }

    // ----- expressions over several lines -----------------------------------------------------

    /// The expression starting at column `col` of a line indented by `ind` levels, with
    /// `reserve` characters following it on its last line.
    fn expr(&self, e: &Expr, ind: usize, col: usize, reserve: usize) -> String {
        if let Some(s) = self.flat(e) {
            if col + width(&s) + reserve <= MAX_WIDTH {
                return s;
            }
        }
        self.broken(e, ind, col, reserve)
    }

    fn broken(&self, e: &Expr, ind: usize, col: usize, reserve: usize) -> String {
        match e {
            Expr::Call(f, args) => {
                let head = self.operand(f, 13, ind, col, 0);
                let c = col + width(last_line(&head));
                self.args(&head, "(", ")", args, ind, c, reserve)
            }
            Expr::Macro(name, args) => {
                if name == "vec" {
                    let head = "vec!".to_string();
                    return self.array(&head, "[", "]", args, ind);
                }
                let head = format!("{name}!");
                self.args(&head, "(", ")", args, ind, col + width(&head), reserve)
            }
            Expr::Method(..) | Expr::Field(..) | Expr::Try(_) => self.chain(e, ind, col, reserve),
            Expr::Struct(path, fields) => {
                let mut s = format!("{path} {{\n");
                for (n, v) in fields {
                    let prefix = if matches!(v, Expr::Path(p) if p == n) {
                        String::new()
                    } else {
                        format!("{n}: ")
                    };
                    let start = (ind + 1) * 4 + width(&prefix);
                    let value = if prefix.is_empty() {
                        n.clone()
                    } else {
                        self.expr(v, ind + 1, start, 1)
                    };
                    s.push_str(&format!("{}{prefix}{value},\n", spaces(ind + 1)));
                }
                s.push_str(&format!("{}}}", spaces(ind)));
                s
            }
            Expr::Array(items) => self.array("", "[", "]", items, ind),
            Expr::Tuple(items) => self.args("", "(", ")", items, ind, col, reserve),
            Expr::Unary(op, x) => {
                format!("{op}{}", self.operand(x, 12, ind, col + op.len(), reserve))
            }
            Expr::Binary(l, op, r) => {
                let p = prec(op);
                if p == 1 {
                    let left = self.expr(l, ind, col, 0);
                    let c = width(last_line(&left)) + op.len() + 2;
                    let right = self.expr(r, ind, c, reserve);
                    return format!("{left} {op} {right}");
                }
                let left = self.operand(l, p, ind, col, 0);
                let flat_right = self.flat_operand(r, p + 1);
                let c = width(last_line(&left)) + op.len() + 2;
                if let Some(fr) = &flat_right {
                    if !left.contains('\n') && c + width(fr) + reserve <= MAX_WIDTH {
                        return format!("{left} {op} {fr}");
                    }
                }
                let right = self.operand(r, p + 1, ind + 1, (ind + 1) * 4 + op.len() + 1, reserve);
                format!("{left}\n{}{op} {right}", spaces(ind + 1))
            }
            Expr::Closure(params, body, mv) => {
                let m = if *mv { "move " } else { "" };
                let head = format!("{m}|{}| ", self.closure_params(params));
                let c = col + width(&head);
                let b = match body.as_ref() {
                    Expr::Block(b) => self.block(b, ind),
                    other => self.expr(other, ind, c, reserve),
                };
                format!("{head}{b}")
            }
            Expr::Block(b) => self.block(b, ind),
            Expr::If(c, then, otherwise) => {
                let cond = self.expr(c, ind, col + 3, 2);
                let mut s = format!("if {cond} {}", self.block(then, ind));
                if let Some(o) = otherwise {
                    s.push_str(" else ");
                    match o.as_ref() {
                        Expr::Block(b) => s.push_str(&self.block(b, ind)),
                        other => s.push_str(&self.broken(other, ind, 0, reserve)),
                    }
                }
                s
            }
            Expr::IfLet(p, x, then, otherwise) => {
                let head = format!("if let {} = ", self.pat(p));
                let v = self.expr(x, ind, col + width(&head), 2);
                let mut s = format!("{head}{v} {}", self.block(then, ind));
                if let Some(o) = otherwise {
                    s.push_str(" else ");
                    match o.as_ref() {
                        Expr::Block(b) => s.push_str(&self.block(b, ind)),
                        other => s.push_str(&self.broken(other, ind, 0, reserve)),
                    }
                }
                s
            }
            Expr::Match(x, arms) => {
                let head = self.expr(x, ind, col + 6, 2);
                let mut s = format!("match {head} {{\n");
                for arm in arms {
                    s.push_str(&self.arm(arm, ind + 1));
                }
                s.push_str(&format!("{}}}", spaces(ind)));
                s
            }
            Expr::Return(Some(x)) => format!("return {}", self.expr(x, ind, col + 7, reserve)),
            Expr::Paren(x) => format!("({})", self.expr(x, ind, col + 1, reserve + 1)),
            Expr::Cast(x, t) => {
                let t = self.ty(t);
                format!(
                    "{} as {t}",
                    self.operand(x, 11, ind, col, reserve + 4 + width(&t))
                )
            }
            Expr::Range(a, b, inclusive) => {
                let op = if *inclusive { "..=" } else { ".." };
                let left = self.operand(a, 2, ind, col, 0);
                let c = width(last_line(&left)) + op.len();
                format!("{left}{op}{}", self.operand(b, 2, ind, c, reserve))
            }
            other => self.flat(other).unwrap_or_default(),
        }
    }

    fn operand(&self, e: &Expr, min: u8, ind: usize, col: usize, reserve: usize) -> String {
        if expr_prec(e) < min {
            format!("({})", self.expr(e, ind, col + 1, reserve + 1))
        } else {
            self.expr(e, ind, col, reserve)
        }
    }

    /// `head(a, b)`: on one line, with the last argument overflowing, or one per line.
    #[allow(clippy::too_many_arguments)]
    fn args(
        &self,
        head: &str,
        open: &str,
        close: &str,
        args: &[Expr],
        ind: usize,
        col: usize,
        reserve: usize,
    ) -> String {
        let flat: Option<Vec<String>> = args.iter().map(|a| self.flat(a)).collect();
        if let Some(list) = &flat {
            let s = list.join(", ");
            if width(&s) <= CALL_WIDTH && col + width(&s) + 2 + reserve <= MAX_WIDTH {
                return format!("{head}{open}{s}{close}");
            }
        }
        // The last argument may overflow: a closure, a block or a struct literal.
        if let Some((last, rest)) = args.split_last() {
            let overflow = overflows(last, args.len());
            let rest_flat: Option<Vec<String>> = rest.iter().map(|a| self.flat(a)).collect();
            if overflow {
                if let Some(list) = rest_flat {
                    let mut prefix = list.join(", ");
                    if !prefix.is_empty() {
                        prefix.push_str(", ");
                    }
                    let c = col + 1 + width(&prefix);
                    if c < MAX_WIDTH && width(&prefix) <= CALL_WIDTH {
                        let l = self.broken(last, ind, c, 1 + reserve);
                        let first = l.lines().next().unwrap_or("");
                        if c + width(first) <= MAX_WIDTH && l.contains('\n') {
                            return format!("{head}{open}{prefix}{l}{close}");
                        }
                    }
                }
            }
        }
        let mut s = format!("{head}{open}\n");
        for a in args {
            let v = self.expr(a, ind + 1, (ind + 1) * 4, 1);
            s.push_str(&format!("{}{v},\n", spaces(ind + 1)));
        }
        s.push_str(&format!("{}{close}", spaces(ind)));
        s
    }

    /// `[a, b]` over several lines: short simple items fill the lines, others one per line.
    fn array(&self, head: &str, open: &str, close: &str, items: &[Expr], ind: usize) -> String {
        let simple = items.iter().all(|i| match i {
            Expr::Lit(_) => true,
            Expr::Str(s) => width(s) <= 8,
            Expr::Unary("-", x) => matches!(x.as_ref(), Expr::Lit(_)),
            _ => false,
        });
        let mut s = format!("{head}{open}\n");
        if simple {
            let mut line = String::new();
            let limit = MAX_WIDTH - (ind + 1) * 4;
            for i in items {
                let v = format!("{},", self.flat(i).unwrap_or_default());
                if !line.is_empty() && width(&line) + 1 + width(&v) > limit {
                    s.push_str(&format!("{}{line}\n", spaces(ind + 1)));
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&v);
            }
            if !line.is_empty() {
                s.push_str(&format!("{}{line}\n", spaces(ind + 1)));
            }
        } else {
            for i in items {
                let v = self.expr(i, ind + 1, (ind + 1) * 4, 1);
                s.push_str(&format!("{}{v},\n", spaces(ind + 1)));
            }
        }
        s.push_str(&format!("{}{close}", spaces(ind)));
        s
    }

    /// A chain over several lines: the root, then each element on its own line.
    fn chain(&self, e: &Expr, ind: usize, col: usize, reserve: usize) -> String {
        let mut elems: Vec<&Expr> = Vec::new();
        let mut cur = e;
        while let Expr::Method(r, ..) | Expr::Field(r, _) | Expr::Try(r) = cur {
            elems.push(cur);
            cur = r;
        }
        elems.reverse();
        let mut root = self.operand(cur, 13, ind, col, 0);
        // A root no wider than the indentation takes the next elements on its line.
        let tab = 4usize.saturating_sub(col.saturating_sub(ind * 4));
        while width(&root) <= tab && !root.contains('\n') && elems.len() > 1 {
            let el = elems.remove(0);
            match el {
                Expr::Field(_, name) => root.push_str(&format!(".{name}")),
                Expr::Method(_, name, targs, args) => {
                    let a = self.flat_list(args, CALL_WIDTH).unwrap_or_default();
                    root.push_str(&format!(".{name}{}({a})", turbofish(self, targs)));
                }
                Expr::Try(_) => root.push('?'),
                _ => {}
            }
        }
        // A chain of one call keeps the call on the line of its root, unless the root is written
        // over several lines; then the elements start at the indentation of the root.
        let multi = root.contains('\n');
        if elems.len() == 1 && !multi {
            if let Some(Expr::Method(_, name, targs, args)) = elems.first() {
                let head = format!("{root}.{name}{}", turbofish(self, targs));
                let c = col + width(last_line(&head));
                return self.args(&head, "(", ")", args, ind, c, reserve);
            }
        }
        let mut s = root;
        let inner = if multi { ind } else { ind + 1 };
        for el in elems {
            let start = inner * 4;
            match el {
                Expr::Method(_, name, targs, args) => {
                    let head = format!(".{name}{}", turbofish(self, targs));
                    let c = start + width(&head);
                    let call = self.args(&head, "(", ")", args, inner, c, reserve);
                    s.push_str(&format!("\n{}{call}", spaces(inner)));
                }
                Expr::Field(_, name) => s.push_str(&format!("\n{}.{name}", spaces(inner))),
                Expr::Try(_) => s.push('?'),
                _ => {}
            }
        }
        s
    }

    fn arm(&self, arm: &Arm, ind: usize) -> String {
        let mut head = self.pat(&arm.pat);
        if let Some(g) = &arm.guard {
            head.push_str(&format!(
                " if {}",
                self.expr(g, ind, ind * 4 + width(&head) + 4, 3)
            ));
        }
        let start = ind * 4 + width(&head) + 4;
        // A block with only a value is written as that value.
        let body = match &arm.body {
            Expr::Block(b)
                if b.stmts.is_empty() && b.tail.as_deref().is_some_and(|t| !block_like(t)) =>
            {
                b.tail.as_deref().unwrap_or(&arm.body)
            }
            other => other,
        };
        match body {
            Expr::Block(b) => format!("{}{head} => {}\n", spaces(ind), self.block(b, ind)),
            e if block_like(e) => {
                // A multi-line `if` or `match` is written inside a block.
                let inner = self.broken(e, ind + 1, (ind + 1) * 4, 0);
                format!(
                    "{}{head} => {{\n{}{inner}\n{}}}\n",
                    spaces(ind),
                    spaces(ind + 1),
                    spaces(ind)
                )
            }
            e => {
                let v = self.expr(e, ind, start, 1);
                if !v.contains('\n') || !self.fits_next_line(e, ind) {
                    format!("{}{head} => {v},\n", spaces(ind))
                } else {
                    let v = self.expr(e, ind + 1, (ind + 1) * 4, 1);
                    format!(
                        "{}{head} => {{\n{}{v}\n{}}}\n",
                        spaces(ind),
                        spaces(ind + 1),
                        spaces(ind)
                    )
                }
            }
        }
    }

    fn fits_next_line(&self, e: &Expr, ind: usize) -> bool {
        self.flat(e)
            .is_some_and(|s| (ind + 1) * 4 + width(&s) < MAX_WIDTH)
    }

    // ----- blocks and statements --------------------------------------------------------------

    fn block(&self, b: &Block, ind: usize) -> String {
        if b.stmts.is_empty() && b.tail.is_none() {
            return "{}".to_string();
        }
        let mut s = "{\n".to_string();
        for st in &b.stmts {
            self.stmt(&mut s, st, ind + 1);
        }
        if let Some(t) = &b.tail {
            let v = self.expr(t, ind + 1, (ind + 1) * 4, 0);
            s.push_str(&format!("{}{v}\n", spaces(ind + 1)));
        }
        s.push_str(&format!("{}}}", spaces(ind)));
        s
    }

    fn stmt(&self, out: &mut String, s: &Stmt, ind: usize) {
        let pad = spaces(ind);
        match s {
            Stmt::Let(p, t, v) => {
                let mut head = format!("let {}", self.pat(p));
                if let Some(t) = t {
                    head.push_str(&format!(": {}", self.ty(t)));
                }
                match v {
                    None => out.push_str(&format!("{pad}{head};\n")),
                    Some(v) => {
                        let rhs = self.rhs(v, ind, ind * 4 + width(&head) + 3, 1);
                        out.push_str(&format!("{pad}{head} ={rhs};\n"));
                    }
                }
            }
            Stmt::Expr(e) => {
                if block_like(e) {
                    let v = self.broken(e, ind, ind * 4, 0);
                    let v = if let Expr::If(..) = e { v } else { v };
                    out.push_str(&format!("{pad}{v}\n"));
                } else if let Expr::Binary(l, op, r) = e {
                    if prec(op) == 1 {
                        let left = self.expr(l, ind, ind * 4, 0);
                        let c = ind * 4 + width(last_line(&left)) + op.len() + 1;
                        let rhs = self.rhs(r, ind, c, 1);
                        out.push_str(&format!("{pad}{left} {op}{rhs};\n"));
                        return;
                    }
                    let v = self.expr(e, ind, ind * 4, 1);
                    out.push_str(&format!("{pad}{v};\n"));
                } else {
                    let v = self.expr(e, ind, ind * 4, 1);
                    out.push_str(&format!("{pad}{v};\n"));
                }
            }
            Stmt::While(c, b) => {
                let cond = self.expr(c, ind, ind * 4 + 6, 2);
                out.push_str(&format!("{pad}while {cond} {}\n", self.block(b, ind)));
            }
            Stmt::Loop(b) => out.push_str(&format!("{pad}loop {}\n", self.block(b, ind))),
            Stmt::WhileLet(p, e, b) => {
                let head = format!("while let {} = ", self.pat(p));
                let v = self.expr(e, ind, ind * 4 + width(&head), 2);
                out.push_str(&format!("{pad}{head}{v} {}\n", self.block(b, ind)));
            }
            Stmt::For(p, e, b) => {
                let head = format!("for {} in ", self.pat(p));
                let it = self.expr(e, ind, ind * 4 + width(&head), 2);
                out.push_str(&format!("{pad}{head}{it} {}\n", self.block(b, ind)));
            }
        }
    }

    /// The right side of `=` (with the leading space): on the same line, or on the next line
    /// when that is better, by the rules of rustfmt (`choose_rhs`).
    fn rhs(&self, v: &Expr, ind: usize, col: usize, reserve: usize) -> String {
        let same = self.expr(v, ind, col, reserve);
        let fits = |s: &str, first: usize| {
            s.lines().enumerate().all(|(i, l)| {
                let c = if i == 0 { first } else { 0 };
                c + width(l) <= MAX_WIDTH
            }) && s.lines().last().is_some_and(|l| {
                let c = if s.contains('\n') { 0 } else { first };
                c + width(l) + reserve <= MAX_WIDTH
            })
        };
        if !same.contains('\n') && fits(&same, col) {
            return format!(" {same}");
        }
        let c = (ind + 1) * 4;
        let next = self.expr(v, ind + 1, c, reserve);
        if !fits(&next, c) {
            return format!(" {same}");
        }
        let ends = |s: &str, ch: char| s.lines().next().is_some_and(|l| l.ends_with(ch));
        let newlines = |s: &str| s.matches('\n').count();
        let prefer_next = !next.contains('\n')
            || newlines(&same) > newlines(&next) + 1
            || ['(', '{', '[']
                .iter()
                .any(|&ch| ends(&same, ch) && !ends(&next, ch));
        if prefer_next {
            format!("\n{}{next}", spaces(ind + 1))
        } else {
            format!(" {same}")
        }
    }

    // ----- items ------------------------------------------------------------------------------

    fn docs(&self, out: &mut String, docs: &[String], ind: usize) {
        for d in docs {
            out.push_str(&format!("{}{}\n", spaces(ind), doc_line("///", d)));
        }
    }

    fn func(&self, out: &mut String, f: &Fn, ind: usize) {
        self.docs(out, &f.docs, ind);
        for a in &f.attrs {
            out.push_str(&format!("{}{a}\n", spaces(ind)));
        }
        let mut params: Vec<String> = Vec::new();
        if let Some(s) = &f.self_param {
            params.push(s.clone());
        }
        for (p, t) in &f.params {
            params.push(format!("{}: {}", self.pat(p), self.ty(t)));
        }
        let ret = match &f.ret {
            Some(t) => format!(" -> {}", self.ty(t)),
            None => String::new(),
        };
        let head = format!(
            "{}{}fn {}{}",
            spaces(ind),
            f.vis,
            f.name,
            self.generics(&f.generics)
        );
        let end = match &f.body {
            Some(_) => " {",
            None => ";",
        };
        let one = format!("{head}({}){ret}{end}", params.join(", "));
        let sig = if width(&one) <= MAX_WIDTH {
            one
        } else {
            let mut s = format!("{head}(\n");
            for p in &params {
                s.push_str(&format!("{}{p},\n", spaces(ind + 1)));
            }
            s.push_str(&format!("{}){ret}{end}", spaces(ind)));
            s
        };
        match &f.body {
            None => out.push_str(&format!("{sig}\n")),
            Some(b) if b.stmts.is_empty() && b.tail.is_none() => {
                out.push_str(&format!("{}}}\n", sig));
            }
            Some(b) => {
                let body = self.block(b, ind);
                // `sig` ends with ` {`; the block printer starts with `{`.
                let sig = sig.strip_suffix('{').unwrap_or(&sig);
                out.push_str(&format!("{sig}{body}\n"));
            }
        }
    }

    fn item(&self, out: &mut String, item: &Item, ind: usize) {
        let pad = spaces(ind);
        match item {
            Item::Use(p) => out.push_str(&format!("{pad}use {p};\n")),
            Item::Mod(vis, name) => out.push_str(&format!("{pad}{vis}mod {name};\n")),
            Item::Fn(f) => self.func(out, f, ind),
            Item::Struct {
                docs,
                derives,
                vis,
                name,
                generics,
                fields,
            } => {
                self.docs(out, docs, ind);
                if !derives.is_empty() {
                    out.push_str(&format!("{pad}#[derive({})]\n", derives.join(", ")));
                }
                let head = format!("{pad}{vis}struct {name}{}", self.generics(generics));
                if fields.is_empty() {
                    out.push_str(&format!("{head} {{}}\n"));
                    return;
                }
                out.push_str(&format!("{head} {{\n"));
                for (d, v, n, t) in fields {
                    self.docs(out, d, ind + 1);
                    out.push_str(&format!("{}{v}{n}: {},\n", spaces(ind + 1), self.ty(t)));
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Item::Enum {
                docs,
                derives,
                vis,
                name,
                generics,
                variants,
            } => {
                self.docs(out, docs, ind);
                if !derives.is_empty() {
                    out.push_str(&format!("{pad}#[derive({})]\n", derives.join(", ")));
                }
                out.push_str(&format!(
                    "{pad}{vis}enum {name}{} {{\n",
                    self.generics(generics)
                ));
                for (d, n, v) in variants {
                    self.docs(out, d, ind + 1);
                    let p = spaces(ind + 1);
                    match v {
                        Variant::Unit => out.push_str(&format!("{p}{n},\n")),
                        Variant::Tuple(ts) => {
                            out.push_str(&format!("{p}{n}({}),\n", self.types(ts)))
                        }
                        Variant::Struct(fs) => {
                            let list: Vec<String> = fs
                                .iter()
                                .map(|(f, t)| format!("{f}: {}", self.ty(t)))
                                .collect();
                            let body = list.join(", ");
                            if width(&body) <= STRUCT_VARIANT_WIDTH {
                                out.push_str(&format!("{p}{n} {{ {body} }},\n"));
                            } else {
                                out.push_str(&format!("{p}{n} {{\n"));
                                for l in list {
                                    out.push_str(&format!("{}{l},\n", spaces(ind + 2)));
                                }
                                out.push_str(&format!("{p}}},\n"));
                            }
                        }
                    }
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Item::Trait {
                docs,
                vis,
                name,
                generics,
                methods,
            } => {
                self.docs(out, docs, ind);
                out.push_str(&format!(
                    "{pad}{vis}trait {name}{} {{\n",
                    self.generics(generics)
                ));
                for m in methods {
                    self.func(out, m, ind + 1);
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Item::Impl {
                generics,
                trait_,
                ty,
                fns,
            } => {
                let t = match trait_ {
                    Some(tr) => format!("{} for ", self.ty(tr)),
                    None => String::new(),
                };
                out.push_str(&format!(
                    "{pad}impl{} {t}{} {{\n",
                    self.generics(generics),
                    self.ty(ty)
                ));
                for (i, f) in fns.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    self.func(out, f, ind + 1);
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Item::Const {
                docs,
                vis,
                name,
                ty,
                value,
            } => {
                self.docs(out, docs, ind);
                let head = format!("{pad}{vis}const {name}: {} =", self.ty(ty));
                let rhs = self.rhs(value, ind, width(&head) + 1, 1);
                out.push_str(&format!("{head}{rhs};\n"));
            }
            Item::Tests(items) => {
                out.push_str(&format!("{pad}#[cfg(test)]\n{pad}mod tests {{\n"));
                out.push_str(&format!("{}use super::*;\n", spaces(ind + 1)));
                for i in items {
                    out.push('\n');
                    self.item(out, i, ind + 1);
                }
                out.push_str(&format!("{pad}}}\n"));
            }
            Item::Raw(text) => {
                out.push_str(text.trim_end());
                out.push('\n');
            }
        }
    }
}

fn last_line(s: &str) -> &str {
    s.rsplit('\n').next().unwrap_or(s)
}

fn turbofish(p: &Printer, targs: &[Type]) -> String {
    if targs.is_empty() {
        String::new()
    } else {
        format!("::<{}>", p.types(targs))
    }
}

/// Whether the last of `count` arguments may overflow: start on the line of the call and end on
/// the line of its closing parenthesis (rustfmt's `can_be_overflowed_expr`).
fn overflows(e: &Expr, count: usize) -> bool {
    match e {
        Expr::Closure(..) | Expr::Block(_) => true,
        Expr::Match(..)
        | Expr::If(..)
        | Expr::IfLet(..)
        | Expr::Struct(..)
        | Expr::Array(_)
        | Expr::Macro(..)
        | Expr::Call(..)
        | Expr::Method(..)
        | Expr::Tuple(_) => count == 1,
        // `&(|a| ...)` is in parentheses, which do not overflow.
        Expr::Unary(_, x) if matches!(x.as_ref(), Expr::Closure(..)) => false,
        Expr::Unary(_, x) | Expr::Try(x) | Expr::Cast(x, _) => overflows(x, count),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> Expr {
        Expr::Path(s.to_string())
    }

    #[test]
    fn struct_literals_go_vertical_when_long() {
        let p = Printer;
        let short = Expr::Struct(
            "Rect".to_string(),
            vec![
                ("w".to_string(), Expr::Lit("3.0".to_string())),
                ("h".to_string(), Expr::Lit("4.0".to_string())),
            ],
        );
        assert_eq!(p.expr(&short, 1, 12, 1), "Rect { w: 3.0, h: 4.0 }");
        let long = Expr::Struct(
            "Store".to_string(),
            vec![
                (
                    "tasks".to_string(),
                    Expr::Macro("vec".to_string(), Vec::new()),
                ),
                ("next_id".to_string(), Expr::Lit("1".to_string())),
            ],
        );
        assert_eq!(
            p.expr(&long, 1, 12, 1),
            "Store {\n        tasks: vec![],\n        next_id: 1,\n    }"
        );
    }

    #[test]
    fn long_chains_put_each_element_on_a_line() {
        let p = Printer;
        let e = Expr::Method(
            Box::new(Expr::Method(
                Box::new(Expr::Method(
                    Box::new(Expr::Field(Box::new(path("self")), "tasks".to_string())),
                    "iter".to_string(),
                    Vec::new(),
                    Vec::new(),
                )),
                "map".to_string(),
                Vec::new(),
                vec![Expr::Closure(
                    vec![(Pat::Bind("task".to_string(), false), None)],
                    Box::new(Expr::Method(
                        Box::new(path("task")),
                        "describe".to_string(),
                        Vec::new(),
                        Vec::new(),
                    )),
                    false,
                )],
            )),
            "collect".to_string(),
            vec![Type::Path("Vec".to_string(), vec![Type::name("_")])],
            Vec::new(),
        );
        assert_eq!(
            p.expr(&e, 1, 12, 1),
            "self\n        .tasks\n        .iter()\n        .map(|task| task.describe())\n        .collect::<Vec<_>>()"
        );
    }

    #[test]
    fn strings_are_quoted() {
        assert_eq!(quote("a\"b\\c\n😀"), "\"a\\\"b\\\\c\\n😀\"");
    }
}
