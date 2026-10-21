//! The reader of Go code of native modules (spec chapter 7, section 8.7, rules 8 to 11): a small
//! parser of the top-level `func` and `type` declarations of a package. Bodies are skipped by
//! matching brackets; nothing else of the language is needed.

use std::collections::{BTreeMap, BTreeSet};

use wolf_ast::{NativeSig, NativeTy};

use crate::{Function, Module, ReadError, Wolf};

/// A token of Go.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    /// A number, string, rune or other literal.
    Lit,
    /// Punctuation: `(`, `...`, `*`.
    Punct(&'static str),
    /// A semicolon, written or inserted at the end of a line.
    Semi,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
    column: usize,
}

const PUNCTS: &[&str] = &[
    "...", "<-", "&&", "||", "==", "!=", "<=", ">=", ":=", "++", "--", "+=", "-=", "*=", "/=",
    "<<", ">>", "&^", "(", ")", "[", "]", "{", "}", ",", ";", ".", ":", "*", "&", "+", "-", "/",
    "%", "^", "|", "<", ">", "=", "!", "~",
];

/// The tokens of a Go file, with the semicolons that Go inserts at the end of lines.
fn lex(src: &str) -> Result<Vec<Token>, ReadError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    let at = |i: usize| chars.get(i).copied();
    // Whether a line end after the last token inserts a semicolon.
    let ends_statement = |out: &[Token]| {
        out.last().is_some_and(|t| match &t.tok {
            Tok::Ident(s) => {
                !matches!(
                    s.as_str(),
                    "func"
                        | "type"
                        | "var"
                        | "const"
                        | "import"
                        | "package"
                        | "struct"
                        | "interface"
                        | "map"
                        | "chan"
                        | "go"
                        | "defer"
                        | "if"
                        | "else"
                        | "for"
                        | "switch"
                        | "select"
                        | "case"
                        | "default"
                ) || matches!(s.as_str(), "break" | "continue" | "fallthrough" | "return")
            }
            Tok::Lit => true,
            Tok::Punct(p) => matches!(*p, ")" | "]" | "}" | "++" | "--"),
            Tok::Semi => false,
        })
    };
    let err = |line: usize, column: usize, message: &str| ReadError {
        message: message.to_string(),
        line,
        column,
    };
    while let Some(c) = at(i) {
        let (tl, tc) = (line, col);
        let advance = |n: usize, i: &mut usize, line: &mut usize, col: &mut usize| {
            for _ in 0..n {
                if at(*i) == Some('\n') {
                    *line += 1;
                    *col = 1;
                } else {
                    *col += 1;
                }
                *i += 1;
            }
        };
        if c == '\n' {
            if ends_statement(&out) {
                out.push(Token {
                    tok: Tok::Semi,
                    line: tl,
                    column: tc,
                });
            }
            advance(1, &mut i, &mut line, &mut col);
        } else if c.is_whitespace() {
            advance(1, &mut i, &mut line, &mut col);
        } else if c == '/' && at(i + 1) == Some('/') {
            while at(i).is_some_and(|c| c != '\n') {
                advance(1, &mut i, &mut line, &mut col);
            }
        } else if c == '/' && at(i + 1) == Some('*') {
            let mut newline = false;
            advance(2, &mut i, &mut line, &mut col);
            loop {
                match at(i) {
                    None => return Err(err(tl, tc, "comment not terminated")),
                    Some('*') if at(i + 1) == Some('/') => {
                        advance(2, &mut i, &mut line, &mut col);
                        break;
                    }
                    Some(c) => {
                        newline |= c == '\n';
                        advance(1, &mut i, &mut line, &mut col);
                    }
                }
            }
            if newline && ends_statement(&out) {
                out.push(Token {
                    tok: Tok::Semi,
                    line: tl,
                    column: tc,
                });
            }
        } else if c == '"' || c == '\'' || c == '`' {
            advance(1, &mut i, &mut line, &mut col);
            loop {
                match at(i) {
                    None => return Err(err(tl, tc, "literal not terminated")),
                    Some('\n') if c != '`' => return Err(err(tl, tc, "literal not terminated")),
                    Some('\\') if c != '`' => advance(2, &mut i, &mut line, &mut col),
                    Some(d) if d == c => {
                        advance(1, &mut i, &mut line, &mut col);
                        break;
                    }
                    Some(_) => advance(1, &mut i, &mut line, &mut col),
                }
            }
            out.push(Token {
                tok: Tok::Lit,
                line: tl,
                column: tc,
            });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '_') {
                advance(1, &mut i, &mut line, &mut col);
            }
            let word: String = chars.get(start..i).unwrap_or(&[]).iter().collect();
            out.push(Token {
                tok: Tok::Ident(word),
                line: tl,
                column: tc,
            });
        } else if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            while at(i).is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_') {
                advance(1, &mut i, &mut line, &mut col);
            }
            out.push(Token {
                tok: Tok::Lit,
                line: tl,
                column: tc,
            });
        } else {
            let rest: String = chars
                .get(i..(i + 3).min(chars.len()))
                .unwrap_or(&[])
                .iter()
                .collect();
            let Some(p) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
                return Err(err(tl, tc, &format!("unexpected character `{c}`")));
            };
            advance(p.chars().count(), &mut i, &mut line, &mut col);
            out.push(Token {
                tok: if *p == ";" { Tok::Semi } else { Tok::Punct(p) },
                line: tl,
                column: tc,
            });
        }
    }
    if ends_statement(&out) {
        out.push(Token {
            tok: Tok::Semi,
            line,
            column: col,
        });
    }
    Ok(out)
}

