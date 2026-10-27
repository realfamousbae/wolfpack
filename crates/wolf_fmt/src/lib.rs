//! `wolf fmt`: the canonical layout of wolfpack source (spec chapter 3, section 8a).
//!
//! The formatter parses a file, builds a [`doc::Doc`] from the syntax tree and prints it within
//! 100 columns. Literals, doc comments and ordinary comments are copied from the source text. The
//! result is checked before it is returned: it must parse to the same tree and keep every comment,
//! so a bug in the formatter never changes a program.

mod doc;

use wolf_ast::{
    self as ast, Arg, Arm, AssignOp, BinaryOp, Block, CondItem, ExprKind, ExternDeclKind, File,
    ForIter, GenericParam, ItemKind, Mode, ParamKind, Pat, PatKind, SectionKind, Stmt, StmtKind,
    Ty, TyKind, UnaryOp, VariantPayload,
};
use wolf_diag::Diagnostic;
use wolf_span::{FileId, Span};

use crate::doc::{Doc, INDENT, WIDTH, cat, flat, flat_width, group, has_hard, nest, text};

/// Why a file was not formatted.
#[derive(Debug)]
pub enum FmtError {
    /// The file has syntax errors; they are returned as they are.
    Syntax(Vec<Diagnostic>),
    /// The formatter produced a different program; this is a bug of the formatter.
    Internal(String),
}

/// Formats one source file.
pub fn format(file: FileId, source: &str) -> Result<String, FmtError> {
    let lexed = wolf_lexer::lex(file, source);
    let parsed = wolf_parser::parse(file, source, &lexed.tokens);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(parsed.diagnostics);
    if !diagnostics.is_empty() {
        return Err(FmtError::Syntax(diagnostics));
    }
    let comments: Vec<(u32, u32)> = lexed
        .comments
        .iter()
        .map(|c| (c.span.start, c.span.end))
        .collect();
    let mut f = Fmt {
        src: source,
        comments: comments.clone(),
        next: 0,
        depth: 0,
    };
    let d = f.file(&parsed.file);
    let mut out = doc::print(&d);
    while out.ends_with('\n') || out.ends_with(' ') {
        out.pop();
    }
    out.push('\n');
    check(file, source, &parsed.file, &comments, &out)?;
    Ok(out)
}

/// The formatted text must be the same program with the same comments.
fn check(
    file: FileId,
    source: &str,
    tree: &File,
    comments: &[(u32, u32)],
    out: &str,
) -> Result<(), FmtError> {
    let lexed = wolf_lexer::lex(file, out);
    let parsed = wolf_parser::parse(file, out, &lexed.tokens);
    if !lexed.diagnostics.is_empty() || !parsed.diagnostics.is_empty() {
        return Err(FmtError::Internal(
            "the formatted text has syntax errors".to_string(),
        ));
    }
    if ast::dump(&parsed.file) != ast::dump(tree) {
        return Err(FmtError::Internal(
            "the formatted text has a different syntax tree".to_string(),
        ));
    }
    let text_of = |s: &str, (a, b): (u32, u32)| {
        s.get(a as usize..b as usize)
            .unwrap_or("")
            .trim_end()
            .to_string()
    };
    let before: Vec<String> = comments.iter().map(|c| text_of(source, *c)).collect();
    let after: Vec<String> = lexed
        .comments
        .iter()
        .map(|c| text_of(out, (c.span.start, c.span.end)))
        .collect();
    if before != after {
        return Err(FmtError::Internal(
            "the formatted text lost or reordered a comment".to_string(),
        ));
    }
    Ok(())
}

struct Fmt<'a> {
    src: &'a str,
    /// Ordinary comments: start and end offsets, in source order.
    comments: Vec<(u32, u32)>,
    /// The first comment not printed yet.
    next: usize,
    /// The indentation level of the statement or member being built.
    depth: usize,
}

/// Something printed one per line: its start (before its docs) and end in the source, and how
/// to build it. It is built after the comments before it are printed, so that a nested element
/// never takes a comment that belongs before the outer one.
struct Element<'b, 'a> {
    start: u32,
    end: u32,
    build: Box<dyn FnOnce(&mut Fmt<'a>) -> Doc + 'b>,
}

impl<'a> Fmt<'a> {
    fn src(&self, span: Span) -> &str {
        self.src
            .get(span.start as usize..span.end as usize)
            .unwrap_or("")
    }

    fn between(&self, a: u32, b: u32) -> &str {
        if a >= b {
            return "";
        }
        self.src.get(a as usize..b as usize).unwrap_or("")
    }

    /// Whether the source has a blank line between `a` and `b`.
    fn blank_between(&self, a: u32, b: u32) -> bool {
        let gap = self.between(a, b);
        let mut newlines = 0;
        for c in gap.chars() {
            if c == '\n' {
                newlines += 1;
                if newlines >= 2 {
                    return true;
                }
            } else if !c.is_whitespace() {
                newlines = 0;
            }
        }
        false
    }

    fn comment_text(&self, (a, b): (u32, u32)) -> String {
        self.between(a, b).trim_end().to_string()
    }

    /// Whether a comment starts inside the range.
    fn comment_inside(&self, a: u32, b: u32) -> bool {
        self.comments
            .get(self.next..)
            .unwrap_or(&[])
            .iter()
            .any(|(s, _)| *s >= a && *s < b)
    }

    /// The comments before `pos` that are not printed yet, each on its own line.
    fn leading(&mut self, pos: u32, prev_end: Option<u32>) -> Vec<Doc> {
        let mut out = Vec::new();
        let mut last = prev_end;
        while let Some(&(s, e)) = self.comments.get(self.next) {
            if s >= pos {
                break;
            }
            if let Some(p) = last {
                if self.blank_between(p, s) && !out.is_empty() {
                    out.push(Doc::Hard);
                }
            }
            out.push(text(self.comment_text((s, e))));
            out.push(Doc::Hard);
            last = Some(e);
            self.next += 1;
        }
        if let Some(p) = last {
            if !out.is_empty() && self.blank_between(p, pos) {
                out.push(Doc::Hard);
            }
        }
        out
    }

    /// A comment after `end` on the same source line.
    fn trailing(&mut self, end: u32) -> Option<Doc> {
        let mut out = String::new();
        let mut prev = end;
        while let Some(&(s, e)) = self.comments.get(self.next) {
            if s < prev || self.between(prev, s).contains('\n') {
                break;
            }
            self.next += 1;
            out.push(' ');
            out.push_str(&self.comment_text((s, e)));
            prev = e;
        }
        (!out.is_empty()).then(|| text(out))
    }

