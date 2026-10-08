//! Blocks, statements and expressions (spec chapter 3, sections 4 to 6).

use wolf_ast::*;
use wolf_diag::{Diagnostic, codes};
use wolf_lexer::TokenKind;
use wolf_span::Span;

use crate::Parser;

/// Restrictions that depend on where an expression is.
#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    /// In conditions, the `match` scrutinee and the `for` collection a struct literal needs
    /// parentheses (spec chapter 3, section 5, rule 7).
    no_struct: bool,
}

const NORMAL: Ctx = Ctx { no_struct: false };
const NO_STRUCT: Ctx = Ctx { no_struct: true };

/// Binding power and operator of a binary token (spec chapter 3, section 6).
fn binary_op(kind: &TokenKind) -> Option<(BinaryOp, u8)> {
    use TokenKind::*;
    Some(match kind {
        OrOr => (BinaryOp::Or, 10),
        AndAnd => (BinaryOp::And, 20),
        EqEq => (BinaryOp::Eq, 30),
        NotEq => (BinaryOp::NotEq, 30),
        Lt => (BinaryOp::Lt, 30),
        Le => (BinaryOp::Le, 30),
        Gt => (BinaryOp::Gt, 30),
        Ge => (BinaryOp::Ge, 30),
        Pipe => (BinaryOp::BitOr, 40),
        Caret => (BinaryOp::BitXor, 45),
        Amp => (BinaryOp::BitAnd, 50),
        Shl => (BinaryOp::Shl, 55),
        Shr => (BinaryOp::Shr, 55),
        Plus => (BinaryOp::Add, 60),
        Minus => (BinaryOp::Sub, 60),
        Star => (BinaryOp::Mul, 70),
        Slash => (BinaryOp::Div, 70),
        Percent => (BinaryOp::Rem, 70),
        _ => return None,
    })
}

fn assign_op(kind: &TokenKind) -> Option<AssignOp> {
    use TokenKind::*;
    Some(match kind {
        Eq => AssignOp::Assign,
        PlusEq => AssignOp::Add,
        MinusEq => AssignOp::Sub,
        StarEq => AssignOp::Mul,
        SlashEq => AssignOp::Div,
        PercentEq => AssignOp::Rem,
        AmpEq => AssignOp::BitAnd,
        PipeEq => AssignOp::BitOr,
        CaretEq => AssignOp::BitXor,
        ShlEq => AssignOp::Shl,
        ShrEq => AssignOp::Shr,
        _ => return None,
    })
}

/// Tokens that are binary operators and cannot start an expression (for E0303).
fn only_binary(kind: &TokenKind) -> bool {
    use TokenKind::*;
    matches!(
        kind,
        Plus | Star
            | Slash
            | Percent
            | EqEq
            | NotEq
            | Lt
            | Le
            | Gt
            | Ge
            | AndAnd
            | Amp
            | Caret
            | Shl
            | Shr
    )
}