/// A Go type, as far as the reader needs it.
#[derive(Clone, Debug, PartialEq)]
enum GoType {
    /// A type name of the package or a predeclared one.
    Name(String),
    /// `pkg.Name`
    Qualified,
    /// `*T`
    Ptr(Box<GoType>),
    /// `[]T`
    Slice(Box<GoType>),
    /// `map[K]V`
    Map(Box<GoType>, Box<GoType>),
    /// `func(A, ..) R`
    Func(Vec<GoType>, Vec<GoType>),
    /// Anything else: arrays, channels, interfaces, structs, generic instances.
    Other(&'static str),
}

/// A parameter: its name, if any, its type and whether it is variadic.
type Param = (Option<String>, GoType, bool);

/// A top-level declaration that the reader keeps.
enum Decl {
    Func {
        name: String,
        generic: bool,
        params: Vec<Param>,
        results: Vec<Param>,
    },
    Type {
        name: String,
        /// `Some(fields)` for a struct type: each field's name (if exported) and type.
        fields: Option<Vec<(String, GoType)>>,
        generic: bool,
    },
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.peek().cloned();
        self.pos += 1;
        t
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn error(&self, message: &str) -> ReadError {
        let t = self.toks.get(self.pos).or(self.toks.last());
        ReadError {
            message: message.to_string(),
            line: t.map_or(1, |t| t.line),
            column: t.map_or(1, |t| t.column),
        }
    }

