//! The file skeleton and declarations (spec chapter 2 and chapter 3, section 2).

use std::collections::HashMap;

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_lexer::TokenKind;
use wolf_span::Span;

use crate::Parser;

/// Which form of `fn` is being parsed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FnContext {
    /// A free function: `self` is not allowed.
    Free,
    /// A method in `impl`: `self` may be the first parameter.
    Method,
    /// A signature in an interface or an extern block: no body.
    Signature,
}

impl Parser<'_> {
    pub(crate) fn file(&mut self) -> File {
        let mut file = File {
            docs: Vec::new(),
            module: None,
            sections: Vec::new(),
        };
        self.module_line(&mut file);
        let mut inherent_impls: HashMap<String, Span> = HashMap::new();
        let mut test_names: HashMap<String, Span> = HashMap::new();
        loop {
            self.skip_newlines();
            self.resync_to_top = false;
            if self.too_deep() {
                self.give_up();
            }
            let docs = self.docs();
            self.skip_newlines();
            let token = self.nth(0).clone();
            match &token.kind {
                TokenKind::Eof => {
                    self.stray_docs(&docs, "at the end of the file");
                    break;
                }
                TokenKind::Module => {
                    self.report(
                        Diagnostic::new(codes::E0201, "the `module` line appears twice")
                            .primary(token.span, "second `module` line")
                            .help("a file has exactly one `module` line, the first line"),
                    );
                    self.bump();
                    self.recover();
                }
                _ if self.at_main_header() => {
                    for doc in &docs {
                        self.report(
                            Diagnostic::new(codes::E0212, "doc comment before a section")
                                .primary(doc.span, "sections are not declarations")
                                .help("use `//!` before the `module` line to document the module"),
                        );
                    }
                    if let Some(section) = self.main_section() {
                        file.sections.push(section);
                    }
                }
                _ => {
                    if let Some(kind) = self.section_header() {
                        for doc in &docs {
                            self.report(
                                Diagnostic::new(codes::E0212, "doc comment before a section")
                                    .primary(doc.span, "sections are not declarations")
                                    .help(
                                        "use `//!` before the `module` line to document the module",
                                    ),
                            );
                        }
                        let section = self.section(kind, &mut inherent_impls, &mut test_names);
                        if kind == SectionKind::Functions {
                            self.main_is_a_section(&section);
                        }
                        file.sections.push(section);
                    } else {
                        self.stray_top_level(&docs);
                    }
                }
            }
        }
        file
    }

    /// `fn main` in `functions` is an error: the entry point is the `main` section (spec chapter
    /// 2, section 3.7; E0214).
    fn main_is_a_section(&mut self, section: &Section) {
        for item in &section.items {
            if let ItemKind::Fn(decl) = &item.kind {
                if decl.sig.name.name == "main" {
                    self.report(
                        Diagnostic::new(codes::E0214, "`fn main` in `functions`")
                            .primary(decl.sig.name.span, "the entry point is not a function")
                            .help("declare it as a `main { }` section above `functions`"),
                    );
                }
            }
        }
    }

    /// Whether the next tokens are the header of the `main` section: `main {`, `main(` or
    /// `main ->`.
    fn at_main_header(&self) -> bool {
        let token = self.raw(0);
        token.kind == TokenKind::Ident
            && self.source(token.span) == "main"
            && matches!(
                self.raw(1).kind,
                TokenKind::LBrace | TokenKind::LParen | TokenKind::Arrow
            )
    }

    /// The `main` section: `main(args: List<String>) -> Result<Void, Error> { ... }`, the
    /// parameter list and the result type both optional (spec chapter 2, section 3.7).
    ///
    /// It is parsed into a section with one `fn main`, so that the rest of the compiler sees an
    /// ordinary function. A parameter without a type gets an empty type span at the end of its
    /// name; the formatter uses that to print what was written.
    fn main_section(&mut self) -> Option<Section> {
        let name_span = self.bump().span; // `main`
        let name = Ident {
            name: "main".to_string(),
            span: name_span,
        };
        let mut params: Vec<Param> = Vec::new();
        if self.at(&TokenKind::LParen) {
            let open = self.bump().span;
            self.ignore_lines(|p| {
                while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                    let before = p.pos;
                    let start = p.peek_span();
                    let mode = p.mode();
                    if let Some(pname) = p.ident("a parameter name") {
                        let ty = if p.eat_then_lines(&TokenKind::Colon) {
                            p.ty()
                        } else {
                            p.list_of_string(pname.span)
                        };
                        params.push(Param {
                            mode,
                            kind: ParamKind::Named { name: pname, ty },
                            span: p.span_from(start),
                        });
                    } else {
                        p.recover();
                    }
                    if !p.eat(&TokenKind::Comma) || p.pos == before {
                        break;
                    }
                }
            });
            if self
                .ignore_lines(|p| p.expect(&TokenKind::RParen, "`)`"))
                .is_none()
            {
                self.report(
                    Diagnostic::new(codes::E0302, "unclosed `(`")
                        .primary(open, "this `(` is never closed"),
                );
                self.recover();
            }
        }
        let ret = if self.eat_then_lines(&TokenKind::Arrow) {
            Some(self.ty())
        } else {
            None
        };
        let sig = FnSig {
            name,
            generics: Vec::new(),
            params,
            ret,
            where_bounds: Vec::new(),
            span: self.span_from(name_span),
        };
        let body = if self.at(&TokenKind::LBrace) {
            self.block()
        } else {
            self.unexpected("`{` and the body of `main`");
            None
        };
        if let Some(body) = &body {
            if body.stmts.is_empty() {
                self.report(
                    Diagnostic::new(codes::E0204, "empty section `main`")
                        .primary(name_span, "this section has no statements")
                        .help("remove the section; empty sections are not written"),
                );
            }
        }
        let span = self.span_from(name_span);
        Some(Section {
            kind: SectionKind::Main,
            name_span,
            span,
            items: vec![Item {
                docs: Vec::new(),
                kind: ItemKind::Fn(FnDecl {
                    public: false,
                    intrinsic: false,
                    sig,
                    body,
                }),
                span,
            }],
        })
    }

    /// The type `List<String>` for a `main` parameter written without a type. Its span is empty
    /// and stands at the end of the parameter name.
    fn list_of_string(&mut self, name: Span) -> Ty {
        let at = Span::new(name.file, name.end, name.end);
        let segment = |text: &str| Ident {
            name: text.to_string(),
            span: at,
        };
        let string = Ty {
            id: self.id(),
            kind: TyKind::Path {
                path: Path {
                    segments: vec![segment("String")],
                    span: at,
                },
                args: Vec::new(),
            },
            span: at,
        };
        Ty {
            id: self.id(),
            kind: TyKind::Path {
                path: Path {
                    segments: vec![segment("List")],
                    span: at,
                },
                args: vec![string],
            },
            span: at,
        }
    }

    /// Inner docs and the `module` line (spec chapter 2, section 1).
    fn module_line(&mut self, file: &mut File) {
        loop {
            self.skip_newlines();
            let token = self.nth(0).clone();
            match &token.kind {
                TokenKind::DocInner(text) => {
                    file.docs.push(Doc {
                        text: text.clone(),
                        inner: true,
                        span: token.span,
                    });
                    self.bump();
                }
                TokenKind::DocOuter(_) => {
                    self.report(
                        Diagnostic::new(codes::E0212, "outer doc comment before the `module` line")
                            .primary(token.span, "this documents nothing")
                            .help("use `//!` to document the module"),
                    );
                    self.bump();
                }
                _ => break,
            }
        }
        let start = self.peek_span();
        if !self.eat(&TokenKind::Module) {
            if self.at(&TokenKind::Error) {
                return;
            }
            self.report(
                Diagnostic::new(codes::E0201, "the file does not start with `module`")
                    .primary(start, "expected `module` here")
                    .help("add `module <path>` as the first line, for example `module main`"),
            );
            return;
        }
        if !self.at(&TokenKind::Ident) {
            let span = self.peek_span();
            self.report(
                Diagnostic::new(codes::E0208, "missing module path after `module`").primary(
                    span,
                    "expected the module path, for example `users.storage`",
                ),
            );
            self.recover();
            return;
        }
        file.module = Some(self.path());
        if !self.at_line_end() {
            let span = self.peek_span();
            if self.at(&TokenKind::LBrace) {
                self.report(
                    Diagnostic::new(codes::E0208, "unexpected content after the module path")
                        .primary(span, "the `module` line contains only the path")
                        .help("put the sections on the following lines"),
                );
            } else {
                self.report(
                    Diagnostic::new(codes::E0208, "unexpected content after the module path")
                        .primary(span, "the `module` line contains only the path"),
                );
            }
            self.recover();
        }
    }

    /// A dotted path of identifiers.
    pub(crate) fn path(&mut self) -> Path {
        self.path_from(false)
    }

    /// A dotted path that may start with `Self` (patterns inside `impl`, chapter 3, section 7).
    pub(crate) fn path_or_self(&mut self) -> Path {
        self.path_from(true)
    }

    fn path_from(&mut self, allow_self: bool) -> Path {
        let mut segments = Vec::new();
        let start = self.peek_span();
        if allow_self && self.at(&TokenKind::SelfType) {
            let span = self.bump().span;
            segments.push(Ident {
                name: "Self".to_string(),
                span,
            });
        } else if let Some(first) = self.ident("a name") {
            segments.push(first);
        }
        while self.at(&TokenKind::Dot) && self.ident_after_dot() {
            self.bump();
            self.skip_newlines();
            if let Some(next) = self.ident("a name") {
                segments.push(next);
            }
        }
        Path {
            segments,
            span: self.span_from(start),
        }
    }

    /// Whether the token after the next `.` (possibly on the next line) is an identifier.
    fn ident_after_dot(&self) -> bool {
        let mut ahead = self.visible_index(0) + 1 - self.pos;
        while self.raw(ahead).kind == TokenKind::Newline {
            ahead += 1;
        }
        self.raw(ahead).kind == TokenKind::Ident
    }

    pub(crate) fn ident(&mut self, what: &str) -> Option<Ident> {
        if self.at(&TokenKind::Ident) {
            let token = self.bump();
            Some(Ident {
                name: self.source(token.span).to_string(),
                span: token.span,
            })
        } else {
            self.unexpected(what);
            None
        }
    }

    /// Outer doc comments, each followed by line ends.
    pub(crate) fn docs(&mut self) -> Vec<Doc> {
        let mut docs = Vec::new();
        loop {
            let token = self.nth(0).clone();
            match &token.kind {
                TokenKind::DocOuter(text) => {
                    docs.push(Doc {
                        text: text.clone(),
                        inner: false,
                        span: token.span,
                    });
                    self.bump();
                    self.skip_newlines();
                }
                TokenKind::DocInner(_) => {
                    self.report(
                        Diagnostic::new(codes::E0212, "inner doc comment after the `module` line")
                            .primary(token.span, "`//!` is allowed only before the `module` line")
                            .help("use `///` to document the declaration below"),
                    );
                    self.bump();
                    self.skip_newlines();
                }
                _ => return docs,
            }
        }
    }

    pub(crate) fn stray_docs(&mut self, docs: &[Doc], place: &str) {
        for doc in docs {
            self.report(
                Diagnostic::new(codes::E0212, format!("doc comment {place}"))
                    .primary(doc.span, "this doc comment documents nothing"),
            );
        }
    }

    /// If the next tokens are a section header (`types {`), consumes the name and returns it.
    fn section_header(&mut self) -> Option<SectionKind> {
        let token = self.nth(0).clone();
        let kind = match &token.kind {
            TokenKind::Impl => SectionKind::Impl,
            TokenKind::Extern => SectionKind::Extern,
            TokenKind::Ident => SectionKind::from_name(self.source(token.span))?,
            _ => return None,
        };
        // A section name followed by `{` (possibly on the next line) starts a section.
        let next = self.raw(1).kind.clone();
        let next_line_brace = next == TokenKind::Newline && self.raw(2).kind == TokenKind::LBrace;
        if next != TokenKind::LBrace && !next_line_brace {
            return None;
        }
        self.bump();
        if next_line_brace {
            let span = self.raw(1).span;
            self.report(
                Diagnostic::new(codes::E0301, "expected `{` on the line of the section name")
                    .primary(
                        span,
                        "the `{` must follow the section name on the same line",
                    )
                    .secondary(token.span, "section name"),
            );
            self.skip_newlines();
        }
        Some(kind)
    }

    fn section(
        &mut self,
        kind: SectionKind,
        inherent_impls: &mut HashMap<String, Span>,
        test_names: &mut HashMap<String, Span>,
    ) -> Section {
        let name_span = self.prev_span();
        let open = self.bump().span; // `{`
        let mut items = Vec::new();
        let mut attempted = false;
        self.significant_lines(|p| {
            loop {
                p.skip_newlines();
                if p.resync_to_top {
                    return;
                }
                let docs = p.docs();
                p.skip_newlines();
                match p.peek() {
                    TokenKind::RBrace => {
                        p.stray_docs(&docs, "at the end of a section");
                        return;
                    }
                    TokenKind::Eof => {
                        p.stray_docs(&docs, "at the end of the file");
                        p.report(
                            Diagnostic::new(
                                codes::E0302,
                                format!("unclosed section `{}`", kind.name()),
                            )
                            .primary(open, "this `{` is never closed"),
                        );
                        return;
                    }
                    _ => {}
                }
                if p.starts_section() {
                    // A missing `}`: the next section begins.
                    p.report(
                        Diagnostic::new(
                            codes::E0302,
                            format!("unclosed section `{}`", kind.name()),
                        )
                        .primary(open, "this `{` is never closed")
                        .secondary(p.peek_span(), "the next section starts here"),
                    );
                    p.stray_docs(&docs, "before a section");
                    return;
                }
                let before = p.pos;
                p.line_errors = p.start_line();
                attempted = true;
                if let Some(item) = p.item(kind, docs) {
                    p.check_item(kind, &item, inherent_impls, test_names);
                    items.push(item);
                }
                p.end_of_entry();
                if p.pos == before {
                    // Make progress no matter what.
                    p.bump();
                }
            }
        });
        let close = self.peek_span();
        let closed = self.raw(0).kind == TokenKind::RBrace;
        if closed {
            if attempted && !self.preceded_by_newline() {
                self.report(
                    Diagnostic::new(codes::E0301, "expected a new line before `}`")
                        .primary(close, "the closing `}` of a section stands on its own line"),
                );
            }
            self.bump();
        }
        if closed && !attempted && !self.too_deep() {
            self.report(
                Diagnostic::new(codes::E0204, format!("empty section `{}`", kind.name()))
                    .primary(name_span, "this section has no entries")
                    .help("remove the section; empty sections are not written"),
            );
        }
        Section {
            kind,
            name_span,
            span: name_span.to(self.prev_span()),
            items,
        }
    }

    fn preceded_by_newline(&self) -> bool {
        self.pos > 0
            && self
                .tokens
                .get(self.pos - 1)
                .is_some_and(|t| t.kind == TokenKind::Newline)
    }

    /// Whether the next tokens start a section: a section name or `impl`/`extern` followed by `{`.
    pub(crate) fn starts_section(&self) -> bool {
        let token = self.raw(0);
        let is_name = match &token.kind {
            TokenKind::Impl | TokenKind::Extern => true,
            TokenKind::Ident => SectionKind::from_name(self.source(token.span)).is_some(),
            _ => false,
        };
        is_name && self.raw(1).kind == TokenKind::LBrace
    }

    /// After an entry: a line end or the closing `}`.
    fn end_of_entry(&mut self) {
        let errors = self.line_errors;
        if self.end_line_after_error(errors) {
            return;
        }
        match self.raw(0).kind {
            TokenKind::Newline | TokenKind::Eof | TokenKind::RBrace => {}
            TokenKind::Comma => {
                self.bump();
            }
            _ => {
                self.unexpected("a new line after the declaration");
                self.recover();
            }
        }
    }

    /// Reports declarations in the wrong section, duplicate inherent impls and test names.
    fn check_item(
        &mut self,
        section: SectionKind,
        item: &Item,
        inherent_impls: &mut HashMap<String, Span>,
        test_names: &mut HashMap<String, Span>,
    ) {
        let home = item.kind.home();
        let allowed =
            home == section || (section == SectionKind::Tests && home == SectionKind::Functions);
        if !allowed {
            let what = item.kind.describe();
            self.report(
                Diagnostic::new(
                    codes::E0205,
                    format!("{what} in the `{}` section", section.name()),
                )
                .primary(
                    head_span(item),
                    format!("a {what} belongs in `{}`", home.name()),
                )
                .help(format!(
                    "move this {what} into the `{}` section",
                    home.name()
                )),
            );
        }
        // `pub` is written on inherent methods only; tests and their helpers are private.
        let misplaced_pub: Vec<Span> = match &item.kind {
            ItemKind::Impl(block) if block.interface.is_some() => block
                .methods
                .iter()
                .filter(|m| m.decl.public)
                .map(|m| m.span)
                .collect(),
            ItemKind::Fn(decl) if section == SectionKind::Tests && decl.public => vec![item.span],
            _ => Vec::new(),
        };
        for span in misplaced_pub {
            let why = if section == SectionKind::Tests {
                "helpers in `tests` are private to the tests"
            } else {
                "methods of an interface implementation have the visibility of the interface"
            };
            self.report(
                Diagnostic::new(codes::E0301, "unexpected `pub`")
                    .primary(Span::new(span.file, span.start, span.start + 3), why)
                    .help("remove `pub`"),
            );
        }
        match &item.kind {
            ItemKind::Impl(block) if block.interface.is_none() => {
                let name = block.type_name.name.clone();
                if let Some(first) = inherent_impls.get(&name) {
                    self.report(
                        Diagnostic::new(
                            codes::E0210,
                            format!("second inherent `impl` block for `{name}`"),
                        )
                        .primary(block.type_name.span, "second block")
                        .secondary(*first, "first block")
                        .help("put all methods of the type into one block"),
                    );
                } else {
                    inherent_impls.insert(name, block.type_name.span);
                }
            }
            ItemKind::Test(test) => {
                let name = test.name.value.clone();
                if let Some(first) = test_names.get(&name) {
                    self.report(
                        Diagnostic::new(codes::E0211, format!("duplicate test name {name:?}"))
                            .primary(test.name.span, "used again here")
                            .secondary(*first, "first used here"),
                    );
                } else {
                    test_names.insert(name, test.name.span);
                }
            }
            _ => {}
        }
    }

    /// Something at the top level that is not a section.
    fn stray_top_level(&mut self, docs: &[Doc]) {
        let token = self.nth(0).clone();
        let text = self.source(token.span).to_string();
        // An unknown section name: `function {`.
        if token.kind == TokenKind::Ident && self.raw(1).kind == TokenKind::LBrace {
            let mut d = Diagnostic::new(codes::E0206, format!("unknown section `{text}`"))
                .primary(token.span, "not a section name");
            if let Some(close) = suggest_section(&text) {
                d = d.help(format!("did you mean `{close}`?"));
            } else {
                d = d.note(
                    "sections are imports, extern, config, types, interfaces, impl, main, functions, tests",
                );
            }
            self.report(d);
            self.bump();
            self.skip_braces();
            return;
        }
        let home = match &token.kind {
            TokenKind::Fn => Some(SectionKind::Functions),
            TokenKind::Struct | TokenKind::Enum | TokenKind::Type => Some(SectionKind::Types),
            TokenKind::Interface => Some(SectionKind::Interfaces),
            TokenKind::Pub => Some(SectionKind::Functions),
            TokenKind::Ident if text == "test" => Some(SectionKind::Tests),
            TokenKind::Ident if text == "intrinsic" => Some(SectionKind::Functions),
            _ => None,
        };
        if let Some(home) = home {
            let start = token.span;
            let before = self.diagnostics.len();
            let item = self.item(home, docs.to_vec());
            // Errors inside the stray declaration are kept; the declaration itself is dropped.
            let span = item.as_ref().map(head_span).unwrap_or(start);
            let real_home = item.as_ref().map(|i| i.kind.home()).unwrap_or(home);
            let what = item
                .as_ref()
                .map(|i| i.kind.describe())
                .unwrap_or("declaration");
            self.diagnostics.insert(
                before,
                Diagnostic::new(codes::E0207, format!("{what} outside of any section"))
                    .primary(span, "declarations must be inside a section")
                    .help(format!(
                        "put this {what} into the `{}` section",
                        real_home.name()
                    )),
            );
            return;
        }
        self.stray_docs(docs, "before something that is not a declaration");
        if token.kind != TokenKind::Error {
            let found = crate::describe(&token, &text);
            self.report(
                Diagnostic::new(codes::E0301, format!("expected a section, found {found}"))
                    .primary(token.span, "expected a section such as `functions {`"),
            );
        }
        self.bump();
        self.recover();
    }

    /// Skips a balanced `{ ... }` group (the body of an unknown section).
    fn skip_braces(&mut self) {
        if self.raw(0).kind != TokenKind::LBrace {
            return;
        }
        let mut depth = 0u32;
        loop {
            match self.raw(0).kind {
                TokenKind::Eof => return,
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        self.pos += 1;
                        return;
                    }
                }
                _ => {}
            }
            self.pos += 1;
        }
    }

    // ----- declarations ---------------------------------------------------------------------

    /// One entry of a section.
    pub(crate) fn item(&mut self, section: SectionKind, docs: Vec<Doc>) -> Option<Item> {
        let start = self.peek_span();
        let pub_span = self.peek_span();
        let public = self.eat(&TokenKind::Pub);
        let kind = match self.peek().clone() {
            TokenKind::Fn => ItemKind::Fn(self.fn_decl(public, false, FnContext::Free)?),
            TokenKind::Ident if self.at_word("intrinsic") && self.nth_is(1, &TokenKind::Fn) => {
                self.bump();
                ItemKind::Fn(self.fn_decl(public, true, FnContext::Free)?)
            }
            TokenKind::Ident if self.at_word("intrinsic") && self.nth_is(1, &TokenKind::Type) => {
                self.bump();
                self.bump();
                let name = self.ident("a type name")?;
                let generics = self.generics();
                ItemKind::IntrinsicType(IntrinsicTypeDecl {
                    public,
                    name,
                    generics,
                })
            }
            TokenKind::Struct => ItemKind::Struct(self.struct_decl(public)?),
            TokenKind::Enum => ItemKind::Enum(self.enum_decl(public)?),
            TokenKind::Type => ItemKind::Alias(self.alias_decl(public)?),
            TokenKind::Interface => ItemKind::Interface(self.interface_decl(public)?),
            TokenKind::Ident
                if self.at_word("test") && matches!(self.nth(1).kind, TokenKind::Str(_)) =>
            {
                ItemKind::Test(self.test_decl()?)
            }
            TokenKind::Ident => match section {
                SectionKind::Imports => ItemKind::Import(self.import(public)?),
                SectionKind::Config => ItemKind::Config(self.config_entry(public)?),
                SectionKind::Impl => ItemKind::Impl(self.impl_block()?),
                SectionKind::Extern => ItemKind::ExternTarget(self.extern_target()?),
                _ if self.nth_is(1, &TokenKind::Colon) => {
                    ItemKind::Config(self.config_entry(public)?)
                }
                _ => {
                    let expected = expected_in(section);
                    self.unexpected(expected);
                    self.recover();
                    return None;
                }
            },
            _ => {
                let expected = expected_in(section);
                self.unexpected(expected);
                self.recover();
                return None;
            }
        };
        if public
            && matches!(
                kind,
                ItemKind::Test(_) | ItemKind::Impl(_) | ItemKind::ExternTarget(_)
            )
        {
            let why = match kind {
                ItemKind::Test(_) => "tests cannot be `pub`",
                ItemKind::Impl(_) => "write `pub` on the methods, not on the block",
                _ => "write `pub` on the declarations inside the block",
            };
            self.report(
                Diagnostic::new(codes::E0301, "unexpected `pub`")
                    .primary(pub_span, why)
                    .help("remove `pub`"),
            );
        }
        Some(Item {
            docs,
            kind,
            span: self.span_from(start),
        })
    }

    fn import(&mut self, public: bool) -> Option<Import> {
        let start = self.peek_span();
        let mut segments = vec![self.ident("a module path")?];
        let mut names = None;
        while self.at(&TokenKind::Dot) {
            self.bump();
            if self.at(&TokenKind::LBrace) {
                names = Some(self.import_group()?);
                break;
            }
            segments.push(self.ident("a name or `{`")?);
        }
        let path = Path {
            segments,
            span: self.span_from(start),
        };
        let mut alias = None;
        if names.is_none() && self.eat_word("as") {
            alias = Some(self.ident("an alias after `as`")?);
        }
        Some(Import {
            public,
            path,
            alias,
            names,
        })
    }

    fn import_group(&mut self) -> Option<Vec<ImportName>> {
        self.bump(); // `{`
        self.ignore_lines(|p| {
            let mut names = Vec::new();
            while !p.at(&TokenKind::RBrace) && !p.at(&TokenKind::Eof) {
                let name = p.ident("an imported name")?;
                let alias = if p.eat_word("as") {
                    Some(p.ident("an alias after `as`")?)
                } else {
                    None
                };
                names.push(ImportName { name, alias });
                if !p.eat(&TokenKind::Comma) {
                    break;
                }
            }
            p.expect(&TokenKind::RBrace, "`}`")?;
            Some(names)
        })
    }

    fn extern_target(&mut self) -> Option<ExternTarget> {
        let target = self.ident("a target name")?;
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        let mut decls = Vec::new();
        self.significant_lines(|p| {
            loop {
                p.skip_newlines();
                let docs = p.docs();
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.at(&TokenKind::Eof) || p.resync_to_top {
                    p.stray_docs(&docs, "at the end of a block");
                    break;
                }
                let before = p.pos;
                p.line_errors = p.start_line();
                if let Some(decl) = p.extern_decl(docs) {
                    decls.push(decl);
                }
                p.end_of_entry();
                if p.pos == before {
                    p.bump();
                }
            }
        });
        if self.expect(&TokenKind::RBrace, "`}`").is_none() {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `{`")
                    .primary(open, "this `{` is never closed"),
            );
        }
        Some(ExternTarget { target, decls })
    }

    fn extern_decl(&mut self, docs: Vec<Doc>) -> Option<ExternDecl> {
        let start = self.peek_span();
        let public = self.eat(&TokenKind::Pub);
        let kind = if self.at(&TokenKind::Fn) {
            let decl = self.fn_decl(public, false, FnContext::Signature)?;
            ExternDeclKind::Fn(decl.sig)
        } else if self.eat(&TokenKind::Type) {
            ExternDeclKind::Type(self.ident("a type name")?)
        } else if !public && self.eat(&TokenKind::Struct) {
            ExternDeclKind::Struct(self.ident("the name of a struct")?)
        } else {
            self.unexpected(if public {
                "`fn` or `type`"
            } else {
                "`fn`, `type` or `struct`"
            });
            self.recover();
            return None;
        };
        if !self.eat_word("from") {
            self.unexpected("`from` and the target path");
            self.recover();
            return None;
        }
        let from = self.string_literal("the target path as a string")?;
        Some(ExternDecl {
            docs,
            public,
            kind,
            from,
            span: self.span_from(start),
            native: None,
            native_fields: Vec::new(),
        })
    }

    pub(crate) fn string_literal(&mut self, what: &str) -> Option<StrLit> {
        let token = self.nth(0).clone();
        if let TokenKind::Str(value) = token.kind {
            self.bump();
            Some(StrLit {
                value,
                span: token.span,
            })
        } else {
            self.unexpected(what);
            None
        }
    }

    fn config_entry(&mut self, public: bool) -> Option<ConfigEntry> {
        let name = self.ident("a constant name")?;
        self.expect_then_lines(&TokenKind::Colon, "`:` and the type of the constant")?;
        let ty = self.ty();
        self.expect(&TokenKind::Eq, "`=` and the value")?;
        self.skip_newlines();
        let value = self.expr();
        self.check_no_assignment();
        Some(ConfigEntry {
            public,
            name,
            ty,
            value,
        })
    }

    /// `<T: A + B, U>` after a declaration name.
    /// `where T: A + B, U: C` after a signature (spec chapter 4, section 8.6); `where` is a
    /// contextual word, special only here.
    fn where_bounds(&mut self) -> Vec<GenericParam> {
        if !self.at_word("where") {
            return Vec::new();
        }
        self.bump();
        let mut params = Vec::new();
        while self.at(&TokenKind::Ident) {
            let Some(name) = self.ident("a type parameter") else {
                break;
            };
            let mut bounds = Vec::new();
            if self.expect(&TokenKind::Colon, "`:`").is_some() {
                bounds.push(self.ty());
                while self.eat(&TokenKind::Plus) {
                    bounds.push(self.ty());
                }
            }
            params.push(GenericParam { name, bounds });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        if params.is_empty() {
            self.unexpected("a type parameter and its bounds after `where`");
        }
        params
    }

    pub(crate) fn generics(&mut self) -> Vec<GenericParam> {
        if !self.at(&TokenKind::Lt) {
            return Vec::new();
        }
        self.bump();
        self.ignore_lines(|p| {
            let mut params = Vec::new();
            while p.at(&TokenKind::Ident) {
                let Some(name) = p.ident("a type parameter") else {
                    break;
                };
                let mut bounds = Vec::new();
                if p.eat_then_lines(&TokenKind::Colon) {
                    bounds.push(p.ty());
                    while p.eat(&TokenKind::Plus) {
                        bounds.push(p.ty());
                    }
                }
                params.push(GenericParam { name, bounds });
                if !p.eat(&TokenKind::Comma) {
                    break;
                }
            }
            if !p.eat_gt() {
                p.unexpected("`>`");
            }
            params
        })
    }

    fn struct_decl(&mut self, public: bool) -> Option<StructDecl> {
        self.bump(); // `struct`
        let name = self.ident("a struct name")?;
        let generics = self.generics();
        let fields = self.field_block(true)?;
        Some(StructDecl {
            public,
            name,
            generics,
            fields,
        })
    }

    /// `{ fields }` of a struct (or of a variant with named fields).
    fn field_block(&mut self, allow_pub: bool) -> Option<Vec<Field>> {
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        let fields = self.significant_lines(|p| {
            let mut fields = Vec::new();
            loop {
                p.skip_newlines();
                let docs = p.docs();
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.at(&TokenKind::Eof) || p.resync_to_top {
                    p.stray_docs(&docs, "at the end of a block");
                    break;
                }
                let start = p.peek_span();
                let public = allow_pub && p.eat(&TokenKind::Pub);
                let before = p.pos;
                p.line_errors = p.start_line();
                let field = p.ident("a field name").and_then(|name| {
                    p.expect_then_lines(&TokenKind::Colon, "`:` and the field type")?;
                    let ty = p.ty();
                    Some((name, ty))
                });
                match field {
                    Some((name, ty)) => fields.push(Field {
                        docs,
                        public,
                        name,
                        ty,
                        span: p.span_from(start),
                    }),
                    None => p.recover(),
                }
                if !p.eat(&TokenKind::Comma) {
                    p.end_of_entry();
                }
                if p.pos == before {
                    p.bump();
                }
            }
            fields
        });
        if self.expect(&TokenKind::RBrace, "`}`").is_none() {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `{`")
                    .primary(open, "this `{` is never closed"),
            );
        }
        Some(fields)
    }

    fn enum_decl(&mut self, public: bool) -> Option<EnumDecl> {
        self.bump(); // `enum`
        let name = self.ident("an enum name")?;
        let generics = self.generics();
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        let variants = self.significant_lines(|p| {
            let mut variants = Vec::new();
            loop {
                p.skip_newlines();
                let docs = p.docs();
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.at(&TokenKind::Eof) || p.resync_to_top {
                    p.stray_docs(&docs, "at the end of a block");
                    break;
                }
                let start = p.peek_span();
                let before = p.pos;
                p.line_errors = p.start_line();
                if let Some(name) = p.ident("a variant name") {
                    let payload = if p.at(&TokenKind::LParen) {
                        p.bump();
                        let types = p.ignore_lines(|p| {
                            let mut types = Vec::new();
                            while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                                types.push(p.ty());
                                if !p.eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                            p.expect(&TokenKind::RParen, "`)`");
                            types
                        });
                        VariantPayload::Tuple(types)
                    } else if p.at(&TokenKind::LBrace) {
                        VariantPayload::Struct(p.field_block(false).unwrap_or_default())
                    } else {
                        VariantPayload::None
                    };
                    variants.push(Variant {
                        docs,
                        name,
                        payload,
                        span: p.span_from(start),
                    });
                } else {
                    p.recover();
                }
                if !p.eat(&TokenKind::Comma) {
                    p.end_of_entry();
                }
                if p.pos == before {
                    p.bump();
                }
            }
            variants
        });
        let close = self.peek_span();
        if self.expect(&TokenKind::RBrace, "`}`").is_none() {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `{`")
                    .primary(open, "this `{` is never closed"),
            );
        } else if variants.is_empty() {
            self.report(
                Diagnostic::new(codes::E0301, "expected a variant, found `}`")
                    .primary(close, "an enum needs at least one variant"),
            );
        }
        Some(EnumDecl {
            public,
            name,
            generics,
            variants,
        })
    }

    fn alias_decl(&mut self, public: bool) -> Option<AliasDecl> {
        self.bump(); // `type`
        let name = self.ident("an alias name")?;
        let generics = self.generics();
        self.expect_then_lines(&TokenKind::Eq, "`=` and the aliased type")?;
        let ty = self.ty();
        Some(AliasDecl {
            public,
            name,
            generics,
            ty,
        })
    }

    fn interface_decl(&mut self, public: bool) -> Option<InterfaceDecl> {
        self.bump(); // `interface`
        let name = self.ident("an interface name")?;
        let generics = self.generics();
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        let methods = self.significant_lines(|p| {
            let mut methods = Vec::new();
            loop {
                p.skip_newlines();
                let docs = p.docs();
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.at(&TokenKind::Eof) || p.resync_to_top {
                    p.stray_docs(&docs, "at the end of a block");
                    break;
                }
                let before = p.pos;
                p.line_errors = p.start_line();
                if p.at(&TokenKind::Fn) {
                    if let Some(decl) = p.fn_decl(false, false, FnContext::Signature) {
                        methods.push(MethodSig {
                            docs,
                            sig: decl.sig,
                        });
                    }
                } else {
                    p.unexpected("`fn`");
                    p.recover();
                }
                p.end_of_entry();
                if p.pos == before {
                    p.bump();
                }
            }
            methods
        });
        if self.expect(&TokenKind::RBrace, "`}`").is_none() {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `{`")
                    .primary(open, "this `{` is never closed"),
            );
        }
        Some(InterfaceDecl {
            public,
            name,
            generics,
            methods,
        })
    }

    fn impl_block(&mut self) -> Option<ImplBlock> {
        let type_name = self.ident("a type name")?;
        let generics = self.generics();
        let interface = if self.eat_then_lines(&TokenKind::Colon) {
            Some(self.ty())
        } else {
            None
        };
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        let methods = self.significant_lines(|p| {
            let mut methods = Vec::new();
            loop {
                p.skip_newlines();
                let docs = p.docs();
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.at(&TokenKind::Eof) || p.resync_to_top {
                    p.stray_docs(&docs, "at the end of a block");
                    break;
                }
                let start = p.peek_span();
                let before = p.pos;
                p.line_errors = p.start_line();
                let public = p.eat(&TokenKind::Pub);
                let intrinsic = p.at_word("intrinsic") && p.nth_is(1, &TokenKind::Fn);
                if intrinsic {
                    p.bump();
                }
                if p.at(&TokenKind::Fn) {
                    if let Some(decl) = p.fn_decl(public, intrinsic, FnContext::Method) {
                        methods.push(Method {
                            docs,
                            decl,
                            span: p.span_from(start),
                        });
                    }
                } else {
                    p.unexpected("`fn`: an impl block contains only methods");
                    p.recover();
                }
                p.end_of_entry();
                if p.pos == before {
                    p.bump();
                }
            }
            methods
        });
        if self.expect(&TokenKind::RBrace, "`}`").is_none() {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `{`")
                    .primary(open, "this `{` is never closed"),
            );
        }
        Some(ImplBlock {
            type_name,
            generics,
            interface,
            methods,
        })
    }

    fn test_decl(&mut self) -> Option<TestDecl> {
        self.bump(); // `test`
        let name = self.string_literal("the test name")?;
        let body = self.block()?;
        Some(TestDecl { name, body })
    }

    pub(crate) fn fn_decl(
        &mut self,
        public: bool,
        intrinsic: bool,
        context: FnContext,
    ) -> Option<FnDecl> {
        let start = self.bump().span; // `fn`
        let name = self.ident("a function name")?;
        let generics = self.generics();
        let params_start = self.peek_span();
        self.expect(&TokenKind::LParen, "`(`")?;
        let params = self.ignore_lines(|p| {
            let mut params: Vec<Param> = Vec::new();
            while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                let before = p.pos;
                if let Some(param) = p.param(params.len(), context) {
                    params.push(param);
                } else {
                    p.recover();
                }
                if !p.eat(&TokenKind::Comma) || p.pos == before {
                    break;
                }
            }
            params
        });
        if self
            .ignore_lines(|p| p.expect(&TokenKind::RParen, "`)`"))
            .is_none()
        {
            self.report(
                Diagnostic::new(codes::E0302, "unclosed `(`")
                    .primary(params_start, "this `(` is never closed"),
            );
            self.recover();
        }
        let ret = if self.eat_then_lines(&TokenKind::Arrow) {
            Some(self.ty())
        } else {
            None
        };
        let span = self.span_from(start);
        let where_bounds = self.where_bounds();
        let sig = FnSig {
            name,
            generics,
            params,
            ret,
            where_bounds,
            span,
        };
        let body = if context == FnContext::Signature || intrinsic {
            if self.at(&TokenKind::LBrace) {
                let span = self.peek_span();
                let why = if intrinsic {
                    "an `intrinsic` function has no body"
                } else {
                    "a signature has no body"
                };
                self.report(
                    Diagnostic::new(codes::E0301, "unexpected function body").primary(span, why),
                );
                let _ = self.block();
            }
            None
        } else if self.at(&TokenKind::LBrace) {
            self.block()
        } else {
            self.unexpected("`{` and the function body");
            None
        };
        Some(FnDecl {
            public,
            intrinsic,
            sig,
            body,
        })
    }

    fn param(&mut self, index: usize, context: FnContext) -> Option<Param> {
        let start = self.peek_span();
        let mode = self.mode();
        if self.at(&TokenKind::SelfValue) {
            let span = self.bump().span;
            if context == FnContext::Free || index > 0 {
                let why = if index > 0 {
                    "`self` must be the first parameter"
                } else {
                    "only methods in `impl` and `interface` have `self`"
                };
                self.report(
                    Diagnostic::new(codes::E0309, "unexpected `self` parameter").primary(span, why),
                );
            }
            return Some(Param {
                mode,
                kind: ParamKind::SelfParam,
                span: self.span_from(start),
            });
        }
        let name = self.ident("a parameter name")?;
        self.expect_then_lines(&TokenKind::Colon, "`:` and the parameter type")?;
        let ty = self.ty();
        Some(Param {
            mode,
            kind: ParamKind::Named { name, ty },
            span: self.span_from(start),
        })
    }

    pub(crate) fn mode(&mut self) -> Mode {
        if self.eat(&TokenKind::Mut) {
            Mode::Mut
        } else if self.eat(&TokenKind::Own) {
            Mode::Own
        } else {
            Mode::Read
        }
    }
}