impl Parser<'_> {
    fn expr_node(&mut self, kind: ExprKind, span: Span) -> Expr {
        Expr {
            id: self.id(),
            kind,
            span,
        }
    }

    fn error_expr(&mut self, span: Span) -> Expr {
        self.expr_node(ExprKind::Error, span)
    }

    // ----- blocks and statements ------------------------------------------------------------

    /// `{ statements }`.
    pub(crate) fn block(&mut self) -> Option<Block> {
        let open = self.expect(&TokenKind::LBrace, "`{`")?;
        if !self.enter() {
            self.leave();
            self.give_up();
            return Some(Block {
                stmts: Vec::new(),
                span: open,
            });
        }
        let stmts = self.significant_lines(|p| {
            let mut stmts = Vec::new();
            loop {
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.resync_to_top {
                    break;
                }
                if p.at(&TokenKind::Eof) {
                    p.report(
                        Diagnostic::new(codes::E0302, "unclosed `{`")
                            .primary(open, "this `{` is never closed"),
                    );
                    break;
                }
                if p.starts_section() {
                    p.report(
                        Diagnostic::new(codes::E0302, "unclosed `{`")
                            .primary(open, "this `{` is never closed")
                            .secondary(p.peek_span(), "a section starts here"),
                    );
                    p.resync_to_top = true;
                    break;
                }
                let before = p.pos;
                let errors = p.start_line();
                if let Some(stmt) = p.stmt() {
                    stmts.push(stmt);
                }
                p.end_of_stmt(errors);
                if p.pos == before {
                    p.bump();
                }
            }
            stmts
        });
        self.leave();
        if !self.resync_to_top {
            self.eat(&TokenKind::RBrace);
        }
        Some(Block {
            stmts,
            span: self.span_from(open),
        })
    }

    /// After a statement: a line end or the closing `}`.
    fn end_of_stmt(&mut self, errors_before: usize) {
        if self.end_line_after_error(errors_before) {
            return;
        }
        match self.raw(0).kind {
            TokenKind::Newline | TokenKind::Eof | TokenKind::RBrace => {}
            TokenKind::Error => self.recover(),
            _ if self.too_deep() => self.give_up(),
            _ => {
                let span = self.raw(0).span;
                self.report(
                    Diagnostic::new(codes::E0313, "two statements on one line")
                        .primary(span, "a new statement starts here")
                        .help("there is no `;`: put each statement on its own line"),
                );
                self.recover();
            }
        }
    }

    fn stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        let token = self.nth(0).clone();
        match &token.kind {
            TokenKind::Let | TokenKind::Var => return self.let_stmt(),
            TokenKind::While => {
                self.bump();
                let cond = self.expr_with(NO_STRUCT);
                self.check_no_assignment();
                let body = self.block()?;
                return Some(Stmt {
                    kind: StmtKind::While { cond, body },
                    span: self.span_from(start),
                });
            }
            TokenKind::For => return self.for_stmt(),
            TokenKind::DocOuter(_) | TokenKind::DocInner(_) => {
                self.report(
                    Diagnostic::new(codes::E0212, "doc comment inside a function body")
                        .primary(token.span, "doc comments document declarations")
                        .help("use `//` for a comment here"),
                );
                self.bump();
                return None;
            }
            TokenKind::Else => {
                self.report(
                    Diagnostic::new(codes::E0304, "`else` on a new line")
                        .primary(token.span, "`else` must follow `}` on the same line")
                        .help("write `} else {`"),
                );
                self.bump();
                if self.at(&TokenKind::If) {
                    let _ = self.expr();
                } else if self.at(&TokenKind::LBrace) {
                    let _ = self.block();
                }
                return None;
            }
            kind if only_binary(kind) => {
                self.report(
                    Diagnostic::new(
                        codes::E0303,
                        format!("binary operator {kind} at the start of a line"),
                    )
                    .primary(token.span, "this line cannot start with an operator")
                    .help("move the operator to the end of the previous line"),
                );
                self.recover();
                return None;
            }
            _ => {}
        }
        let expr = self.expr();
        if let Some(op) = assign_op(self.peek()) {
            let op_span = self.bump().span;
            if !is_place(&expr) {
                self.report(
                    Diagnostic::new(codes::E0308, "invalid left side of an assignment")
                        .primary(expr.span, "this is not a place that can be assigned")
                        .secondary(op_span, "assignment here")
                        .note("a place is a variable, `self`, a field or an index, like `user.name` or `items[i]`"),
                );
            }
            self.skip_newlines();
            let value = self.expr();
            self.check_no_assignment();
            return Some(Stmt {
                kind: StmtKind::Assign {
                    target: expr,
                    op,
                    value,
                },
                span: self.span_from(start),
            });
        }
        Some(Stmt {
            kind: StmtKind::Expr(expr),
            span: self.span_from(start),
        })
    }

    fn let_stmt(&mut self) -> Option<Stmt> {
        let start = self.peek_span();
        let mutable = self.bump().kind == TokenKind::Var;
        let name = if self.at(&TokenKind::Underscore) {
            let span = self.bump().span;
            if mutable {
                self.report(
                    Diagnostic::new(codes::E0301, "expected a variable name, found `_`")
                        .primary(span, "only `let _ = ...` discards a value"),
                );
            }
            None
        } else {
            Some(self.ident("a variable name")?)
        };
        let ty = if self.eat_then_lines(&TokenKind::Colon) {
            Some(self.ty())
        } else {
            None
        };
        if self
            .expect(&TokenKind::Eq, "`=` and the initial value")
            .is_none()
        {
            self.recover();
            return None;
        }
        self.skip_newlines();
        let value = self.expr();
        self.check_no_assignment();
        Some(Stmt {
            kind: StmtKind::Let {
                mutable,
                name,
                ty,
                value,
            },
            span: self.span_from(start),
        })
    }

    fn for_stmt(&mut self) -> Option<Stmt> {
        let start = self.bump().span; // `for`
        let var = self.ident("a loop variable")?;
        if self.expect(&TokenKind::In, "`in`").is_none() {
            self.recover();
            return None;
        }
        let mode_span = self.peek_span();
        let mode = self.mode();
        let first = self.expr_with(NO_STRUCT);
        let iter = if self.at(&TokenKind::DotDot) || self.at(&TokenKind::DotDotEq) {
            let inclusive = self.bump().kind == TokenKind::DotDotEq;
            let end = self.expr_with(NO_STRUCT);
            if mode != Mode::Read {
                self.report(
                    Diagnostic::new(codes::E0301, "a range cannot be marked `mut` or `own`")
                        .primary(mode_span, "remove this marker"),
                );
            }
            ForIter::Range {
                start: first,
                end,
                inclusive,
            }
        } else {
            ForIter::Collection { mode, expr: first }
        };
        let body = self.block()?;
        Some(Stmt {
            kind: StmtKind::For { var, iter, body },
            span: self.span_from(start),
        })
    }

    /// Reports an assignment operator where an expression ended (E0308).
    pub(crate) fn check_no_assignment(&mut self) {
        if assign_op(self.peek()).is_some() {
            let span = self.peek_span();
            self.report(
                Diagnostic::new(codes::E0308, "assignment used as an expression")
                    .primary(span, "assignment is a statement, not an expression")
                    .help("write the assignment on its own line"),
            );
            self.recover();
        }
    }

    /// Reports a range where ranges are not allowed (E0307) and skips it.
    fn check_no_range(&mut self) {
        if self.at(&TokenKind::DotDot) || self.at(&TokenKind::DotDotEq) {
            let span = self.peek_span();
            self.report(
                Diagnostic::new(codes::E0307, "range outside `for` or index brackets").primary(
                    span,
                    "ranges are allowed only in `for i in a..b` and `items[a..b]`",
                ),
            );
            self.bump();
            let _ = self.expr();
        }
    }

    // ----- expressions ----------------------------------------------------------------------

    /// An expression in a normal position.
    pub(crate) fn expr(&mut self) -> Expr {
        self.expr_with(NORMAL)
    }

    fn expr_with(&mut self, ctx: Ctx) -> Expr {
        let span = self.peek_span();
        if !self.enter() {
            self.leave();
            self.give_up();
            return self.error_expr(span);
        }
        let expr = if self.at(&TokenKind::Pipe) || self.at(&TokenKind::OrOr) {
            self.closure(ctx)
        } else {
            self.binary(0, ctx)
        };
        self.leave();
        expr
    }

    fn binary(&mut self, min_bp: u8, ctx: Ctx) -> Expr {
        let start = self.peek_span();
        let mut lhs = self.unary(ctx);
        let mut chain = 0u32;
        while let Some((op, bp)) = binary_op(self.peek()) {
            if bp < min_bp {
                break;
            }
            let op_span = self.bump().span;
            if op == BinaryOp::Gt && looks_like_type_args(&lhs) && self.at(&TokenKind::LParen) {
                self.report(
                    Diagnostic::new(codes::E0311, "type arguments need a dot in expressions")
                        .primary(lhs.span.to(op_span), "parsed as comparisons `<` and `>`")
                        .help("write explicit type arguments as `name.<Type>(...)`"),
                );
                self.recover();
                return self.error_expr(start);
            }
            self.skip_newlines();
            // Left associative: the right side binds tighter.
            let rhs = if self.at(&TokenKind::Pipe) || self.at(&TokenKind::OrOr) {
                self.closure(ctx)
            } else {
                self.binary(bp + 1, ctx)
            };
            if op.is_comparison() && is_comparison(&lhs) {
                self.report(
                    Diagnostic::new(codes::E0305, "comparison operators cannot be chained")
                        .primary(op_span, "second comparison")
                        .help("use `&&`: `a < b && b < c`"),
                );
            }
            let span = lhs.span.to(rhs.span);
            lhs = self.expr_node(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
            chain += 1;
            if !self.enter() {
                chain += 1;
                self.depth_cleanup(chain);
                self.give_up();
                return lhs;
            }
        }
        self.depth_cleanup(chain);
        lhs
    }

    fn depth_cleanup(&mut self, levels: u32) {
        for _ in 0..levels {
            self.leave();
        }
    }

    fn unary(&mut self, ctx: Ctx) -> Expr {
        let token = self.nth(0).clone();
        let op = match token.kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Bang => UnaryOp::Not,
            TokenKind::Tilde => UnaryOp::BitNot,
            _ => return self.postfix(ctx),
        };
        self.bump();
        if !self.enter() {
            self.leave();
            self.give_up();
            return self.error_expr(token.span);
        }
        let operand = self.unary(ctx);
        self.leave();
        let span = token.span.to(operand.span);
        self.expr_node(
            ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            span,
        )
    }

    fn postfix(&mut self, ctx: Ctx) -> Expr {
        let mut expr = self.primary(ctx);
        let mut chain = 0u32;
        loop {
            // A line starting with `.` continues the expression (spec chapter 1, section 7).
            if !self.ignoring_lines() && self.raw(0).kind == TokenKind::Newline {
                let mut ahead = 0;
                while self.raw(ahead).kind == TokenKind::Newline {
                    ahead += 1;
                }
                if self.raw(ahead).kind == TokenKind::Dot {
                    self.skip_newlines();
                }
            }
            let token = self.nth(0).clone();
            let next = match token.kind {
                TokenKind::Dot => self.dot_suffix(expr),
                TokenKind::LParen => self.call(expr),
                TokenKind::LBracket => self.index(expr),
                TokenKind::Question => {
                    self.bump();
                    let span = expr.span.to(token.span);
                    self.expr_node(ExprKind::Try(Box::new(expr)), span)
                }
                TokenKind::LBrace if as_path(&expr).is_some() => {
                    if ctx.no_struct {
                        if self.looks_like_struct_literal(&expr) {
                            self.report(
                                Diagnostic::new(
                                    codes::E0306,
                                    "struct literal in a condition needs parentheses",
                                )
                                .primary(expr.span, "this looks like a struct literal")
                                .help("wrap it in parentheses: `(Point { x: 0, y: 0 })`"),
                            );
                            self.struct_literal(expr)
                        } else {
                            break;
                        }
                    } else {
                        self.struct_literal(expr)
                    }
                }
                _ => break,
            };
            expr = next;
            chain += 1;
            if !self.enter() {
                chain += 1;
                self.depth_cleanup(chain);
                self.give_up();
                return expr;
            }
        }
        self.depth_cleanup(chain);
        expr
    }

    fn looks_like_struct_literal(&self, path: &Expr) -> bool {
        let uppercase = match &path.kind {
            ExprKind::Name(n) => n.name.starts_with(|c: char| c.is_ascii_uppercase()),
            ExprKind::Field { name, .. } => name.name.starts_with(|c: char| c.is_ascii_uppercase()),
            ExprKind::TypeArgs { .. } => true,
            _ => false,
        };
        if !uppercase {
            return false;
        }
        let mut ahead = 1;
        while self.raw(ahead).kind == TokenKind::Newline {
            ahead += 1;
        }
        let first = &self.raw(ahead).kind;
        let second = &self.raw(ahead + 1).kind;
        (ahead == 1 && matches!(first, TokenKind::RBrace))
            || (*first == TokenKind::Ident
                && matches!(
                    second,
                    TokenKind::Colon | TokenKind::Comma | TokenKind::RBrace
                ))
    }

    fn dot_suffix(&mut self, base: Expr) -> Expr {
        self.bump(); // `.`
        self.skip_newlines();
        if self.at(&TokenKind::Lt) {
            self.bump();
            let args = self.ignore_lines(|p| {
                let mut args = Vec::new();
                while !p.at(&TokenKind::Gt) && !p.at(&TokenKind::Shr) && !p.at(&TokenKind::Eof) {
                    args.push(p.ty());
                    if !p.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                if !p.eat_gt() {
                    p.unexpected("`>`");
                }
                args
            });
            let span = self.span_from(base.span);
            return self.expr_node(
                ExprKind::TypeArgs {
                    base: Box::new(base),
                    args,
                },
                span,
            );
        }
        match self.ident("a field or method name after `.`") {
            Some(name) => {
                let span = base.span.to(name.span);
                self.expr_node(
                    ExprKind::Field {
                        base: Box::new(base),
                        name,
                    },
                    span,
                )
            }
            None => {
                let span = base.span;
                self.error_expr(span)
            }
        }
    }

    fn call(&mut self, callee: Expr) -> Expr {
        let open = self.bump().span; // `(`
        let args = self.ignore_lines(|p| {
            let mut args = Vec::new();
            while !p.at(&TokenKind::RParen) && !p.at(&TokenKind::Eof) {
                let before = p.pos;
                let mode = p.mode();
                let value = p.expr();
                p.check_no_assignment();
                p.check_no_range();
                args.push(Arg { mode, value });
                if !p.eat(&TokenKind::Comma) || p.pos == before {
                    break;
                }
            }
            args
        });
        self.close(&TokenKind::RParen, "`)`", open);
        let span = self.span_from(callee.span);
        self.expr_node(
            ExprKind::Call {
                callee: Box::new(callee),
                args,
            },
            span,
        )
    }

    /// Consumes a closing delimiter, or reports it missing at its opener (E0302).
    fn close(&mut self, kind: &TokenKind, what: &str, open: Span) {
        // The closer belongs to the delimited group, where newlines are ignored.
        if self.ignore_lines(|p| p.eat(kind)) {
            return;
        }
        if self.at(&TokenKind::Error) || self.too_deep() {
            self.recover();
            self.eat(kind);
            return;
        }
        let span = self.ignore_lines(|p| p.peek_span());
        self.report(
            Diagnostic::new(codes::E0302, format!("unclosed delimiter, expected {what}"))
                .primary(open, "this delimiter is never closed")
                .secondary(
                    Span::new(span.file, span.start, span.start),
                    format!("expected {what} here"),
                ),
        );
        // No skipping: the following lines are most likely ordinary code.
    }

    fn index(&mut self, base: Expr) -> Expr {
        let open = self.bump().span; // `[`
        let result = self.ignore_lines(|p| {
            if p.at(&TokenKind::DotDot) || p.at(&TokenKind::DotDotEq) {
                let inclusive = p.bump().kind == TokenKind::DotDotEq;
                let end = if p.at(&TokenKind::RBracket) {
                    None
                } else {
                    Some(Box::new(p.expr()))
                };
                return (None, true, end, inclusive);
            }
            let first = p.expr();
            if p.at(&TokenKind::DotDot) || p.at(&TokenKind::DotDotEq) {
                let inclusive = p.bump().kind == TokenKind::DotDotEq;
                let end = if p.at(&TokenKind::RBracket) {
                    None
                } else {
                    Some(Box::new(p.expr()))
                };
                return (Some(Box::new(first)), true, end, inclusive);
            }
            (Some(Box::new(first)), false, None, false)
        });
        self.close(&TokenKind::RBracket, "`]`", open);
        let span = self.span_from(base.span);
        let (start, is_slice, end, inclusive) = result;
        let kind = match (is_slice, start) {
            (false, Some(index)) => ExprKind::Index {
                base: Box::new(base),
                index,
            },
            (_, start) => ExprKind::Slice {
                base: Box::new(base),
                start,
                end,
                inclusive,
            },
        };
        self.expr_node(kind, span)
    }

    fn struct_literal(&mut self, path_expr: Expr) -> Expr {
        let (path, type_args) = as_path(&path_expr).unwrap_or_else(|| {
            (
                Path {
                    segments: Vec::new(),
                    span: path_expr.span,
                },
                Vec::new(),
            )
        });
        let open = self.bump().span; // `{`
        let fields = self.ignore_lines(|p| {
            let mut fields = Vec::new();
            while p.at(&TokenKind::Ident) {
                let Some(name) = p.ident("a field name") else {
                    break;
                };
                let value = if p.eat(&TokenKind::Colon) {
                    let value = p.expr();
                    p.check_no_assignment();
                    Some(value)
                } else {
                    None
                };
                fields.push(FieldInit { name, value });
                if !p.eat(&TokenKind::Comma) {
                    break;
                }
            }
            fields
        });
        let stuck = self.ignore_lines(|p| !p.at(&TokenKind::RBrace) && !p.at(&TokenKind::Error));
        if stuck {
            self.ignore_lines(|p| p.unexpected("a field name or `}`"));
        }
        self.close(&TokenKind::RBrace, "`}`", open);
        let span = self.span_from(path_expr.span);
        self.expr_node(
            ExprKind::StructLit {
                path,
                type_args,
                fields,
            },
            span,
        )
    }

    fn primary(&mut self, ctx: Ctx) -> Expr {
        let token = self.nth(0).clone();
        let span = token.span;
        let kind = match token.kind {
            TokenKind::Int(value) => {
                self.bump();
                ExprKind::Int(value)
            }
            TokenKind::Float(value) => {
                self.bump();
                ExprKind::Float(value)
            }
            TokenKind::Str(value) => {
                self.bump();
                ExprKind::Str(value)
            }
            TokenKind::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            TokenKind::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            TokenKind::Ident => {
                self.bump();
                ExprKind::Name(Ident {
                    name: self.source(span).to_string(),
                    span,
                })
            }
            TokenKind::SelfValue => {
                self.bump();
                ExprKind::SelfValue
            }
            TokenKind::SelfType => {
                self.bump();
                ExprKind::SelfType
            }
            TokenKind::LParen => return self.paren(),
            TokenKind::LBracket => return self.list_or_map(),
            TokenKind::If => return self.if_expr(),
            TokenKind::Match => return self.match_expr(),
            TokenKind::Pipe | TokenKind::OrOr => return self.closure(ctx),
            TokenKind::Return => {
                self.bump();
                let ends = matches!(
                    self.raw(0).kind,
                    TokenKind::Newline
                        | TokenKind::Eof
                        | TokenKind::RBrace
                        | TokenKind::RParen
                        | TokenKind::RBracket
                        | TokenKind::Comma
                );
                if ends {
                    ExprKind::Return(None)
                } else {
                    let value = self.expr_with(ctx);
                    let full = span.to(value.span);
                    return self.expr_node(ExprKind::Return(Some(Box::new(value))), full);
                }
            }
            TokenKind::Break => {
                self.bump();
                ExprKind::Break
            }
            TokenKind::Continue => {
                self.bump();
                ExprKind::Continue
            }
            TokenKind::Error => {
                self.bump();
                ExprKind::Error
            }
            TokenKind::DotDot | TokenKind::DotDotEq => {
                self.check_no_range();
                ExprKind::Error
            }
            _ => {
                self.unexpected("an expression");
                if !matches!(
                    self.raw(0).kind,
                    TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket
                ) {
                    self.recover();
                }
                ExprKind::Error
            }
        };
        let span = self.span_from(span);
        self.expr_node(kind, span)
    }

    fn paren(&mut self) -> Expr {
        let open = self.bump().span; // `(`
        if self.ignore_lines(|p| p.eat(&TokenKind::RParen)) {
            let span = self.span_from(open);
            return self.expr_node(ExprKind::Unit, span);
        }
        let inner = self.ignore_lines(|p| {
            let inner = p.expr();
            p.check_no_assignment();
            p.check_no_range();
            if p.at(&TokenKind::Comma) {
                let span = p.peek_span();
                p.report(
                    Diagnostic::new(codes::E0301, "expected `)`, found `,`")
                        .primary(span, "there are no tuples in Wolfpack")
                        .help("use a struct with named fields"),
                );
                p.recover();
            }
            inner
        });
        self.close(&TokenKind::RParen, "`)`", open);
        let span = self.span_from(open);
        self.expr_node(ExprKind::Paren(Box::new(inner)), span)
    }

    fn list_or_map(&mut self) -> Expr {
        let open = self.bump().span; // `[`
        let kind = self.ignore_lines(|p| {
            if p.at(&TokenKind::Colon) && p.nth_is(1, &TokenKind::RBracket) {
                p.bump();
                return ExprKind::Map(Vec::new());
            }
            if p.at(&TokenKind::RBracket) {
                return ExprKind::List(Vec::new());
            }
            let first = p.expr();
            p.check_no_assignment();
            if p.eat(&TokenKind::Colon) {
                let value = p.expr();
                let mut entries = vec![(first, value)];
                while p.eat(&TokenKind::Comma) {
                    if p.at(&TokenKind::RBracket) {
                        break;
                    }
                    let before = p.pos;
                    let key = p.expr();
                    if p.expect(&TokenKind::Colon, "`:` in a map literal")
                        .is_none()
                    {
                        p.recover();
                        break;
                    }
                    let value = p.expr();
                    entries.push((key, value));
                    if p.pos == before {
                        break;
                    }
                }
                return ExprKind::Map(entries);
            }
            let mut items = vec![first];
            while p.eat(&TokenKind::Comma) {
                if p.at(&TokenKind::RBracket) {
                    break;
                }
                let before = p.pos;
                items.push(p.expr());
                p.check_no_assignment();
                if p.pos == before {
                    break;
                }
            }
            ExprKind::List(items)
        });
        self.close(&TokenKind::RBracket, "`]`", open);
        let span = self.span_from(open);
        self.expr_node(kind, span)
    }

    fn if_expr(&mut self) -> Expr {
        let start = self.bump().span; // `if`
        let cond = self.expr_with(NO_STRUCT);
        self.check_no_assignment();
        let Some(then) = self.block() else {
            self.recover();
            return self.error_expr(start);
        };
        // `else` must be on the line of `}`; a newline in between ends the `if` (E0304 later).
        let otherwise = if self.raw(0).kind == TokenKind::Else {
            self.bump();
            if self.at(&TokenKind::If) {
                Some(Box::new(self.if_expr()))
            } else {
                match self.block() {
                    Some(block) => {
                        let span = block.span;
                        Some(Box::new(self.expr_node(ExprKind::Block(block), span)))
                    }
                    None => None,
                }
            }
        } else {
            None
        };
        let span = self.span_from(start);
        self.expr_node(
            ExprKind::If {
                cond: Box::new(cond),
                then,
                otherwise,
            },
            span,
        )
    }

    fn match_expr(&mut self) -> Expr {
        let start = self.bump().span; // `match`
        let mode = self.mode();
        let scrutinee = self.expr_with(NO_STRUCT);
        let Some(open) = self.expect(&TokenKind::LBrace, "`{` after the matched value") else {
            self.recover();
            return self.error_expr(start);
        };
        let arms = self.significant_lines(|p| {
            let mut arms = Vec::new();
            loop {
                p.skip_newlines();
                if p.at(&TokenKind::RBrace) || p.resync_to_top {
                    break;
                }
                if p.at(&TokenKind::Eof) {
                    p.report(
                        Diagnostic::new(codes::E0302, "unclosed `{`")
                            .primary(open, "this `{` is never closed"),
                    );
                    break;
                }
                let before = p.pos;
                let errors = p.start_line();
                if let Some(arm) = p.arm() {
                    arms.push(arm);
                }
                if p.end_line_after_error(errors) {
                    if p.pos == before {
                        p.bump();
                    }
                    continue;
                }
                match p.raw(0).kind {
                    TokenKind::Newline | TokenKind::RBrace | TokenKind::Eof => {}
                    TokenKind::Comma => {
                        let span = p.raw(0).span;
                        p.report(
                            Diagnostic::new(codes::E0301, "unexpected `,` after a match arm")
                                .primary(span, "arms are separated by new lines")
                                .help("remove the `,`"),
                        );
                        p.bump();
                    }
                    _ => {
                        p.unexpected("a new line after the match arm");
                        p.recover();
                    }
                }
                if p.pos == before {
                    p.bump();
                }
            }
            arms
        });
        if !self.resync_to_top {
            self.eat(&TokenKind::RBrace);
        }
        let span = self.span_from(start);
        self.expr_node(
            ExprKind::Match {
                mode,
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        )
    }

    fn arm(&mut self) -> Option<Arm> {
        let start = self.peek_span();
        let pattern = self.pattern();
        let guard = if self.eat(&TokenKind::If) {
            Some(self.expr())
        } else {
            None
        };
        if self.expect(&TokenKind::FatArrow, "`=>`").is_none() {
            self.recover();
            return None;
        }
        self.skip_newlines();
        let body = if self.at(&TokenKind::LBrace) {
            let block = self.block()?;
            let span = block.span;
            self.expr_node(ExprKind::Block(block), span)
        } else {
            self.expr()
        };
        Some(Arm {
            pattern,
            guard,
            body,
            span: self.span_from(start),
        })
    }

    fn closure(&mut self, ctx: Ctx) -> Expr {
        let start = self.peek_span();
        let mut params = Vec::new();
        if self.eat(&TokenKind::OrOr) {
            // `||`: no parameters.
        } else {
            self.bump(); // `|`
            self.ignore_lines(|p| {
                while p.at(&TokenKind::Ident) {
                    let Some(name) = p.ident("a parameter name") else {
                        break;
                    };
                    let ty = if p.eat(&TokenKind::Colon) {
                        Some(p.ty())
                    } else {
                        None
                    };
                    params.push(ClosureParam { name, ty });
                    if !p.eat(&TokenKind::Comma) {
                        break;
                    }
                }
            });
            if self
                .expect(&TokenKind::Pipe, "`|` after the closure parameters")
                .is_none()
            {
                self.recover();
                return self.error_expr(start);
            }
        }
        let ret = if self.eat_then_lines(&TokenKind::Arrow) {
            Some(self.ty())
        } else {
            None
        };
        let body = if self.at(&TokenKind::LBrace) {
            match self.block() {
                Some(block) => {
                    let span = block.span;
                    self.expr_node(ExprKind::Block(block), span)
                }
                None => self.error_expr(start),
            }
        } else {
            if let Some(ret) = &ret {
                let span = ret.span;
                self.report(
                    Diagnostic::new(
                        codes::E0310,
                        "closure with a result type needs a block body",
                    )
                    .primary(span, "result type written here")
                    .help("write the body in braces: `|x: Int| -> Int { x + 1 }`"),
                );
            }
            self.expr_with(ctx)
        };
        let span = start.to(body.span);
        self.expr_node(
            ExprKind::Closure {
                params,
                ret,
                body: Box::new(body),
            },
            span,
        )
    }
}

/// A place that can be assigned: a variable, `self`, and fields and indices of those.
fn is_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Name(_) | ExprKind::SelfValue => true,
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => is_place(base),
        ExprKind::Error => true,
        _ => false,
    }
}

fn is_comparison(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Binary { op, .. } if op.is_comparison())
}

/// `parse < Int` as the left side of `>`: probably meant `parse.<Int>`.
fn looks_like_type_args(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Binary {
            op: BinaryOp::Lt,
            lhs,
            rhs,
        } => as_path(lhs).is_some() && as_path(rhs).is_some(),
        _ => false,
    }
}

/// If `expr` is a dotted path with optional type arguments at the end (`a.b.<T>`), returns it.
fn as_path(expr: &Expr) -> Option<(Path, Vec<Ty>)> {
    fn segments(expr: &Expr, out: &mut Vec<Ident>) -> bool {
        match &expr.kind {
            ExprKind::Name(n) => {
                out.push(n.clone());
                true
            }
            ExprKind::Field { base, name } if segments(base, out) => {
                out.push(name.clone());
                true
            }
            _ => false,
        }
    }
    let (base, args) = match &expr.kind {
        ExprKind::TypeArgs { base, args } => (base.as_ref(), args.clone()),
        _ => (expr, Vec::new()),
    };
    let mut out = Vec::new();
    if segments(base, &mut out) {
        let span = base.span;
        Some((
            Path {
                segments: out,
                span,
            },
            args,
        ))
    } else {
        None
    }
}