    fn expect(&mut self, p: &str) -> Result<(), ReadError> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(self.error(&format!("expected `{p}`")))
        }
    }

    fn ident(&mut self) -> Result<String, ReadError> {
        match self.bump() {
            Some(Tok::Ident(s)) => Ok(s),
            _ => {
                self.pos -= 1;
                Err(self.error("expected a name"))
            }
        }
    }

    /// Skips a bracketed group starting at an opening bracket.
    fn skip_group(&mut self) -> Result<(), ReadError> {
        let mut depth = 0usize;
        loop {
            match self.bump() {
                Some(Tok::Punct("(" | "[" | "{")) => depth += 1,
                Some(Tok::Punct(")" | "]" | "}")) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(_) => {}
                None => return Err(self.error("unclosed bracket")),
            }
        }
    }

    /// Skips to the end of the current top-level statement.
    fn skip_statement(&mut self) -> Result<(), ReadError> {
        loop {
            match self.peek() {
                None => return Ok(()),
                Some(Tok::Semi) => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(Tok::Punct("(" | "[" | "{")) => self.skip_group()?,
                Some(_) => self.pos += 1,
            }
        }
    }

    fn file(&mut self) -> Result<Vec<Decl>, ReadError> {
        let mut decls = Vec::new();
        while let Some(t) = self.peek().cloned() {
            match t {
                Tok::Semi => self.pos += 1,
                Tok::Ident(k) if k == "func" => {
                    self.pos += 1;
                    if let Some(d) = self.func()? {
                        decls.push(d);
                    }
                }
                Tok::Ident(k) if k == "type" => {
                    self.pos += 1;
                    if self.eat("(") {
                        while !self.is(")") {
                            if self.peek().is_none() {
                                return Err(self.error("unclosed `type (`"));
                            }
                            if matches!(self.peek(), Some(Tok::Semi)) {
                                self.pos += 1;
                                continue;
                            }
                            decls.push(self.type_spec()?);
                        }
                        self.expect(")")?;
                    } else {
                        decls.push(self.type_spec()?);
                    }
                }
                _ => self.skip_statement()?,
            }
        }
        Ok(decls)
    }

    /// `func` was read. `None` for a method.
    fn func(&mut self) -> Result<Option<Decl>, ReadError> {
        let method = self.is("(");
        if method {
            self.skip_group()?;
        }
        let name = self.ident()?;
        let mut generic = false;
        if self.is("[") {
            generic = true;
            self.skip_group()?;
        }
        let params = self.params()?;
        let results = self.results()?;
        if self.is("{") {
            self.skip_group()?;
        }
        Ok((!method).then_some(Decl::Func {
            name,
            generic,
            params,
            results,
        }))
    }

    fn type_spec(&mut self) -> Result<Decl, ReadError> {
        let name = self.ident()?;
        let mut generic = false;
        // `type X[T any] ...`; `type X []int` is not generic.
        if self.is("[") && matches!(self.peek_at(1), Some(Tok::Ident(_))) && !self.closes_at(1) {
            generic = true;
            self.skip_group()?;
        }
        self.eat("=");
        let fields = if matches!(self.peek(), Some(Tok::Ident(k)) if k == "struct") {
            self.pos += 1;
            Some(self.struct_fields()?)
        } else {
            self.ty()?;
            None
        };
        if matches!(self.peek(), Some(Tok::Semi)) {
            self.pos += 1;
        }
        Ok(Decl::Type {
            name,
            fields,
            generic,
        })
    }

    /// Whether the bracket opened before `n` closes right after one name: `[N]T` is an array.
    fn closes_at(&self, n: usize) -> bool {
        matches!(self.peek_at(n + 1), Some(Tok::Punct("]")))
    }

    fn struct_fields(&mut self) -> Result<Vec<(String, GoType)>, ReadError> {
        self.expect("{")?;
        let mut fields = Vec::new();
        while !self.is("}") {
            if self.peek().is_none() {
                return Err(self.error("unclosed struct"));
            }
            if matches!(self.peek(), Some(Tok::Semi)) {
                self.pos += 1;
                continue;
            }
            // `A, B T`, or an embedded field `T` / `*T`.
            let mut names = Vec::new();
            let embedded = !matches!(self.peek_at(1), Some(Tok::Ident(_) | Tok::Punct(_)))
                || matches!(self.peek_at(1), Some(Tok::Semi | Tok::Punct("." | "}")))
                || self.is("*");
            if embedded {
                let t = self.ty()?;
                fields.push((String::new(), t));
            } else {
                names.push(self.ident()?);
                while self.eat(",") {
                    names.push(self.ident()?);
                }
                let t = self.ty()?;
                for n in names {
                    fields.push((n, t.clone()));
                }
            }
            // A tag.
            if matches!(self.peek(), Some(Tok::Lit)) {
                self.pos += 1;
            }
        }
        self.expect("}")?;
        Ok(fields)
    }

    fn params(&mut self) -> Result<Vec<Param>, ReadError> {
        self.expect("(")?;
        let mut items: Vec<(Option<String>, Option<GoType>, bool)> = Vec::new();
        while !self.is(")") {
            if self.peek().is_none() {
                return Err(self.error("unclosed parameter list"));
            }
            // `name Type`, `name ...Type`, `Type`, or a lone `name` sharing the next type.
            let named = matches!(self.peek(), Some(Tok::Ident(_)))
                && !matches!(self.peek_at(1), Some(Tok::Punct("," | ")" | ".")));
            if named {
                let n = self.ident()?;
                let variadic = self.eat("...");
                let t = self.ty()?;
                items.push((Some(n), Some(t), variadic));
            } else if matches!(self.peek(), Some(Tok::Ident(_)))
                && matches!(self.peek_at(1), Some(Tok::Punct("," | ")")))
            {
                let n = self.ident()?;
                items.push((Some(n), None, false));
            } else {
                let variadic = self.eat("...");
                let t = self.ty()?;
                items.push((None, Some(t), variadic));
            }
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        // A lone name takes the type of the next named parameter; without any named one, every
        // item is a type.
        let any_named = items.iter().any(|(n, t, _)| n.is_some() && t.is_some());
        let mut out: Vec<Param> = Vec::new();
        let mut pending: Vec<String> = Vec::new();
        for (n, t, v) in items {
            match (n, t) {
                (Some(n), None) if any_named => pending.push(n),
                (Some(n), None) => out.push((None, GoType::Name(n), false)),
                (n, Some(t)) => {
                    for p in pending.drain(..) {
                        out.push((Some(p), t.clone(), v));
                    }
                    out.push((n, t, v));
                }
                (None, None) => {}
            }
        }
        Ok(out)
    }

    fn results(&mut self) -> Result<Vec<Param>, ReadError> {
        if self.is("(") {
            return self.params();
        }
        if self.is("{") || matches!(self.peek(), Some(Tok::Semi) | None) {
            return Ok(Vec::new());
        }
        Ok(vec![(None, self.ty()?, false)])
    }

    fn ty(&mut self) -> Result<GoType, ReadError> {
        match self.peek().cloned() {
            Some(Tok::Punct("*")) => {
                self.pos += 1;
                Ok(GoType::Ptr(Box::new(self.ty()?)))
            }
            Some(Tok::Punct("(")) => {
                self.pos += 1;
                let t = self.ty()?;
                self.expect(")")?;
                Ok(t)
            }
            Some(Tok::Punct("[")) => {
                if matches!(self.peek_at(1), Some(Tok::Punct("]"))) {
                    self.pos += 2;
                    Ok(GoType::Slice(Box::new(self.ty()?)))
                } else {
                    self.skip_group()?;
                    self.ty()?;
                    Ok(GoType::Other("an array"))
                }
            }
            Some(Tok::Punct("<-")) => {
                self.pos += 1;
                self.ty()?;
                Ok(GoType::Other("a channel"))
            }
            Some(Tok::Ident(k)) => {
                self.pos += 1;
                match k.as_str() {
                    "map" => {
                        self.expect("[")?;
                        let key = self.ty()?;
                        self.expect("]")?;
                        let value = self.ty()?;
                        Ok(GoType::Map(Box::new(key), Box::new(value)))
                    }
                    "func" => {
                        let params = self.params()?;
                        let results = if self.is("(") {
                            self.params()?
                        } else if self.starts_type() {
                            vec![(None, self.ty()?, false)]
                        } else {
                            Vec::new()
                        };
                        if params.iter().chain(&results).any(|p| p.2) {
                            return Ok(GoType::Other("a variadic function"));
                        }
                        Ok(GoType::Func(
                            params.into_iter().map(|p| p.1).collect(),
                            results.into_iter().map(|p| p.1).collect(),
                        ))
                    }
                    "chan" => {
                        self.eat("<-");
                        self.ty()?;
                        Ok(GoType::Other("a channel"))
                    }
                    "interface" | "struct" => {
                        if self.is("{") {
                            self.skip_group()?;
                        }
                        Ok(GoType::Other(if k == "struct" {
                            "an unnamed struct"
                        } else {
                            "an interface"
                        }))
                    }
                    _ => {
                        if self.eat(".") {
                            self.ident()?;
                            if self.is("[") {
                                self.skip_group()?;
                            }
                            return Ok(GoType::Qualified);
                        }
                        if self.is("[") && !matches!(self.peek_at(1), Some(Tok::Punct("]"))) {
                            self.skip_group()?;
                            return Ok(GoType::Other("an instance of a generic type"));
                        }
                        Ok(GoType::Name(k))
                    }
                }
            }
            _ => Err(self.error("expected a type")),
        }
    }

    fn starts_type(&self) -> bool {
        match self.peek() {
            Some(Tok::Ident(_)) => true,
            Some(Tok::Punct(p)) => matches!(*p, "*" | "[" | "(" | "<-"),
            _ => false,
        }
    }
}

