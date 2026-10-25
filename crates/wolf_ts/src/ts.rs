//! A small tree of TypeScript and its printer.
//!
//! The backend builds this tree from the IR and then prints it (spec chapter 8, section 1). The
//! printer follows the style of `prettier`: two spaces, double quotes, semicolons, and long
//! argument lists split one per line.

/// A TypeScript type.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// A named type with arguments: `Map<string, bigint>`, `Stack<T>`.
    Ref(String, Vec<Type>),
    /// `T[]`
    Array(Box<Type>),
    /// `A | B`
    Union(Vec<Type>),
    /// `(a: A) => R`
    Fn(Vec<(String, Type)>, Box<Type>),
    /// A string literal type: `"Red"`.
    Lit(String),
    /// `{ a: A; b: B }`
    Object(Vec<(String, Type)>),
}

impl Type {
    /// A type without arguments.
    pub fn name(name: &str) -> Type {
        Type::Ref(name.to_string(), Vec::new())
    }
}

/// A TypeScript expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// A name.
    Ident(String),
    /// A `bigint` literal, without the `n`.
    BigInt(i64),
    /// A number literal as written.
    Num(String),
    /// A string literal.
    Str(String),
    /// `true` or `false`.
    Bool(bool),
    /// `undefined`
    Undefined,
    /// `this`
    This,
    /// `[a, b]`
    Array(Vec<Expr>),
    /// `{ a: x, b }`; an entry with an empty name is a spread `...x`.
    Object(Vec<(String, Expr)>),
    /// `{ get v() { return x; }, set v(value) { x = value; } }`: a reference to a place.
    RefTo(Box<Expr>),
    /// `new C<T>(args)`
    New(String, Vec<Expr>),
    /// `f(args)` or `f<T>(args)`
    Call(Box<Expr>, Vec<Type>, Vec<Expr>),
    /// `a.b`
    Member(Box<Expr>, String),
    /// `a[b]`
    Index(Box<Expr>, Box<Expr>),
    /// `(a, b) => body`
    Arrow(Vec<(String, Option<Type>)>, Option<Type>, ArrowBody),
    /// `a op b`
    Binary(Box<Expr>, &'static str, Box<Expr>),
    /// `op a`
    Unary(&'static str, Box<Expr>),
    /// `c ? a : b`
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `a as T`
    As(Box<Expr>, Type),
    /// `[...a]`
    Spread(Box<Expr>),
    /// A template literal: string parts are text, the others are `${...}`.
    Template(Vec<Expr>),
}

/// The body of an arrow function.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrowBody {
    /// `=> expr`
    Expr(Box<Expr>),
    /// `=> { ... }`
    Block(Vec<Stmt>),
}

/// A TypeScript statement.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `const x: T = e;`
    Const(String, Option<Type>, Expr),
    /// `let x: T = e;` or `let x: T;`
    Let(String, Option<Type>, Option<Expr>),
    /// `e;`
    Expr(Expr),
    /// `a = b;`
    Assign(Expr, Expr),
    /// `if (c) { ... } else { ... }`; an `else` holding a single `if` prints as `else if`.
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    /// `while (c) { ... }`
    While(Expr, Vec<Stmt>),
    /// `for (let i = a; cond; step) { ... }`
    For(Box<Stmt>, Expr, Expr, Vec<Stmt>),
    /// `for (const x of xs) { ... }`
    ForOf(String, Expr, Vec<Stmt>),
    /// `return e;`
    Return(Option<Expr>),
    /// `break;` or `break label;`
    Break(Option<String>),
    /// `continue;` or `continue label;`
    Continue(Option<String>),
    /// `throw e;`
    Throw(Expr),
    /// `label: { ... }` or `label: while ...`
    Labeled(String, Box<Stmt>),
    /// `{ ... }`
    Block(Vec<Stmt>),
    /// `try { ... } catch (e) { ... }`
    Try(Vec<Stmt>, String, Vec<Stmt>),
}