    /// Elements one per line inside braces that close at `close`, with comments and blank lines
    /// kept. The result starts with a newline and is nested; the caller adds the closing line.
    fn lines(&mut self, elements: Vec<Element<'_, 'a>>, close: u32) -> Doc {
        let mut parts = Vec::new();
        let mut prev: Option<u32> = None;
        for el in elements {
            let blank = prev.is_some_and(|p| self.blank_between(p, self.first_before(el.start)));
            let lead = self.leading(el.start, None);
            parts.push(Doc::Hard);
            if blank {
                parts.push(Doc::Hard);
            }
            parts.extend(lead);
            parts.push((el.build)(self));
            if let Some(t) = self.trailing(el.end) {
                parts.push(t);
            }
            prev = Some(el.end);
        }
        let blank = prev.is_some_and(|p| self.blank_between(p, self.first_before(close)));
        let mut rest = self.leading(close, None);
        if !rest.is_empty() {
            parts.push(Doc::Hard);
            if blank {
                parts.push(Doc::Hard);
            }
            // The closing line follows; no newline after the last comment.
            while matches!(rest.last(), Some(Doc::Hard)) {
                rest.pop();
            }
            parts.extend(rest);
        }
        nest(cat(parts))
    }

    /// The start of the first comment not printed yet if it is before `pos`, else `pos`.
    fn first_before(&self, pos: u32) -> u32 {
        self.comments
            .get(self.next)
            .map(|c| c.0)
            .filter(|s| *s < pos)
            .unwrap_or(pos)
    }

    // ----- comments inside a line --------------------------------------------------------------

    /// The comments before `pos` that are not printed yet, as a prefix of the expression or
    /// element that starts at `pos`. A block comment stays on the line, followed by one space; a
    /// line comment, or a comment that ends its source line, is followed by a line break.
    fn lead_prefix(&mut self, pos: u32, in_list: bool) -> Option<Doc> {
        let mut parts = Vec::new();
        let mut nested = None;
        while let Some(&(s, e)) = self.comments.get(self.next) {
            let t = self.comment_text((s, e));
            if s >= pos || t.contains('\n') {
                break;
            }
            if nested.is_none() {
                nested = Some(!in_list);
            }
            self.next += 1;
            let hard = t.starts_with("//") || self.between(e, pos).contains('\n');
            parts.push(text(t));
            parts.push(if hard { Doc::Hard } else { text(" ") });
        }
        if parts.is_empty() {
            return None;
        }
        let d = cat(parts);
        Some(if nested == Some(true) { nest(d) } else { d })
    }

    /// The block comments before `pos` that are not printed yet, each after one space.
    fn blocks_before(&mut self, pos: u32) -> Vec<Doc> {
        let mut out = Vec::new();
        while let Some(&(s, e)) = self.comments.get(self.next) {
            let t = self.comment_text((s, e));
            if s >= pos || t.starts_with("//") || t.contains('\n') {
                break;
            }
            self.next += 1;
            out.push(text(format!(" {t}")));
        }
        out
    }

    /// Whether a line comment starts in `a..b` and is not printed yet.
    fn pending_line_comment(&self, a: u32, b: u32) -> bool {
        self.comments
            .get(self.next..)
            .unwrap_or(&[])
            .iter()
            .any(|&(s, e)| s >= a && s < b && self.comment_text((s, e)).starts_with("//"))
    }

    /// The start of the first token at or after `from`, skipping blanks and comments.
    fn token_start(&self, from: u32) -> u32 {
        let mut pos = from as usize;
        loop {
            let rest = self.src.get(pos..).unwrap_or("");
            let trimmed = rest.trim_start();
            pos += rest.len() - trimmed.len();
            if trimmed.starts_with("//") {
                pos += trimmed.find('\n').unwrap_or(trimmed.len());
            } else if trimmed.starts_with("/*") {
                pos += trimmed.find("*/").map_or(trimmed.len(), |i| i + 2);
            } else {
                return pos as u32;
            }
        }
    }

    /// The comments around an element of a list that are not printed yet: the ones before its
    /// comma stay before it; a line comment on the line of the comma goes after the comma; a block
    /// comment after the comma goes with the next element if that follows on the same line.
    /// `next` is the start of the next element, `close` the end of the list.
    fn item_tail(&mut self, doc: Doc, end: u32, next: Option<u32>, close: u32) -> Item {
        let limit = next.unwrap_or(close);
        let mut block = Vec::new();
        let mut line = None;
        let mut prev = end;
        let mut comma = false;
        while let Some(&(s, e)) = self.comments.get(self.next) {
            let t = self.comment_text((s, e));
            if s < prev || s >= limit || t.contains('\n') {
                break;
            }
            let gap = self.between(prev, s);
            if gap.contains('\n') {
                break;
            }
            comma |= gap.contains(',');
            if t.starts_with("//") {
                self.next += 1;
                line = Some(t);
                break;
            }
            if !comma || next.is_none() || self.between(e, limit).contains('\n') {
                self.next += 1;
                block.push(t);
                prev = e;
            } else {
                break;
            }
        }
        Item { doc, block, line }
    }

    // ----- file and sections ----------------------------------------------------------------

    fn file(&mut self, file: &File) -> Doc {
        let mut parts = Vec::new();
        let mut prev: Option<u32> = None;
        for d in &file.docs {
            parts.extend(self.leading(d.span.start, prev));
            if prev.is_some_and(|p| self.blank_between(p, d.span.start)) {
                parts.push(Doc::Hard);
            }
            parts.push(text(self.src(d.span).trim_end()));
            parts.push(Doc::Hard);
            prev = Some(d.span.end);
        }
        if let Some(m) = &file.module {
            parts.extend(self.leading(m.span.start, prev));
            if prev.is_some_and(|p| self.blank_between(p, m.span.start)) {
                parts.push(Doc::Hard);
            }
            parts.push(text(format!("module {}", path(m))));
            if let Some(t) = self.trailing(m.span.end) {
                parts.push(t);
            }
            parts.push(Doc::Hard);
            prev = Some(m.span.end);
        }
        for s in &file.sections {
            if prev.is_some() {
                parts.push(Doc::Hard);
            }
            parts.extend(self.leading(s.name_span.start, None));
            if let (SectionKind::Main, Some(ItemKind::Fn(decl))) =
                (s.kind, s.items.first().map(|i| &i.kind))
            {
                parts.push(self.main_header(&decl.sig));
                if let Some(b) = &decl.body {
                    parts.push(text(" "));
                    self.depth = 0;
                    parts.push(self.block(b, true));
                }
                if let Some(t) = self.trailing(s.span.end) {
                    parts.push(t);
                }
                parts.push(Doc::Hard);
                prev = Some(s.span.end);
                continue;
            }
            parts.push(text(format!("{} {{", s.kind.name())));
            self.depth = 1;
            let elements: Vec<Element> = s.items.iter().map(Self::item).collect();
            parts.push(self.lines(elements, s.span.end.saturating_sub(1)));
            self.depth = 0;
            parts.push(Doc::Hard);
            parts.push(text("}"));
            if let Some(t) = self.trailing(s.span.end) {
                parts.push(t);
            }
            parts.push(Doc::Hard);
            prev = Some(s.span.end);
        }
        let rest = self.leading(u32::MAX, prev);
        if !rest.is_empty() {
            parts.push(Doc::Hard);
            parts.extend(rest);
        }
        cat(parts)
    }

