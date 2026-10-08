//! Types and patterns (spec chapter 3, sections 3 and 7).

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_lexer::{Token, TokenKind};
use wolf_span::Span;

use crate::Parser;

impl Parser<'_> {
    /// A written type.
    pub(crate) fn ty(&mut self) -> Ty {
        let start = self.peek_span();
        if !self.enter() {
            self.leave();
            self.give_up();
            return self.ty_node(TyKind::Error, start);
        }
        let kind = match self.peek().clone() {
            TokenKind::SelfType => {
                self.bump();
                TyKind::SelfType
            }
            TokenKind::Fn => self.fn_type(),
            TokenKind::Ident => {
                let path = self.path();
                let mut args = Vec::new();
                if self.at(&TokenKind::Lt) {
                    self.bump();
                    args = self.ignore_lines(|p| {
                        let mut args = Vec::new();
                        while !p.at_type_close() && !p.at(&TokenKind::Eof) {
                            let before = p.pos;
                            args.push(p.ty());
                            if !p.eat(&TokenKind::Comma) || p.pos == before {
                                break;
                            }
                        }
                        if !p.eat_gt() {
                            p.unexpected("`>` to close the type arguments");
                        }
                        args
                    });
                }
                TyKind::Path { path, args }
            }
            _ => {
                self.unexpected("a type");
                TyKind::Error
            }
        };
        self.leave();
        let span = self.span_from(start);
        self.ty_node(kind, span)
    }

    fn ty_node(&mut self, kind: TyKind, span: Span) -> Ty {
        Ty {
            id: self.id(),
            kind,
            span,
        }
    }

    fn fn_type(&mut self) -> TyKind {
        self.bump(); // `fn`
        if self.expect(&TokenKind::LParen, "`(`").is_none() {
            return TyKind::Error;
        }
        let params = self.ignore_lines(|p| {
            let mut params = Vec::new();
            while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                let before = p.pos;
                let mode = p.mode();
                params.push((mode, p.ty()));
                if !p.eat(&TokenKind::Comma) || p.pos == before {
                    break;
                }
            }
            p.expect(&TokenKind::RParen, "`)`");
            params
        });
        let ret = if self.eat_then_lines(&TokenKind::Arrow) {
            Some(Box::new(self.ty()))
        } else {
            None
        };
        TyKind::Fn { params, ret }
    }

    fn at_type_close(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Gt | TokenKind::Shr | TokenKind::Ge | TokenKind::ShrEq
        )
    }

    /// Consumes one `>`, splitting `>>`, `>=` and `>>=` (spec chapter 1, section 6, rule 3).
    pub(crate) fn eat_gt(&mut self) -> bool {
        let index = self.visible_index(0);
        let Some(token) = self.tokens.get(index).cloned() else {
            return false;
        };
        let rest = match token.kind {
            TokenKind::Gt => {
                self.bump();
                return true;
            }
            TokenKind::Shr => TokenKind::Gt,
            TokenKind::Ge => TokenKind::Eq,
            TokenKind::ShrEq => TokenKind::Ge,
            _ => return false,
        };
        let span = Span::new(token.span.file, token.span.start + 1, token.span.end);
        if let Some(slot) = self.tokens.get_mut(index) {
            *slot = Token { kind: rest, span };
        }
        self.pos = index;
        true
    }

    // ----- patterns -------------------------------------------------------------------------

    /// A pattern with alternatives: `A | B`.
    pub(crate) fn pattern(&mut self) -> Pat {
        let start = self.peek_span();
        if !self.enter() {
            self.leave();
            self.give_up();
            return self.pat_node(PatKind::Error, start);
        }
        let first = self.single_pattern();
        let pat = if self.at(&TokenKind::Pipe) {
            let mut alternatives = vec![first];
            while self.eat(&TokenKind::Pipe) {
                alternatives.push(self.single_pattern());
            }
            let span = self.span_from(start);
            self.pat_node(PatKind::Or(alternatives), span)
        } else {
            first
        };
        self.leave();
        pat
    }

    fn pat_node(&mut self, kind: PatKind, span: Span) -> Pat {
        Pat {
            id: self.id(),
            kind,
            span,
        }
    }

    fn single_pattern(&mut self) -> Pat {
        let token = self.nth(0).clone();
        let start = token.span;
        let kind = match token.kind {
            TokenKind::Underscore => {
                self.bump();
                PatKind::Wild
            }
            TokenKind::Int(value) => {
                self.bump();
                PatKind::Int {
                    negative: false,
                    value,
                }
            }
            TokenKind::Minus if matches!(self.nth(1).kind, TokenKind::Int(_)) => {
                self.bump();
                match self.bump().kind {
                    TokenKind::Int(value) => PatKind::Int {
                        negative: true,
                        value,
                    },
                    _ => PatKind::Error,
                }
            }
            TokenKind::Float(_) => {
                self.bump();
                self.report(
                    Diagnostic::new(codes::E0312, "float literal in a pattern")
                        .primary(start, "floats cannot be matched exactly")
                        .help("compare with `<` and `>` in an `if` instead"),
                );
                PatKind::Error
            }
            TokenKind::Minus if matches!(self.nth(1).kind, TokenKind::Float(_)) => {
                self.bump();
                let span = self.bump().span;
                self.report(
                    Diagnostic::new(codes::E0312, "float literal in a pattern")
                        .primary(start.to(span), "floats cannot be matched exactly")
                        .help("compare with `<` and `>` in an `if` instead"),
                );
                PatKind::Error
            }
            TokenKind::Str(value) => {
                self.bump();
                PatKind::Str(value)
            }
            TokenKind::True => {
                self.bump();
                PatKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                PatKind::Bool(false)
            }
            TokenKind::Ident => {
                let path = self.path();
                if self.at(&TokenKind::LParen) {
                    self.bump();
                    let items = self.ignore_lines(|p| {
                        let mut items = Vec::new();
                        while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                            let before = p.pos;
                            items.push(p.pattern());
                            if !p.eat(&TokenKind::Comma) || p.pos == before {
                                break;
                            }
                        }
                        p.expect(&TokenKind::RParen, "`)`");
                        items
                    });
                    PatKind::Tuple { path, items }
                } else if self.at(&TokenKind::LBrace) {
                    self.bump();
                    let (fields, rest) = self.ignore_lines(|p| {
                        let mut fields = Vec::new();
                        let mut rest = false;
                        loop {
                            if p.eat(&TokenKind::DotDot) {
                                rest = true;
                                break;
                            }
                            if !p.at(&TokenKind::Ident) {
                                break;
                            }
                            let Some(name) = p.ident("a field name") else {
                                break;
                            };
                            let pat = if p.eat(&TokenKind::Colon) {
                                Some(p.pattern())
                            } else {
                                None
                            };
                            fields.push(FieldPat { name, pat });
                            if !p.eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                        p.expect(&TokenKind::RBrace, "`}`");
                        (fields, rest)
                    });
                    PatKind::Struct { path, fields, rest }
                } else if path.segments.len() == 1 {
                    match path.segments.into_iter().next() {
                        Some(name) => PatKind::Name(name),
                        None => PatKind::Error,
                    }
                } else {
                    PatKind::Path(path)
                }
            }
            TokenKind::Error => {
                self.bump();
                PatKind::Error
            }
            _ => {
                self.unexpected("a pattern");
                PatKind::Error
            }
        };
        let span = self.span_from(start);
        self.pat_node(kind, span)
    }
}