/// A class member function.
#[derive(Clone, Debug, PartialEq)]
pub struct Method {
    /// Doc comment lines.
    pub docs: Vec<String>,
    /// `static`
    pub is_static: bool,
    /// Its name.
    pub name: String,
    /// Type parameters.
    pub generics: Vec<String>,
    /// Parameters.
    pub params: Vec<(String, Type)>,
    /// The result type.
    pub ret: Type,
    /// The body.
    pub body: Vec<Stmt>,
}

/// A constructor: parameters and body.
pub type Constructor = (Vec<(String, Type)>, Vec<Stmt>);

/// A method signature of an interface: docs, name, parameters and result.
pub type Signature = (Vec<String>, String, Vec<(String, Type)>, Type);

/// A top-level declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// `import { a, b as c } from "path";`
    Import(Vec<(String, Option<String>)>, String),
    /// `import * as name from "path";`
    ImportAll(String, String),
    /// `export { a, b as c } from "path";`
    ExportFrom(Vec<(String, Option<String>)>, String),
    /// A class.
    Class {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `export`
        export: bool,
        /// Its name.
        name: String,
        /// Type parameters with constraints.
        generics: Vec<(String, Option<Type>)>,
        /// Implemented interfaces.
        implements: Vec<Type>,
        /// Fields: name, type and an initializer for constant fields such as `kind`.
        fields: Vec<(String, Type, Option<Expr>)>,
        /// The constructor parameters and body.
        constructor: Option<Constructor>,
        /// Methods.
        methods: Vec<Method>,
    },
    /// An interface.
    Interface {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `export`
        export: bool,
        /// Its name.
        name: String,
        /// Type parameters.
        generics: Vec<(String, Option<Type>)>,
        /// The interfaces after `extends`.
        extends: Vec<Type>,
        /// Method signatures: docs, name, params, result.
        methods: Vec<Signature>,
    },
    /// `type Name<T> = ...;`
    TypeAlias {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `export`
        export: bool,
        /// Its name.
        name: String,
        /// Type parameters.
        generics: Vec<String>,
        /// The type.
        ty: Type,
    },
    /// A function.
    Function {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `export`
        export: bool,
        /// Its name.
        name: String,
        /// Type parameters with constraints.
        generics: Vec<(String, Option<Type>)>,
        /// Parameters.
        params: Vec<(String, Type)>,
        /// The result type.
        ret: Type,
        /// The body.
        body: Vec<Stmt>,
    },
    /// `export const name: T = value;`
    Const {
        /// Doc comment lines.
        docs: Vec<String>,
        /// `export`
        export: bool,
        /// Its name.
        name: String,
        /// Its type.
        ty: Type,
        /// Its value.
        value: Expr,
    },
    /// A statement at the top level.
    Stmt(Stmt),
    /// Source text copied as it is (runtime support code).
    Raw(String),
}

/// A file.
#[derive(Clone, Debug, PartialEq)]
pub struct File {
    /// The header comment, without `//`.
    pub header: String,
    /// The declarations.
    pub items: Vec<Item>,
}

/// Prints a file.
pub fn print(file: &File) -> String {
    let mut p = Printer::default();
    p.line(&format!("// {}", file.header));
    let mut previous_import = false;
    for (i, item) in file.items.iter().enumerate() {
        let import = matches!(item, Item::Import(..) | Item::ImportAll(..));
        if i == 0 || !(import && previous_import) {
            p.blank();
        }
        p.item(item);
        previous_import = import;
    }
    p.out
}

#[derive(Default)]
struct Printer {
    out: String,
    indent: usize,
}

impl Printer {
    /// A line at the current indentation; lines inside `text` (from multi-line arrows) keep
    /// their own indentation relative to it.
    fn line(&mut self, text: &str) {
        for l in text.split('\n') {
            if !l.is_empty() {
                for _ in 0..self.indent {
                    self.out.push_str("  ");
                }
                self.out.push_str(l);
            }
            self.out.push('\n');
        }
    }