    fn docs(&self, docs: &[ast::Doc]) -> Vec<Doc> {
        let mut out = Vec::new();
        for d in docs {
            out.push(text(self.src(d.span).trim_end()));
            out.push(Doc::Hard);
        }
        out
    }

    fn element<'b>(
        docs: &'b [ast::Doc],
        span: Span,
        build: impl FnOnce(&mut Fmt<'a>) -> Doc + 'b,
    ) -> Element<'b, 'a> {
        let start = docs
            .first()
            .map_or(span.start, |d| d.span.start.min(span.start));
        Element {
            start,
            end: span.end,
            build: Box::new(move |f| {
                let mut parts = f.docs(docs);
                parts.push(build(f));
                cat(parts)
            }),
        }
    }

    // ----- items ------------------------------------------------------------------------------

    fn item<'b>(item: &'b ast::Item) -> Element<'b, 'a> {
        Self::element(&item.docs, item.span, move |f| f.item_doc(item))
    }

    fn item_doc(&mut self, item: &ast::Item) -> Doc {
        match &item.kind {
            ItemKind::Import(i) => {
                let mut s = format!("{}{}", vis(i.public), path(&i.path));
                if let Some(a) = &i.alias {
                    s.push_str(&format!(" as {}", a.name));
                }
                let mut parts = vec![text(s)];
                if let Some(names) = &i.names {
                    let items: Vec<Doc> = names
                        .iter()
                        .map(|n| match &n.alias {
                            Some(a) => text(format!("{} as {}", n.name.name, a.name)),
                            None => text(n.name.name.clone()),
                        })
                        .collect();
                    parts.push(delimited(".{", "}", items));
                }
                cat(parts)
            }
            ItemKind::ExternTarget(t) => {
                let close = item.span.end.saturating_sub(1);
                let elements = t
                    .decls
                    .iter()
                    .map(|d| {
                        Self::element(&d.docs, d.span, move |f| {
                            let head = match &d.kind {
                                ExternDeclKind::Fn(sig) => {
                                    cat(vec![text(vis(d.public)), f.sig(sig)])
                                }
                                ExternDeclKind::Type(n) => {
                                    text(format!("{}type {}", vis(d.public), n.name))
                                }
                                ExternDeclKind::Struct(n) => text(format!("struct {}", n.name)),
                            };
                            cat(vec![head, text(format!(" from {}", f.src(d.from.span)))])
                        })
                    })
                    .collect();
                let inner = self.nested(elements, close);
                braces(text(format!("{} {{", t.target.name)), inner)
            }
            ItemKind::Config(c) => {
                let head = format!("{}{}: ", vis(c.public), c.name.name);
                let head = cat(vec![text(head), self.ty(&c.ty), text(" = ")]);
                self.assigned(head, &c.value)
            }
            ItemKind::Struct(s) => {
                let head = text(format!(
                    "{}struct {}{} {{",
                    vis(s.public),
                    s.name.name,
                    self.generics(&s.generics)
                ));
                if s.fields.is_empty() {
                    text(format!(
                        "{}struct {}{} {{}}",
                        vis(s.public),
                        s.name.name,
                        self.generics(&s.generics)
                    ))
                } else {
                    let close = item.span.end.saturating_sub(1);
                    let inner = self.nested(Self::fields(&s.fields), close);
                    braces(head, inner)
                }
            }
            ItemKind::Enum(e) => {
                let head = text(format!(
                    "{}enum {}{} {{",
                    vis(e.public),
                    e.name.name,
                    self.generics(&e.generics)
                ));
                let close = item.span.end.saturating_sub(1);
                let elements = e
                    .variants
                    .iter()
                    .map(|v| Self::element(&v.docs, v.span, move |f| f.variant(v)))
                    .collect();
                let inner = self.nested(elements, close);
                braces(head, inner)
            }
            ItemKind::Alias(a) => cat(vec![
                text(format!(
                    "{}type {}{} = ",
                    vis(a.public),
                    a.name.name,
                    self.generics(&a.generics)
                )),
                self.ty(&a.ty),
            ]),
            ItemKind::IntrinsicType(t) => text(format!(
                "{}intrinsic type {}{}",
                vis(t.public),
                t.name.name,
                self.generics(&t.generics)
            )),
            ItemKind::Interface(i) => {
                let supers: Vec<String> = i.supers.iter().map(|t| self.ty_text(t)).collect();
                let supers = if supers.is_empty() {
                    String::new()
                } else {
                    format!(": {}", supers.join(" + "))
                };
                let head = text(format!(
                    "{}interface {}{}{supers} {{",
                    vis(i.public),
                    i.name.name,
                    self.generics(&i.generics)
                ));
                let close = item.span.end.saturating_sub(1);
                let elements = i
                    .methods
                    .iter()
                    .map(|m| {
                        let span = match (&m.decl.overrides, &m.decl.body) {
                            (Some(o), Some(b)) => o.to(b.span),
                            (Some(o), None) => o.to(m.decl.sig.span),
                            (None, Some(b)) => m.decl.sig.span.to(b.span),
                            (None, None) => m.decl.sig.span,
                        };
                        Self::element(&m.docs, span, move |f| f.fn_decl(&m.decl))
                    })
                    .collect();
                let inner = self.nested(elements, close);
                braces(head, inner)
            }
            ItemKind::Impl(b) => {
                let mut head = vec![text(format!(
                    "{}{}",
                    b.type_name.name,
                    self.impl_args(&b.args)
                ))];
                if let Some(i) = &b.interface {
                    head.push(text(": "));
                    head.push(self.ty(i));
                }
                head.push(text(" {"));
                // A block that defines nothing, such as one that takes every default, is `{}`.
                if b.methods.is_empty() && !self.comment_inside(item.span.start, item.span.end) {
                    head.push(text("}"));
                    return cat(head);
                }
                let close = item.span.end.saturating_sub(1);
                let elements = b
                    .methods
                    .iter()
                    .map(|m| Self::element(&m.docs, m.span, move |f| f.fn_decl(&m.decl)))
                    .collect();
                let inner = self.nested(elements, close);
                braces(cat(head), inner)
            }
            ItemKind::Fn(decl) => self.fn_decl(decl),
            ItemKind::Test(t) => cat(vec![
                text(format!("test {} ", self.src(t.name.span))),
                self.block(&t.body, true),
            ]),
        }
    }

    /// Elements of a nested declaration, built one level deeper.
    fn nested(&mut self, elements: Vec<Element<'_, 'a>>, close: u32) -> Doc {
        self.depth += 1;
        let d = self.lines(elements, close);
        self.depth -= 1;
        d
    }