/// From the start of a declaration to the end of its name: `pub struct Point`.
pub(crate) fn head_span(item: &Item) -> Span {
    let name = match &item.kind {
        ItemKind::Import(import) => import.path.span,
        ItemKind::ExternTarget(target) => target.target.span,
        ItemKind::Config(entry) => entry.name.span,
        ItemKind::Struct(decl) => decl.name.span,
        ItemKind::Enum(decl) => decl.name.span,
        ItemKind::Alias(decl) => decl.name.span,
        ItemKind::IntrinsicType(decl) => decl.name.span,
        ItemKind::Interface(decl) => decl.name.span,
        ItemKind::Impl(block) => block.type_name.span,
        ItemKind::Fn(decl) => decl.sig.name.span,
        ItemKind::Test(test) => test.name.span,
    };
    Span::new(item.span.file, item.span.start, name.end)
}

fn expected_in(section: SectionKind) -> &'static str {
    match section {
        SectionKind::Imports => "an import",
        SectionKind::Extern => "a target block (`ts`, `go` or `rust`)",
        SectionKind::Config => "a constant",
        SectionKind::Types => "`struct`, `enum` or `type`",
        SectionKind::Interfaces => "`interface`",
        SectionKind::Impl => "an impl block",
        SectionKind::Main => "the body of `main`",
        SectionKind::Functions => "`fn`",
        SectionKind::Tests => "`test` or `fn`",
    }
}

/// A section name close to `text`, for "did you mean" hints.
fn suggest_section(text: &str) -> Option<&'static str> {
    SectionKind::ALL
        .iter()
        .map(|kind| (distance(text, kind.name()), kind.name()))
        .filter(|(d, name)| *d <= 2.max(name.len() / 4))
        .min()
        .map(|(_, name)| name)
}

/// Levenshtein distance on characters.
pub(crate) fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            let above = row.get(j + 1).copied().unwrap_or(0);
            let left = row.get(j).copied().unwrap_or(0);
            let value = (above + 1).min(left + 1).min(prev + cost);
            prev = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = value;
            }
        }
    }
    row.last().copied().unwrap_or(0)
}