    fn blank(&mut self) {
        if !self.out.ends_with("\n\n") && !self.out.is_empty() {
            self.out.push('\n');
        }
    }

    fn docs(&mut self, docs: &[String]) {
        match docs {
            [] => {}
            [one] => self.line(&format!("/** {} */", one.trim())),
            many => {
                self.line("/**");
                for d in many {
                    let d = d.trim_end();
                    if d.trim().is_empty() {
                        self.line(" *");
                    } else {
                        self.line(&format!(" * {}", d.strip_prefix(' ').unwrap_or(d)));
                    }
                }
                self.line(" */");
            }
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Import(names, from) => {
                let names: Vec<String> = names.iter().map(import_name).collect();
                self.line(&format!(
                    "import {{ {} }} from {};",
                    names.join(", "),
                    quote(from)
                ));
            }
            Item::ImportAll(name, from) => {
                self.line(&format!("import * as {name} from {};", quote(from)));
            }
            Item::ExportFrom(names, from) => {
                let names: Vec<String> = names.iter().map(import_name).collect();
                self.line(&format!(
                    "export {{ {} }} from {};",
                    names.join(", "),
                    quote(from)
                ));
            }
            Item::Class {
                docs,
                export,
                name,
                generics,
                implements,
                fields,
                constructor,
                methods,
            } => {
                self.docs(docs);
                let mut head = format!(
                    "{}class {name}{}",
                    export_kw(*export),
                    generic_decls(generics)
                );
                if !implements.is_empty() {
                    let list: Vec<String> = implements.iter().map(ty).collect();
                    head.push_str(&format!(" implements {}", list.join(", ")));
                }
                self.line(&format!("{head} {{"));
                self.indent += 1;
                for (n, t, init) in fields {
                    match init {
                        Some(init) => {
                            self.line(&format!("readonly {n}: {} = {};", ty(t), expr(init)))
                        }
                        None => self.line(&format!("{n}: {};", ty(t))),
                    }
                }
                if let Some((params, body)) = constructor {
                    if !fields.is_empty() {
                        self.blank();
                    }
                    self.line(&format!("constructor({}) {{", params_text(params)));
                    self.block_body(body);
                    self.line("}");
                }
                for m in methods {
                    if !self.out.ends_with("{\n") {
                        self.blank();
                    }
                    self.docs(&m.docs);
                    let generics = if m.generics.is_empty() {
                        String::new()
                    } else {
                        format!("<{}>", m.generics.join(", "))
                    };
                    self.line(&format!(
                        "{}{}{generics}({}): {} {{",
                        if m.is_static { "static " } else { "" },
                        m.name,
                        params_text(&m.params),
                        ty(&m.ret)
                    ));
                    self.block_body(&m.body);
                    self.line("}");
                }
                self.indent -= 1;
                self.line("}");
            }
            Item::Interface {
                docs,
                export,
                name,
                generics,
                extends,
                methods,
            } => {
                self.docs(docs);
                let extends = if extends.is_empty() {
                    String::new()
                } else {
                    let list: Vec<String> = extends.iter().map(ty).collect();
                    format!(" extends {}", list.join(", "))
                };
                self.line(&format!(
                    "{}interface {name}{}{extends} {{",
                    export_kw(*export),
                    generic_decls(generics)
                ));
                self.indent += 1;
                for (d, n, params, ret) in methods {
                    self.docs(d);
                    self.line(&format!("{n}({}): {};", params_text(params), ty(ret)));
                }
                self.indent -= 1;
                self.line("}");
            }
            Item::TypeAlias {
                docs,
                export,
                name,
                generics,
                ty: t,
            } => {
                self.docs(docs);
                let g = if generics.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", generics.join(", "))
                };
                self.line(&format!(
                    "{}type {name}{g} = {};",
                    export_kw(*export),
                    ty(t)
                ));
            }
            Item::Function {
                docs,
                export,
                name,
                generics,
                params,
                ret,
                body,
            } => {
                self.docs(docs);
                self.line(&format!(
                    "{}function {name}{}({}): {} {{",
                    export_kw(*export),
                    generic_decls(generics),
                    params_text(params),
                    ty(ret)
                ));
                self.block_body(body);
                self.line("}");
            }
            Item::Const {
                docs,
                export,
                name,
                ty: t,
                value,
            } => {
                self.docs(docs);
                self.line(&format!(
                    "{}const {name}: {} = {};",
                    export_kw(*export),
                    ty(t),
                    expr(value)
                ));
            }
            Item::Stmt(s) => self.stmt(s),
            Item::Raw(text) => {
                for l in text.trim_end().lines() {
                    self.line(l);
                }
            }
        }
    }

    fn block_body(&mut self, body: &[Stmt]) {
        self.indent += 1;
        for s in body {
            self.stmt(s);
        }
        self.indent -= 1;
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Const(n, t, e) => match t {
                Some(t) => self.line(&format!("const {n}: {} = {};", ty(t), expr(e))),
                None => self.line(&format!("const {n} = {};", expr(e))),
            },
            Stmt::Let(n, t, e) => {
                let t = t
                    .as_ref()
                    .map(|t| format!(": {}", ty(t)))
                    .unwrap_or_default();
                match e {
                    Some(e) => self.line(&format!("let {n}{t} = {};", expr(e))),
                    None => self.line(&format!("let {n}{t};")),
                }
            }
            Stmt::Expr(e) => self.line(&format!("{};", expr_stmt(e))),
            Stmt::Assign(a, b) => self.line(&format!("{} = {};", expr(a), expr(b))),
            Stmt::If(c, then, otherwise) => {
                self.line(&format!("if ({}) {{", expr(c)));
                self.block_body(then);
                let mut rest = otherwise.as_ref();
                while let Some(o) = rest {
                    if let [Stmt::If(c2, t2, o2)] = o.as_slice() {
                        self.line(&format!("}} else if ({}) {{", expr(c2)));
                        self.block_body(t2);
                        rest = o2.as_ref();
                    } else {
                        self.line("} else {");
                        self.block_body(o);
                        rest = None;
                    }
                }
                self.line("}");
            }
            Stmt::While(c, body) => {
                self.line(&format!("while ({}) {{", expr(c)));
                self.block_body(body);
                self.line("}");
            }
            Stmt::For(init, cond, step, body) => {
                let init = match init.as_ref() {
                    Stmt::Let(n, t, Some(e)) => {
                        let t = t
                            .as_ref()
                            .map(|t| format!(": {}", ty(t)))
                            .unwrap_or_default();
                        format!("let {n}{t} = {}", expr(e))
                    }
                    _ => String::new(),
                };
                self.line(&format!("for ({init}; {}; {}) {{", expr(cond), expr(step)));
                self.block_body(body);
                self.line("}");
            }
            Stmt::ForOf(n, e, body) => {
                self.line(&format!("for (const {n} of {}) {{", expr(e)));
                self.block_body(body);
                self.line("}");
            }
            Stmt::Return(e) => match e {
                Some(e) => self.line(&format!("return {};", expr(e))),
                None => self.line("return;"),
            },
            Stmt::Break(l) => match l {
                Some(l) => self.line(&format!("break {l};")),
                None => self.line("break;"),
            },
            Stmt::Continue(l) => match l {
                Some(l) => self.line(&format!("continue {l};")),
                None => self.line("continue;"),
            },
            Stmt::Throw(e) => self.line(&format!("throw {};", expr(e))),
            Stmt::Labeled(l, inner) => match inner.as_ref() {
                Stmt::Block(body) => {
                    self.line(&format!("{l}: {{"));
                    self.block_body(body);
                    self.line("}");
                }
                other => {
                    let mut sub = Printer {
                        out: String::new(),
                        indent: self.indent,
                    };
                    sub.stmt(other);
                    let text = sub.out;
                    let trimmed = text.trim_start();
                    let pad = "  ".repeat(self.indent);
                    self.out.push_str(&format!("{pad}{l}: {trimmed}"));
                }
            },
            Stmt::Block(body) => {
                self.line("{");
                self.block_body(body);
                self.line("}");
            }
            Stmt::Try(body, name, handler) => {
                self.line("try {");
                self.block_body(body);
                self.line(&format!("}} catch ({name}) {{"));
                self.block_body(handler);
                self.line("}");
            }
        }
    }
}