    fn fields<'b>(fields: &'b [ast::Field]) -> Vec<Element<'b, 'a>> {
        fields
            .iter()
            .map(|fl| {
                Self::element(&fl.docs, fl.span, move |f| {
                    cat(vec![
                        text(format!("{}{}: ", vis(fl.public), fl.name.name)),
                        f.ty(&fl.ty),
                    ])
                })
            })
            .collect()
    }

    fn variant(&mut self, v: &ast::Variant) -> Doc {
        match &v.payload {
            VariantPayload::None => text(v.name.name.clone()),
            VariantPayload::Tuple(tys) => {
                let items: Vec<Doc> = tys.iter().map(|t| self.ty(t)).collect();
                cat(vec![text(v.name.name.clone()), delimited("(", ")", items)])
            }
            VariantPayload::Struct(fields) => {
                if fields.iter().all(|f| f.docs.is_empty())
                    && !self.comment_inside(v.span.start, v.span.end)
                {
                    let items: Vec<Doc> = fields
                        .iter()
                        .map(|f| cat(vec![text(format!("{}: ", f.name.name)), self.ty(&f.ty)]))
                        .collect();
                    cat(vec![
                        text(format!("{} ", v.name.name)),
                        spaced_braces(items),
                    ])
                } else {
                    let close = v.span.end.saturating_sub(1);
                    let inner = self.nested(Self::fields(fields), close);
                    braces(text(format!("{} {{", v.name.name)), inner)
                }
            }
        }
    }

    fn fn_decl(&mut self, decl: &ast::FnDecl) -> Doc {
        let intrinsic = if decl.intrinsic { "intrinsic " } else { "" };
        let overrides = if decl.overrides.is_some() {
            "override "
        } else {
            ""
        };
        let mut parts = vec![
            text(format!("{}{intrinsic}{overrides}", vis(decl.public))),
            self.sig(&decl.sig),
        ];
        if let Some(b) = &decl.body {
            parts.push(text(" "));
            parts.push(self.block(b, true));
        }
        cat(parts)
    }

    /// `main`, `main(args)`, `main -> T` or `main(args: List<String>) -> T`, as written: a
    /// parameter whose type has an empty span was written without a type.
    fn main_header(&mut self, sig: &ast::FnSig) -> Doc {
        let mut parts = vec![text("main")];
        let params: Vec<Doc> = sig
            .params
            .iter()
            .map(|p| {
                let m = mode(p.mode);
                match &p.kind {
                    ParamKind::SelfParam => text(format!("{m}self")),
                    ParamKind::Named { name, ty } if ty.span.start == ty.span.end => {
                        text(format!("{m}{}", name.name))
                    }
                    ParamKind::Named { name, ty } => {
                        cat(vec![text(format!("{m}{}: ", name.name)), self.ty(ty)])
                    }
                }
            })
            .collect();
        if !params.is_empty() {
            parts.push(delimited("(", ")", params));
        }
        if let Some(r) = &sig.ret {
            parts.push(text(" -> "));
            parts.push(self.ty(r));
        }
        if let Some(w) = self.where_bounds(&sig.where_bounds) {
            parts.push(text(w));
        }
        cat(parts)
    }

    fn sig(&mut self, sig: &ast::FnSig) -> Doc {
        let params: Vec<Doc> = sig
            .params
            .iter()
            .map(|p| {
                let m = mode(p.mode);
                match &p.kind {
                    ParamKind::SelfParam => text(format!("{m}self")),
                    ParamKind::Named { name, ty } => {
                        cat(vec![text(format!("{m}{}: ", name.name)), self.ty(ty)])
                    }
                }
            })
            .collect();
        let mut parts = vec![
            text(format!(
                "fn {}{}",
                sig.name.name,
                self.generics(&sig.generics)
            )),
            delimited("(", ")", params),
        ];
        if let Some(r) = &sig.ret {
            parts.push(text(" -> "));
            parts.push(self.ty(r));
        }
        if let Some(w) = self.where_bounds(&sig.where_bounds) {
            parts.push(text(w));
        }
        cat(parts)
    }

    /// ` where T: A + B, U: C`, or nothing.
    fn where_bounds(&self, bounds: &[GenericParam]) -> Option<String> {
        if bounds.is_empty() {
            return None;
        }
        let list: Vec<String> = bounds
            .iter()
            .map(|g| {
                let b: Vec<String> = g.bounds.iter().map(|t| self.ty_text(t)).collect();
                format!("{}: {}", g.name.name, b.join(" + "))
            })
            .collect();
        Some(format!(" where {}", list.join(", ")))
    }

    /// The head of an `impl` block: parameters with their bounds and type arguments.
    fn impl_args(&self, args: &[ast::ImplArg]) -> String {
        if args.is_empty() {
            return String::new();
        }
        let list: Vec<String> = args
            .iter()
            .map(|a| {
                let t = self.ty_text(&a.ty);
                if a.bounds.is_empty() {
                    t
                } else {
                    let b: Vec<String> = a.bounds.iter().map(|t| self.ty_text(t)).collect();
                    format!("{t}: {}", b.join(" + "))
                }
            })
            .collect();
        format!("<{}>", list.join(", "))
    }

    fn generics(&self, generics: &[GenericParam]) -> String {
        if generics.is_empty() {
            return String::new();
        }
        let list: Vec<String> = generics
            .iter()
            .map(|g| {
                if g.bounds.is_empty() {
                    g.name.name.clone()
                } else {
                    let b: Vec<String> = g.bounds.iter().map(|t| self.ty_text(t)).collect();
                    format!("{}: {}", g.name.name, b.join(" + "))
                }
            })
            .collect();
        format!("<{}>", list.join(", "))
    }

    // ----- types ------------------------------------------------------------------------------

    fn ty(&self, t: &Ty) -> Doc {
        text(self.ty_text(t))
    }

    fn ty_text(&self, t: &Ty) -> String {
        match &t.kind {
            TyKind::Path { path: p, args } => {
                if args.is_empty() {
                    path(p)
                } else {
                    let a: Vec<String> = args.iter().map(|x| self.ty_text(x)).collect();
                    format!("{}<{}>", path(p), a.join(", "))
                }
            }
            TyKind::Fn { params, ret } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|(m, x)| format!("{}{}", mode(*m), self.ty_text(x)))
                    .collect();
                match ret {
                    Some(r) => format!("fn({}) -> {}", ps.join(", "), self.ty_text(r)),
                    None => format!("fn({})", ps.join(", ")),
                }
            }
            TyKind::SelfType => "Self".to_string(),
            TyKind::Error => self.src(t.span).to_string(),
        }
    }

    // ----- statements -------------------------------------------------------------------------

    /// A block; `one_line` allows `{ x }` when the source has it on one line.
    fn block(&mut self, b: &Block, one_line: bool) -> Doc {
        if one_line {
            if let Some(d) = self.one_line(b) {
                return group(d);
            }
        }
        self.multi_line(b)
    }

    /// `{ x }` without its group, if the block is written on one line with one statement.
    fn one_line(&mut self, b: &Block) -> Option<Doc> {
        if b.stmts.len() != 1
            || self.src(b.span).contains('\n')
            || self.comment_inside(b.span.start, b.span.end)
        {
            return None;
        }
        let s = b.stmts.first()?;
        let saved = self.next;
        self.depth += 1;
        let d = self.stmt(s);
        self.depth -= 1;
        if has_hard(&d) {
            self.next = saved;
            return None;
        }
        Some(cat(vec![
            text("{"),
            nest(cat(vec![Doc::Line, d])),
            Doc::Line,
            text("}"),
        ]))
    }

    fn multi_line(&mut self, b: &Block) -> Doc {
        if b.stmts.is_empty() && !self.comment_inside(b.span.start, b.span.end) {
            return text("{}");
        }
        self.depth += 1;
        let elements: Vec<Element> = b
            .stmts
            .iter()
            .map(|s| Element {
                start: s.span.start,
                end: s.span.end,
                build: Box::new(move |f: &mut Fmt<'a>| f.stmt(s)),
            })
            .collect();
        let inner = self.lines(elements, b.span.end.saturating_sub(1));
        self.depth -= 1;
        cat(vec![text("{"), inner, Doc::Hard, text("}")])
    }

    fn stmt(&mut self, s: &Stmt) -> Doc {
        match &s.kind {
            StmtKind::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let kw = if *mutable { "var" } else { "let" };
                let n = name.as_ref().map_or("_".to_string(), |n| n.name.clone());
                let mut head = vec![text(format!("{kw} {n}"))];
                if let Some(t) = ty {
                    head.push(text(": "));
                    head.push(self.ty(t));
                }
                head.push(text(" = "));
                self.assigned(cat(head), value)
            }
            StmtKind::LetPattern {
                mutable,
                pattern,
                value,
            } => {
                let kw = if *mutable { "var" } else { "let" };
                let head = text(format!("{kw} {} = ", self.pat(pattern)));
                self.assigned(head, value)
            }
            StmtKind::Assign { target, op, value } => {
                let head = cat(vec![
                    self.expr(target),
                    text(format!(" {} ", assign_op(*op))),
                ]);
                self.assigned(head, value)
            }
            StmtKind::While { label, cond, body } => cat(vec![
                text(format!("{}while ", label_text(label))),
                self.expr(cond),
                text(" "),
                self.block(body, true),
            ]),
            StmtKind::WhileLet { label, items, body } => cat(vec![
                text(format!("{}while ", label_text(label))),
                self.cond_items(items),
                text(" "),
                self.block(body, true),
            ]),
            StmtKind::For {
                label,
                var,
                iter,
                body,
            } => {
                let mut parts = vec![text(format!("{}for {} in ", label_text(label), var.name))];
                match iter {
                    ForIter::Collection { mode: m, expr } => {
                        parts.push(text(mode(*m)));
                        parts.push(self.expr(expr));
                    }
                    ForIter::Range {
                        start,
                        end,
                        inclusive,
                    } => {
                        parts.push(self.expr(start));
                        parts.push(text(if *inclusive { "..=" } else { ".." }));
                        parts.push(self.expr(end));
                    }
                }
                parts.push(text(" "));
                parts.push(self.block(body, true));
                cat(parts)
            }
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    /// `head value`, where `head` ends with `= ` or `: Type = `: the value moves to the next line
    /// when that makes it fit (rule 6).
    fn assigned(&mut self, head: Doc, value: &ast::Expr) -> Doc {
        let col = self.depth * INDENT;
        let v = self.expr(value);
        let head_w = flat_width(&head).unwrap_or(0);
        if let Some(w) = flat_width(&v) {
            let next_col = (self.depth + 1) * INDENT;
            // Literals in brackets and braces break inside instead.
            let literal = matches!(
                value.kind,
                ExprKind::List(_) | ExprKind::Map(_) | ExprKind::StructLit { .. }
            );
            if col + head_w + w > WIDTH && next_col + w <= WIDTH && !block_like(value) && !literal {
                let trimmed = match head {
                    Doc::Cat(mut parts) => {
                        if let Some(Doc::Text(t)) = parts.last_mut() {
                            *t = t.trim_end().to_string();
                        }
                        Doc::Cat(parts)
                    }
                    other => other,
                };
                return cat(vec![trimmed, nest(cat(vec![Doc::Hard, v]))]);
            }
        }
        cat(vec![head, v])
    }

    // ----- expressions ------------------------------------------------------------------------

    fn expr(&mut self, e: &ast::Expr) -> Doc {
        let lead = self.lead_prefix(e.span.start, false);
        let d = self.expr_inner(e);
        match lead {
            Some(l) => cat(vec![l, d]),
            None => d,
        }
    }

    /// An element of a list: the comments before it are not nested.
    fn element_expr(&mut self, e: &ast::Expr) -> Doc {
        let lead = self.lead_prefix(e.span.start, true);
        let d = self.expr(e);
        match lead {
            Some(l) => cat(vec![l, d]),
            None => d,
        }
    }

    fn expr_inner(&mut self, e: &ast::Expr) -> Doc {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Error => text(self.src(e.span)),
            ExprKind::Str(_) | ExprKind::FStr(_) => self.string(e.span),
            ExprKind::Bool(b) => text(b.to_string()),
            ExprKind::Unit => text("()"),
            ExprKind::Name(n) => text(n.name.clone()),
            ExprKind::SelfValue => text("self"),
            ExprKind::SelfType => text("Self"),
            ExprKind::Paren(x) => {
                let mut parts = vec![text("("), self.expr(x)];
                parts.extend(self.blocks_before(e.span.end));
                parts.push(text(")"));
                cat(parts)
            }
            ExprKind::List(items) => {
                if items.is_empty() {
                    return self.empty_brackets("[", "]", e.span.end);
                }
                let mut docs = Vec::new();
                for (i, x) in items.iter().enumerate() {
                    let d = self.element_expr(x);
                    let next = items.get(i + 1).map(|n| n.span.start);
                    docs.push(self.item_tail(d, x.span.end, next, e.span.end));
                }
                delimited("[", "]", docs)
            }
            ExprKind::Map(entries) => {
                if entries.is_empty() {
                    return self.empty_brackets("[:", "]", e.span.end);
                }
                let mut docs = Vec::new();
                for (i, (k, v)) in entries.iter().enumerate() {
                    let d = cat(vec![self.element_expr(k), text(": "), self.expr(v)]);
                    let next = entries.get(i + 1).map(|n| n.0.span.start);
                    docs.push(self.item_tail(d, v.span.end, next, e.span.end));
                }
                delimited("[", "]", docs)
            }
            ExprKind::StructLit {
                path: p,
                type_args,
                fields,
                base,
            } => {
                let mut head = path(p);
                if !type_args.is_empty() {
                    let a: Vec<String> = type_args.iter().map(|t| self.ty_text(t)).collect();
                    head.push_str(&format!(".<{}>", a.join(", ")));
                }
                if fields.is_empty() && base.is_none() {
                    return text(format!("{head} {{}}"));
                }
                let mut docs = Vec::new();
                for (i, fi) in fields.iter().enumerate() {
                    let lead = self.lead_prefix(fi.name.span.start, true);
                    let (d, end) = match &fi.value {
                        Some(v) => (
                            cat(vec![text(format!("{}: ", fi.name.name)), self.expr(v)]),
                            v.span.end,
                        ),
                        None => (text(fi.name.name.clone()), fi.name.span.end),
                    };
                    let d = match lead {
                        Some(l) => cat(vec![l, d]),
                        None => d,
                    };
                    let next = fields
                        .get(i + 1)
                        .map(|n| n.name.span.start)
                        .or(base.as_ref().map(|b| b.span.start));
                    docs.push(self.item_tail(d, end, next, e.span.end));
                }
                if let Some(b) = base {
                    let lead = self.lead_prefix(b.span.start, true);
                    let d = cat(vec![text(".."), self.expr(b)]);
                    let d = match lead {
                        Some(l) => cat(vec![l, d]),
                        None => d,
                    };
                    docs.push(self.item_tail(d, b.span.end, None, e.span.end));
                }
                cat(vec![text(format!("{head} ")), spaced_braces(docs)])
            }
            ExprKind::Field { .. }
            | ExprKind::Call { .. }
            | ExprKind::TypeArgs { .. }
            | ExprKind::Index { .. }
            | ExprKind::Slice { .. }
            | ExprKind::Try(_) => self.postfix(e),
            ExprKind::Unary { op, operand } => {
                let o = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                };
                cat(vec![text(o), self.expr(operand)])
            }
            ExprKind::Binary { op, .. } => {
                let mut operands = Vec::new();
                let mut ops = Vec::new();
                flatten(e, prec(*op), &mut operands, &mut ops);
                // Each operand with the operator after it; the line breaks after operators.
                let n = operands.len();
                let mut items = Vec::new();
                let mut has_line = false;
                for (i, x) in operands.iter().enumerate() {
                    let d = self.expr(x);
                    if i + 1 < n {
                        let o = ops.get(i).map_or("", |o: &BinaryOp| o.symbol());
                        // A block comment before the operator stays before it; a line comment
                        // after it stays at the end of its line.
                        let op_start = self.token_start(x.span.end);
                        let mut parts = vec![d];
                        parts.extend(self.blocks_before(op_start));
                        parts.push(text(format!(" {o}")));
                        let op_end = op_start + o.len() as u32;
                        let limit = operands.get(i + 1).map_or(op_end, |y| y.span.start);
                        if let Some(&(s, ce)) = self.comments.get(self.next) {
                            let t = self.comment_text((s, ce));
                            if s >= op_end
                                && s < limit
                                && t.starts_with("//")
                                && !self.between(op_end, s).contains('\n')
                            {
                                self.next += 1;
                                parts.push(text(format!(" {t}")));
                                parts.push(Doc::BreakParent);
                                has_line = true;
                            }
                        }
                        items.push(cat(parts));
                    } else {
                        items.push(d);
                    }
                }
                if has_line {
                    let mut parts = Vec::new();
                    for (i, d) in items.into_iter().enumerate() {
                        if i > 0 {
                            parts.push(Doc::Line);
                        }
                        parts.push(d);
                    }
                    return group(nest(cat(parts)));
                }
                group(nest(Doc::Fill(items)))
            }
            ExprKind::Closure { params, ret, body } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|p| match &p.ty {
                        Some(t) => format!("{}: {}", p.name.name, self.ty_text(t)),
                        None => p.name.name.clone(),
                    })
                    .collect();
                let mut head = format!("|{}|", ps.join(", "));
                if let Some(r) = ret {
                    head.push_str(&format!(" -> {}", self.ty_text(r)));
                }
                cat(vec![text(format!("{head} ")), self.expr(body)])
            }
            ExprKind::Block(b) => self.block(b, true),
            ExprKind::If { .. } | ExprKind::IfLet { .. } => {
                // An `if` whose blocks are all on one line is one group: on one line, or every
                // block broken.
                if let Some(d) = self.if_one_line(e) {
                    return group(d);
                }
                self.if_lines(e)
            }
            ExprKind::Match {
                mode: m,
                scrutinee,
                arms,
            } => {
                let head = cat(vec![
                    text(format!("match {}", mode(*m))),
                    self.expr(scrutinee),
                    text(" {"),
                ]);
                self.depth += 1;
                let elements: Vec<Element> = arms
                    .iter()
                    .map(|a| Element {
                        start: a.span.start,
                        end: a.span.end,
                        build: Box::new(move |f: &mut Fmt<'a>| f.arm(a)),
                    })
                    .collect();
                let inner = self.lines(elements, e.span.end.saturating_sub(1));
                self.depth -= 1;
                cat(vec![head, inner, Doc::Hard, text("}")])
            }
            ExprKind::Return(v) => match v {
                Some(x) => cat(vec![text("return "), self.expr(x)]),
                None => text("return"),
            },
            ExprKind::Loop { label, body } => cat(vec![
                text(format!("{}loop ", label_text(label))),
                self.block(body, true),
            ]),
            ExprKind::Break { label, value } => match (label, value) {
                (Some(l), Some(v)) => cat(vec![text(format!("break {}: ", l.name)), self.expr(v)]),
                (Some(l), None) => text(format!("break {}", l.name)),
                (None, Some(v)) => cat(vec![text("break "), self.expr(v)]),
                (None, None) => text("break"),
            },
            ExprKind::Continue { label } => match label {
                Some(l) => text(format!("continue {}", l.name)),
                None => text("continue"),
            },
        }
    }

    /// A string literal as written. A multiline string keeps its content and moves with the
    /// indentation of the code (chapter 3, section 8a, rule 8).
    fn string(&self, span: Span) -> Doc {
        let source = self.src(span);
        let lines: Vec<&str> = source
            .split('\n')
            .map(|l| l.trim_end_matches('\r'))
            .collect();
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            return text(source);
        };
        if lines.len() < 2 {
            return text(source);
        }
        let indent = &last[..last.len() - last.trim_start().len()];
        let content = lines
            .get(1..lines.len() - 1)
            .unwrap_or_default()
            .iter()
            .map(|l| match l.strip_prefix(indent) {
                Some(rest) => rest.to_string(),
                None => l.trim_start().to_string(),
            })
            .collect();
        Doc::Verbatim(
            first.trim_end().to_string(),
            content,
            last.trim_start().to_string(),
        )
    }

    /// The head of an `if` or an `if let` up to its first block, the block and the `else` part.
    fn if_head<'e>(
        &mut self,
        e: &'e ast::Expr,
    ) -> Option<(Vec<Doc>, &'e ast::Block, Option<&'e ast::Expr>)> {
        match &e.kind {
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => Some((
                vec![text("if "), self.expr(cond), text(" ")],
                then,
                otherwise.as_deref(),
            )),
            ExprKind::IfLet {
                items,
                then,
                otherwise,
            } => Some((
                vec![text("if "), self.cond_items(items), text(" ")],
                then,
                otherwise.as_deref(),
            )),
            _ => None,
        }
    }

    /// The items of an `if let` or a `while let`; a chain that does not fit breaks after every
    /// `&&`, one item per line (spec chapter 3, section 8a, rule 6).
    fn cond_items(&mut self, items: &[CondItem]) -> Doc {
        let n = items.len();
        let mut docs = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let d = match item {
                CondItem::Let {
                    pattern,
                    mode: m,
                    value,
                } => cat(vec![
                    text(format!("let {} = {}", self.pat(pattern), mode(*m))),
                    self.expr(value),
                ]),
                CondItem::Bool(e) => self.expr(e),
            };
            if i + 1 < n {
                docs.extend([d, text(" &&"), Doc::Line]);
            } else {
                docs.push(d);
            }
        }
        if n == 1 {
            return docs.pop().unwrap_or_else(|| text(""));
        }
        group(nest(cat(docs)))
    }

    fn if_one_line(&mut self, e: &ast::Expr) -> Option<Doc> {
        let saved = self.next;
        let (mut parts, then, otherwise) = self.if_head(e)?;
        let Some(t) = self.one_line(then) else {
            self.next = saved;
            return None;
        };
        parts.push(t);
        if let Some(o) = otherwise {
            parts.push(text(" else "));
            let rest = match &o.kind {
                ExprKind::Block(b) => self.one_line(b),
                ExprKind::If { .. } | ExprKind::IfLet { .. } => self.if_one_line(o),
                _ => None,
            };
            let Some(r) = rest else {
                self.next = saved;
                return None;
            };
            parts.push(r);
        }
        Some(cat(parts))
    }

    fn if_lines(&mut self, e: &ast::Expr) -> Doc {
        let Some((mut parts, then, otherwise)) = self.if_head(e) else {
            return self.expr(e);
        };
        parts.push(self.multi_line(then));
        if let Some(o) = otherwise {
            parts.push(text(" else "));
            match &o.kind {
                ExprKind::Block(b) => parts.push(self.multi_line(b)),
                ExprKind::If { .. } | ExprKind::IfLet { .. } => parts.push(self.if_lines(o)),
                _ => parts.push(self.expr(o)),
            }
        }
        cat(parts)
    }

    fn arm(&mut self, a: &Arm) -> Doc {
        let mut parts = vec![text(self.pat(&a.pattern))];
        if let Some(g) = &a.guard {
            parts.push(text(" if "));
            parts.push(self.expr(g));
        }
        parts.push(text(" => "));
        parts.push(self.expr(&a.body));
        cat(parts)
    }

    fn pat(&self, p: &Pat) -> String {
        match &p.kind {
            PatKind::Wild => "_".to_string(),
            PatKind::Name(n) => n.name.clone(),
            PatKind::Int { .. } | PatKind::Str(_) | PatKind::Error => self.src(p.span).to_string(),
            PatKind::Bool(b) => b.to_string(),
            PatKind::Path(x) => path(x),
            PatKind::Tuple { path: x, items } => {
                let i: Vec<String> = items.iter().map(|y| self.pat(y)).collect();
                format!("{}({})", path(x), i.join(", "))
            }
            PatKind::Struct {
                path: x,
                fields,
                rest,
            } => {
                let mut i: Vec<String> = fields
                    .iter()
                    .map(|f| match &f.pat {
                        Some(y) => format!("{}: {}", f.name.name, self.pat(y)),
                        None => f.name.name.clone(),
                    })
                    .collect();
                if *rest {
                    i.push("..".to_string());
                }
                if i.is_empty() {
                    format!("{} {{}}", path(x))
                } else {
                    format!("{} {{ {} }}", path(x), i.join(", "))
                }
            }
            PatKind::Or(alts) => {
                let a: Vec<String> = alts.iter().map(|y| self.pat(y)).collect();
                a.join(" | ")
            }
        }
    }

    /// Field access, calls, indexing and `?`: a chain of two or more method calls puts each
    /// call on its own line when it does not fit (rule 6).
    fn postfix(&mut self, e: &ast::Expr) -> Doc {
        let mut ops: Vec<&ast::Expr> = Vec::new();
        let mut cur = e;
        while let ExprKind::Field { base, .. }
        | ExprKind::Call { callee: base, .. }
        | ExprKind::TypeArgs { base, .. }
        | ExprKind::Index { base, .. }
        | ExprKind::Slice { base, .. }
        | ExprKind::Try(base) = &cur.kind
        {
            ops.push(cur);
            cur = base;
        }
        ops.reverse();
        let calls = ops
            .iter()
            .filter(|x| {
                matches!(&x.kind, ExprKind::Call { callee, .. } if matches!(callee.kind, ExprKind::Field { .. }))
            })
            .count();
        let base = self.expr(cur);
        // A line comment between two parts of the chain needs a line of the chain to end on.
        let mut gaps = Vec::new();
        let mut prev_end = cur.span.end;
        for op in &ops {
            if let ExprKind::Field { name, .. } = &op.kind {
                gaps.push(Some((prev_end, name.span.start)));
            } else {
                gaps.push(None);
            }
            prev_end = op.span.end;
        }
        let commented = gaps
            .iter()
            .flatten()
            .any(|&(a, b)| self.pending_line_comment(a, b));
        if calls < 2 && !commented {
            let mut parts = vec![base];
            for (op, gap) in ops.into_iter().zip(gaps) {
                if let Some((_, b)) = gap {
                    parts.extend(self.blocks_before(b));
                }
                parts.push(self.postfix_op(op));
            }
            return cat(parts);
        }
        let mut chain = Vec::new();
        for (op, gap) in ops.into_iter().zip(gaps) {
            if let Some((_, b)) = gap {
                chain.extend(self.chain_comments(b));
            }
            if matches!(op.kind, ExprKind::Field { .. }) {
                chain.push(Doc::Soft);
            }
            chain.push(self.postfix_op(op));
        }
        group(cat(vec![base, nest(cat(chain))]))
    }

    /// The comments before `pos` inside a chain, each after one space; a line comment ends its
    /// line.
    fn chain_comments(&mut self, pos: u32) -> Vec<Doc> {
        let mut out = Vec::new();
        while let Some(&(s, e)) = self.comments.get(self.next) {
            let t = self.comment_text((s, e));
            if s >= pos || t.contains('\n') {
                break;
            }
            self.next += 1;
            let line = t.starts_with("//");
            out.push(text(format!(" {t}")));
            if line {
                out.push(Doc::BreakParent);
            }
        }
        out
    }

    /// `[]`, `[:]` or `()`, with the block comments inside it.
    fn empty_brackets(&mut self, open: &str, close: &str, end: u32) -> Doc {
        let mut parts = vec![text(open)];
        let inside = self.blocks_before(end);
        for (i, c) in inside.into_iter().enumerate() {
            if let (0, Doc::Text(t)) = (i, &c) {
                parts.push(text(t.trim_start().to_string()));
            } else {
                parts.push(c);
            }
        }
        parts.push(text(close));
        cat(parts)
    }

    fn postfix_op(&mut self, op: &ast::Expr) -> Doc {
        match &op.kind {
            ExprKind::Field { name, .. } => text(format!(".{}", name.name)),
            ExprKind::TypeArgs { args, .. } => {
                let a: Vec<String> = args.iter().map(|t| self.ty_text(t)).collect();
                text(format!(".<{}>", a.join(", ")))
            }
            ExprKind::Call { args, .. } => self.args(args, op.span.end),
            ExprKind::Index { index, .. } => {
                let mut parts = vec![text("["), self.expr(index)];
                parts.extend(self.blocks_before(op.span.end));
                parts.push(text("]"));
                cat(parts)
            }
            ExprKind::Slice {
                start,
                end,
                inclusive,
                ..
            } => {
                let mut parts = vec![text("[")];
                if let Some(s) = start {
                    parts.push(self.expr(s));
                }
                parts.push(text(if *inclusive { "..=" } else { ".." }));
                if let Some(x) = end {
                    parts.push(self.expr(x));
                }
                parts.push(text("]"));
                cat(parts)
            }
            ExprKind::Try(_) => text("?"),
            _ => text(""),
        }
    }

    fn args(&mut self, args: &[Arg], end: u32) -> Doc {
        if args.is_empty() {
            return self.empty_brackets("(", ")", end);
        }
        let mut docs = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let lead = self.lead_prefix(a.value.span.start, true);
            let d = cat(vec![text(mode(a.mode)), self.expr(&a.value)]);
            let d = match lead {
                Some(l) => cat(vec![l, d]),
                None => d,
            };
            let next = args.get(i + 1).map(|n| n.value.span.start);
            docs.push(self.item_tail(d, a.value.span.end, next, end));
        }
        // A last argument that is a closure with a block, a `match`, an `if` or a block stays on
        // the line of the call.
        let hug = args.last().is_some_and(|a| huggable(&a.value))
            && docs.iter().all(|d| d.block.is_empty() && d.line.is_none())
            && docs.last().is_some_and(|d| has_hard(&d.doc))
            && docs.iter().rev().skip(1).all(|d| !has_hard(&d.doc));
        if hug {
            let mut parts = vec![text("(")];
            let n = docs.len();
            for (i, d) in docs.into_iter().enumerate() {
                if i + 1 < n {
                    parts.push(flat(d.doc));
                    parts.push(text(", "));
                } else {
                    parts.push(d.doc);
                }
            }
            parts.push(text(")"));
            return cat(parts);
        }
        delimited("(", ")", docs)
    }
}