/// The `snake_case` name of an exported Go name: `CountWords` is `count_words`, `ParseURL` is
/// `parse_url`, `URLPath` is `url_path`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() && i > 0 {
            let prev = chars.get(i - 1).copied().unwrap_or(' ');
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next.is_lowercase())
            {
                out.push('_');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn exported(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

/// Reads the declarations of the files of a Go package of `native/go/` (spec chapter 7, section
/// 8.7, rules 8 to 11). `files` holds the name and text of each `.go` file of the directory.
pub fn read_go(files: &[(String, String)]) -> Result<Module, (String, ReadError)> {
    let mut decls = Vec::new();
    for (name, text) in files {
        if name.ends_with("_test.go")
            || text
                .lines()
                .any(|l| l.trim_start().starts_with("//go:build"))
        {
            continue;
        }
        let toks = lex(text).map_err(|e| (name.clone(), e))?;
        let mut p = Parser { toks, pos: 0 };
        decls.extend(p.file().map_err(|e| (name.clone(), e))?);
    }
    let mut module = Module::default();
    // Types first: struct types are opaque as `*X`, other named types as `X`.
    let mut opaque: BTreeMap<String, bool> = BTreeMap::new();
    for d in &decls {
        let Decl::Type {
            name,
            fields,
            generic,
        } = d
        else {
            continue;
        };
        if !exported(name) {
            continue;
        }
        if *generic {
            module
                .unavailable
                .insert(name.clone(), "it is a generic type".to_string());
            continue;
        }
        match fields {
            Some(fields)
                if !fields.is_empty() && fields.iter().all(|(n, t)| exported(n) && field_ok(t)) =>
            {
                module.unavailable.insert(
                    name.clone(),
                    "a struct with only exported fields becomes a record of wolfpack, which a later version supports".to_string(),
                );
            }
            Some(_) => {
                opaque.insert(name.clone(), true);
                module.types.push(name.clone());
                module.pointers.insert(name.clone());
            }
            None => {
                opaque.insert(name.clone(), false);
                module.types.push(name.clone());
            }
        }
    }
    let reader = Reader { opaque: &opaque };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for d in &decls {
        let Decl::Func {
            name,
            generic,
            params,
            results,
        } = d
        else {
            continue;
        };
        if !exported(name) {
            continue;
        }
        let wolf_name = snake(name);
        if !seen.insert(wolf_name.clone()) {
            module.fns.retain(|f| f.name != wolf_name);
            module.unavailable.insert(
                wolf_name,
                format!("two Go functions have this name in `snake_case`, one of them `{name}`"),
            );
            continue;
        }
        let f = if *generic {
            Err("it has type parameters".to_string())
        } else {
            reader.function(&wolf_name, name, params, results)
        };
        match f {
            Ok(f) => module.fns.push(f),
            Err(reason) => {
                module.unavailable.insert(wolf_name, reason);
            }
        }
    }
    Ok(module)
}

/// Whether a field type of a struct could be a field of a record (rule 9, without opaque types).
fn field_ok(t: &GoType) -> bool {
    match t {
        GoType::Name(n) => scalar(n).is_some(),
        GoType::Slice(x) => field_ok(x),
        GoType::Map(k, v) => field_ok(k) && field_ok(v),
        _ => false,
    }
}

/// A predeclared Go type of `Int`, `Float`, `Bool` or `String`.
fn scalar(name: &str) -> Option<(Wolf, NativeTy)> {
    let ints = [
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
    ];
    Some(match name {
        n if ints.contains(&n) => (Wolf::Int, NativeTy::Int(n.to_string())),
        "byte" => (Wolf::Int, NativeTy::Int("uint8".to_string())),
        "rune" => (Wolf::Int, NativeTy::Int("int32".to_string())),
        "float32" | "float64" => (Wolf::Float, NativeTy::Float(name.to_string())),
        "bool" => (Wolf::Bool, NativeTy::Bool),
        "string" => (Wolf::String, NativeTy::String),
        _ => return None,
    })
}

struct Reader<'a> {
    /// Opaque types, and whether each is used as `*X` (a struct type).
    opaque: &'a BTreeMap<String, bool>,
}

impl Reader<'_> {
    fn function(
        &self,
        wolf_name: &str,
        go_name: &str,
        params: &[Param],
        results: &[Param],
    ) -> Result<Function, String> {
        let target_name = go_name.to_string();
        let mut ps = Vec::new();
        let mut natives = Vec::new();
        for (i, (n, t, variadic)) in params.iter().enumerate() {
            let name = n.clone().unwrap_or_else(|| format!("arg{i}"));
            if *variadic {
                return Err(format!("its parameter `{name}` is variadic"));
            }
            let (w, nt) = self
                .ty(t, true)
                .map_err(|r| format!("its parameter `{name}`: {r}"))?;
            ps.push((snake(&name), w));
            natives.push(nt);
        }
        let types: Vec<&GoType> = results.iter().map(|r| &r.1).collect();
        let (ret, native_ret) = match types.as_slice() {
            [] => (Wolf::Void, NativeTy::Unit),
            [GoType::Name(e)] if e == "error" => (
                Wolf::Result(Box::new(Wolf::Void)),
                NativeTy::Result(Box::new(NativeTy::Unit)),
            ),
            [t] => self.ty(t, false).map_err(|r| format!("its result: {r}"))?,
            [t, GoType::Name(e)] if e == "error" => {
                let (w, n) = self.ty(t, false).map_err(|r| format!("its result: {r}"))?;
                (Wolf::Result(Box::new(w)), NativeTy::Result(Box::new(n)))
            }
            [t, GoType::Name(b)] if b == "bool" => {
                let (w, n) = self.ty(t, false).map_err(|r| format!("its result: {r}"))?;
                (Wolf::Option(Box::new(w)), NativeTy::Option(Box::new(n)))
            }
            _ => {
                return Err(
                    "its results are not one value, `(T, bool)` or `(T, error)`".to_string()
                );
            }
        };
        Ok(Function {
            name: wolf_name.to_string(),
            target_name,
            params: ps,
            ret,
            native: NativeSig {
                params: natives,
                ret: native_ret,
            },
        })
    }

    /// A Go type as a wolfpack type, for a parameter (`param`) or a result.
    fn ty(&self, t: &GoType, param: bool) -> Result<(Wolf, NativeTy), String> {
        match t {
            GoType::Name(n) => {
                if let Some(s) = scalar(n) {
                    return Ok(s);
                }
                match self.opaque.get(n) {
                    Some(false) => Ok((
                        Wolf::Named(n.clone()),
                        NativeTy::Named {
                            name: n.clone(),
                            by_ref: false,
                        },
                    )),
                    Some(true) => Err(format!("`{n}` by value: use `*{n}`")),
                    None if n == "error" => {
                        Err("an `error` that is not the last result".to_string())
                    }
                    None => Err(format!("the type `{n}`, which wolfpack does not map")),
                }
            }
            GoType::Ptr(x) => match x.as_ref() {
                GoType::Name(n) if self.opaque.get(n) == Some(&true) => Ok((
                    Wolf::Named(n.clone()),
                    NativeTy::Named {
                        name: n.clone(),
                        by_ref: true,
                    },
                )),
                _ => Err("a pointer to something other than a struct of the package".to_string()),
            },
            GoType::Slice(x) => {
                let (w, n) = self.inner(x)?;
                Ok((Wolf::List(Box::new(w)), NativeTy::Vec(Box::new(n))))
            }
            GoType::Map(k, v) => {
                let (kw, kn) = self.inner(k)?;
                if !matches!(kw, Wolf::Int | Wolf::String | Wolf::Bool) {
                    return Err("a map whose keys are not integers, strings or `bool`".to_string());
                }
                let (vw, vn) = self.inner(v)?;
                Ok((
                    Wolf::Map(Box::new(kw), Box::new(vw)),
                    NativeTy::Map {
                        kind: "map".to_string(),
                        by_ref: false,
                        key: Box::new(kn),
                        value: Box::new(vn),
                    },
                ))
            }
            GoType::Func(ps, rs) if param => self.callback(ps, rs),
            GoType::Func(..) => Err("a function that is not a parameter".to_string()),
            GoType::Qualified => Err("a type of another package".to_string()),
            GoType::Other(what) => Err(format!("{what}, which wolfpack does not map")),
        }
    }

    /// A type inside a list or a map: no opaque values, which cannot be copied.
    fn inner(&self, t: &GoType) -> Result<(Wolf, NativeTy), String> {
        let named = match t {
            GoType::Name(n) => Some(n),
            GoType::Ptr(x) => match x.as_ref() {
                GoType::Name(n) => Some(n),
                _ => None,
            },
            _ => None,
        };
        if let Some(n) = named.filter(|n| self.opaque.contains_key(*n)) {
            return Err(format!(
                "`{n}` inside a list or a map, where values of extern types cannot be copied"
            ));
        }
        match t {
            GoType::Func(..) => Err("a function inside a list or a map".to_string()),
            other => self.ty(other, false),
        }
    }

    /// A callback in the Go forms of rule 9: parameters and result `int64`, `float64`, `bool`,
    /// `string`, `[]T` of these, or `*X` of a struct of the package.
    fn callback(&self, ps: &[GoType], rs: &[GoType]) -> Result<(Wolf, NativeTy), String> {
        let simple = |t: &GoType| -> Option<Wolf> {
            match t {
                GoType::Name(n) => match n.as_str() {
                    "int64" => Some(Wolf::Int),
                    "float64" => Some(Wolf::Float),
                    "bool" => Some(Wolf::Bool),
                    "string" => Some(Wolf::String),
                    _ => None,
                },
                _ => None,
            }
        };
        let one = |t: &GoType| -> Option<Wolf> {
            match t {
                GoType::Slice(x) => simple(x).map(|w| Wolf::List(Box::new(w))),
                GoType::Ptr(x) => match x.as_ref() {
                    GoType::Name(n) if self.opaque.get(n) == Some(&true) => {
                        Some(Wolf::Named(n.clone()))
                    }
                    _ => None,
                },
                other => simple(other),
            }
        };
        let mut params = Vec::new();
        for p in ps {
            params.push(one(p).ok_or(
                "a callback parameter that wolfpack does not map: callbacks take `int64`, `float64`, `bool`, `string`, `[]T` of these or `*X`",
            )?);
        }
        let ret = match rs {
            [] => Wolf::Void,
            [r] => one(r).ok_or(
                "a callback result that wolfpack does not map: callbacks return `int64`, `float64`, `bool`, `string` or `[]T` of these",
            )?,
            _ => return Err("a callback with several results".to_string()),
        };
        Ok((Wolf::Fn(params, Box::new(ret)), NativeTy::Callback))
    }
}