fn import_name((name, alias): &(String, Option<String>)) -> String {
    match alias {
        Some(a) => format!("{name} as {a}"),
        None => name.clone(),
    }
}

fn export_kw(export: bool) -> &'static str {
    if export { "export " } else { "" }
}

fn generic_decls(generics: &[(String, Option<Type>)]) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let list: Vec<String> = generics
        .iter()
        .map(|(n, c)| match c {
            Some(c) => format!("{n} extends {}", ty(c)),
            None => n.clone(),
        })
        .collect();
    format!("<{}>", list.join(", "))
}

fn params_text(params: &[(String, Type)]) -> String {
    let list: Vec<String> = params
        .iter()
        .map(|(n, t)| format!("{n}: {}", ty(t)))
        .collect();
    list.join(", ")
}

/// A type as text.
pub fn ty(t: &Type) -> String {
    match t {
        Type::Ref(name, args) => {
            if args.is_empty() {
                name.clone()
            } else {
                let list: Vec<String> = args.iter().map(ty).collect();
                format!("{name}<{}>", list.join(", "))
            }
        }
        Type::Array(inner) => match inner.as_ref() {
            Type::Union(_) | Type::Fn(..) => format!("({})[]", ty(inner)),
            _ => format!("{}[]", ty(inner)),
        },
        Type::Union(list) => {
            let list: Vec<String> = list
                .iter()
                .map(|t| match t {
                    Type::Fn(..) => format!("({})", ty(t)),
                    _ => ty(t),
                })
                .collect();
            list.join(" | ")
        }
        Type::Fn(params, ret) => format!("({}) => {}", params_text(params), ty(ret)),
        Type::Lit(s) => quote(s),
        Type::Object(fields) => {
            let list: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{n}: {}", ty(t)))
                .collect();
            format!("{{ {} }}", list.join("; "))
        }
    }
}