/// An element of a list with the comments around it.
struct Item {
    doc: Doc,
    /// Block comments before the comma, each printed after one space.
    block: Vec<String>,
    /// A line comment after the comma.
    line: Option<String>,
}

impl From<Doc> for Item {
    fn from(doc: Doc) -> Item {
        Item {
            doc,
            block: Vec::new(),
            line: None,
        }
    }
}

/// The elements one after another, separated by `,` and a line, with a trailing comma when the
/// group is broken.
fn items_inner<I: Into<Item>>(mut inner: Vec<Doc>, items: Vec<I>) -> Vec<Doc> {
    let n = items.len();
    for (i, item) in items.into_iter().enumerate() {
        let item: Item = item.into();
        inner.push(item.doc);
        for b in item.block {
            inner.push(text(format!(" {b}")));
        }
        if i + 1 < n {
            inner.push(text(","));
        } else {
            inner.push(Doc::IfBreak(",".to_string()));
        }
        if let Some(l) = item.line {
            inner.push(text(format!(" {l}")));
            inner.push(Doc::BreakParent);
        }
        if i + 1 < n {
            inner.push(Doc::Line);
        }
    }
    inner
}

/// `open items close` on one line, or one item per line with a trailing comma.
fn delimited<I: Into<Item>>(open: &str, close: &str, items: Vec<I>) -> Doc {
    let inner = items_inner(vec![Doc::Soft], items);
    group(cat(vec![
        text(open),
        nest(cat(inner)),
        Doc::Soft,
        text(close),
    ]))
}

/// `{ a, b }` on one line, or one item per line with a trailing comma.
fn spaced_braces<I: Into<Item>>(items: Vec<I>) -> Doc {
    let inner = items_inner(vec![Doc::Line], items);
    group(cat(vec![text("{"), nest(cat(inner)), Doc::Line, text("}")]))
}

/// `head` (ending with `{`), the nested lines, and the closing brace.
fn braces(head: Doc, inner: Doc) -> Doc {
    cat(vec![head, inner, Doc::Hard, text("}")])
}

fn huggable(e: &ast::Expr) -> bool {
    match &e.kind {
        ExprKind::Closure { body, .. } => block_like(body),
        _ => block_like(e),
    }
}

fn block_like(e: &ast::Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Block(_)
            | ExprKind::Match { .. }
            | ExprKind::If { .. }
            | ExprKind::IfLet { .. }
            | ExprKind::Loop { .. }
    )
}

/// `label: ` before a loop, or nothing.
fn label_text(label: &Option<ast::Ident>) -> String {
    label
        .as_ref()
        .map_or(String::new(), |l| format!("{}: ", l.name))
}

/// The operands and operators of a chain of binary operators of one precedence.
fn flatten<'e>(
    e: &'e ast::Expr,
    level: u8,
    operands: &mut Vec<&'e ast::Expr>,
    ops: &mut Vec<BinaryOp>,
) {
    match &e.kind {
        ExprKind::Binary { op, lhs, rhs } if prec(*op) == level => {
            flatten(lhs, level, operands, ops);
            ops.push(*op);
            operands.push(rhs);
        }
        _ => operands.push(e),
    }
}

fn prec(op: BinaryOp) -> u8 {
    use BinaryOp as B;
    match op {
        B::Mul | B::Div | B::Rem => 10,
        B::Add | B::Sub => 9,
        B::Shl | B::Shr => 8,
        B::BitAnd => 7,
        B::BitXor => 6,
        B::BitOr => 5,
        B::Eq | B::NotEq | B::Lt | B::Le | B::Gt | B::Ge => 4,
        B::And => 3,
        B::Or => 2,
    }
}

fn path(p: &ast::Path) -> String {
    p.segments
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn vis(public: bool) -> &'static str {
    if public { "pub " } else { "" }
}

fn mode(m: Mode) -> &'static str {
    match m {
        Mode::Read => "",
        Mode::Mut => "mut ",
        Mode::Own => "own ",
    }
}

fn assign_op(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "=",
        AssignOp::Add => "+=",
        AssignOp::Sub => "-=",
        AssignOp::Mul => "*=",
        AssignOp::Div => "/=",
        AssignOp::Rem => "%=",
        AssignOp::BitAnd => "&=",
        AssignOp::BitOr => "|=",
        AssignOp::BitXor => "^=",
        AssignOp::Shl => "<<=",
        AssignOp::Shr => ">>=",
    }
}