/// A string literal with double quotes.
pub fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Binding strength, higher binds tighter.
fn prec(e: &Expr) -> u8 {
    match e {
        Expr::Arrow(..) => 1,
        Expr::Cond(..) => 2,
        Expr::Binary(_, op, _) => match *op {
            "||" | "??" => 3,
            "&&" => 4,
            "|" => 5,
            "^" => 6,
            "&" => 7,
            "===" | "!==" => 8,
            "<" | ">" | "<=" | ">=" | "instanceof" | "in" => 9,
            "<<" | ">>" => 10,
            "+" | "-" => 11,
            _ => 12,
        },
        Expr::As(..) => 9,
        Expr::Unary(..) => 14,
        Expr::Spread(..) => 1,
        _ => 17,
    }
}

fn wrap(e: &Expr, min: u8) -> String {
    if prec(e) < min {
        format!("({})", expr(e))
    } else {
        expr(e)
    }
}

/// An expression used as a statement: object literals and arrows need parentheses.
fn expr_stmt(e: &Expr) -> String {
    match e {
        Expr::Object(_) | Expr::Arrow(..) => format!("({})", expr(e)),
        _ => expr(e),
    }
}

fn list(items: &[String]) -> String {
    items.join(", ")
}

/// An expression as text.
pub fn expr(e: &Expr) -> String {
    match e {
        Expr::Ident(n) => n.clone(),
        Expr::BigInt(v) => {
            if *v < 0 {
                format!("-{}n", v.unsigned_abs())
            } else {
                format!("{v}n")
            }
        }
        Expr::Num(s) => s.clone(),
        Expr::Str(s) => quote(s),
        Expr::Bool(b) => b.to_string(),
        Expr::Undefined => "undefined".to_string(),
        Expr::This => "this".to_string(),
        Expr::Array(items) => {
            let items: Vec<String> = items.iter().map(expr).collect();
            format!("[{}]", list(&items))
        }
        Expr::Object(fields) => {
            if fields.is_empty() {
                return "{}".to_string();
            }
            let items: Vec<String> = fields
                .iter()
                .map(|(n, v)| match v {
                    _ if n.is_empty() => format!("...{}", expr(v)),
                    Expr::Ident(i) if i == n => n.clone(),
                    _ => format!("{n}: {}", expr(v)),
                })
                .collect();
            format!("{{ {} }}", list(&items))
        }
        Expr::RefTo(place) => {
            let p = expr(place);
            format!("{{ get v() {{ return {p}; }}, set v(value) {{ {p} = value; }} }}")
        }
        Expr::New(name, args) => {
            let args: Vec<String> = args.iter().map(expr).collect();
            format!("new {name}({})", list(&args))
        }
        Expr::Call(callee, targs, args) => {
            let args: Vec<String> = args.iter().map(expr).collect();
            let targs = if targs.is_empty() {
                String::new()
            } else {
                let t: Vec<String> = targs.iter().map(ty).collect();
                format!("<{}>", t.join(", "))
            };
            format!("{}{targs}({})", wrap(callee, 16), list(&args))
        }
        Expr::Member(base, name) => match base.as_ref() {
            Expr::BigInt(_) | Expr::Num(_) => format!("({}).{name}", expr(base)),
            _ => format!("{}.{name}", wrap(base, 16)),
        },
        Expr::Index(base, index) => format!("{}[{}]", wrap(base, 16), expr(index)),
        Expr::Arrow(params, ret, body) => {
            let simple =
                params.len() == 1 && params.iter().all(|(_, t)| t.is_none()) && ret.is_none();
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| match t {
                    Some(t) => format!("{n}: {}", ty(t)),
                    None => n.clone(),
                })
                .collect();
            let head = if simple {
                ps.join(", ")
            } else {
                format!("({})", ps.join(", "))
            };
            let ret = ret
                .as_ref()
                .map(|r| format!(": {}", ty(r)))
                .unwrap_or_default();
            match body {
                ArrowBody::Expr(b) => match b.as_ref() {
                    Expr::Object(_) => format!("{head}{ret} => ({})", expr(b)),
                    _ => format!("{head}{ret} => {}", wrap(b, 2)),
                },
                ArrowBody::Block(stmts) => {
                    let mut p = Printer {
                        indent: 1,
                        ..Printer::default()
                    };
                    for s in stmts {
                        p.stmt(s);
                    }
                    format!("{head}{ret} => {{\n{}}}", p.out)
                }
            }
        }
        Expr::Binary(l, op, r) => {
            let p = prec(e);
            format!("{} {op} {}", wrap(l, p), wrap(r, p + 1))
        }
        Expr::Unary(op, a) => format!("{op}{}", wrap(a, 14)),
        Expr::Cond(c, a, b) => format!("{} ? {} : {}", wrap(c, 3), wrap(a, 2), wrap(b, 2)),
        Expr::As(a, t) => format!("{} as {}", wrap(a, 10), ty(t)),
        Expr::Spread(a) => format!("...{}", wrap(a, 2)),
        Expr::Template(parts) => {
            let mut out = String::from("`");
            for part in parts {
                match part {
                    Expr::Str(text) => out.push_str(&template_text(text)),
                    other => out.push_str(&format!("${{{}}}", expr(other))),
                }
            }
            out.push('`');
            out
        }
    }
}

/// Text of a template literal with `` ` ``, `\` and `${` escaped.
fn template_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => out.push_str("\\`"),
            '\\' => out.push_str("\\\\"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
