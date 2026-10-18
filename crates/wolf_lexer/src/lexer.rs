//! The lexer state machine.

use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, Span};

use crate::Lexed;
use crate::cursor::Cursor;
use crate::token::{Comment, FPart, Token, TokenKind};

/// The largest integer literal: 2^63, valid only directly under a unary minus.
const INT_LIMIT: u128 = 1 << 63;

/// How deep interpolated strings may nest inside each other's interpolations.
const MAX_FSTRING_DEPTH: u32 = 32;

pub(crate) struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    cursor: Cursor,
    out: Lexed,
    /// How many interpolations the lexer is inside.
    fstring_depth: u32,
}

/// The form of a string literal (spec chapter 1, sections 5.3 and 5.3a).
#[derive(Clone, Copy)]
struct Form {
    /// `f"..."`
    interpolated: bool,
    /// `r"..."`
    raw: bool,
}

/// One line of the content of a string while it is read.
#[derive(Default)]
struct Line {
    /// The spaces and tabs at the start of the line (multiline strings only).
    lead: String,
    /// Offset of the start of the line.
    start: u32,
    parts: Vec<FPart>,
}

impl Line {
    fn push_char(&mut self, c: char) {
        match self.parts.last_mut() {
            Some(FPart::Text(text)) => text.push(c),
            _ => self.parts.push(FPart::Text(c.to_string())),
        }
    }

    fn is_blank(&self) -> bool {
        self.parts.is_empty()
    }
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(file: FileId, text: &'a str) -> Lexer<'a> {
        Lexer {
            file,
            text,
            cursor: Cursor::new(text),
            out: Lexed::default(),
            fstring_depth: 0,
        }
    }

    pub(crate) fn run(mut self) -> Lexed {
        // A byte order mark is ignored, but only at the very start (rule 1.2).
        self.cursor.eat('\u{feff}');
        while let Some(c) = self.cursor.peek(0) {
            self.step(c);
        }
        let end = self.cursor.offset();
        self.push(TokenKind::Eof, end, end);
        self.out
    }

    /// Lexes the text that starts with `c`.
    fn step(&mut self, c: char) {
        let start = self.cursor.offset();
        match c {
            ' ' | '\t' => {
                self.cursor.bump();
            }
            '\n' => {
                self.cursor.bump();
                self.line_end(start);
            }
            '\r' => {
                self.cursor.bump();
                if self.cursor.eat('\n') {
                    self.line_end(start);
                } else {
                    self.lone_cr(start);
                }
            }
            '/' if self.cursor.peek(1) == Some('/') => self.line_comment(start),
            '/' if self.cursor.peek(1) == Some('*') => self.block_comment(start),
            '"' => self.string(
                start,
                Form {
                    interpolated: false,
                    raw: false,
                },
            ),
            '0'..='9' => self.number(start),
            c if is_ident_start(c) => self.ident(start),
            _ => self.punctuation(start, c),
        }
    }

    // ----- helpers -------------------------------------------------------------------------

    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file, start, end)
    }

    fn push(&mut self, kind: TokenKind, start: u32, end: u32) {
        let span = self.span(start, end);
        self.out.tokens.push(Token { kind, span });
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.out.diagnostics.push(diagnostic);
    }

    fn source(&self, start: u32, end: u32) -> &'a str {
        self.text.get(start as usize..end as usize).unwrap_or("")
    }

    /// Records a line end. Consecutive line ends and comment-only lines collapse into one
    /// `Newline` token, and no `Newline` is emitted before the first token (section 7).
    fn line_end(&mut self, start: u32) {
        let end = self.cursor.offset();
        match self.out.tokens.last() {
            None => {}
            Some(token) if token.kind == TokenKind::Newline => {}
            Some(_) => self.push(TokenKind::Newline, start, end),
        }
    }

    fn lone_cr(&mut self, start: u32) {
        let span = self.span(start, start + 1);
        self.error(
            Diagnostic::new(codes::E0110, "carriage return without line feed")
                .primary(span, "a line must end with `\\n` or `\\r\\n`")
                .help("save the file with LF or CRLF line endings"),
        );
    }

    // ----- comments ------------------------------------------------------------------------

    fn line_comment(&mut self, start: u32) {
        // `//` already checked.
        self.cursor.bump();
        self.cursor.bump();
        let kind = match (self.cursor.peek(0), self.cursor.peek(1)) {
            (Some('/'), Some('/')) => LineKind::Plain,
            (Some('/'), _) => LineKind::Outer,
            (Some('!'), _) => LineKind::Inner,
            _ => LineKind::Plain,
        };
        if kind != LineKind::Plain {
            self.cursor.bump();
        }
        let content_start = self.cursor.offset();
        while let Some(c) = self.cursor.peek(0) {
            if c == '\n' || (c == '\r' && self.cursor.peek(1) == Some('\n')) {
                break;
            }
            let at = self.cursor.offset();
            self.cursor.bump();
            if c == '\r' {
                self.lone_cr(at);
            }
        }
        let end = self.cursor.offset();
        let content = self.source(content_start, end).to_string();
        match kind {
            LineKind::Plain => self.out.comments.push(Comment {
                span: self.span(start, end),
                block: false,
            }),
            LineKind::Outer => self.push(TokenKind::DocOuter(content), start, end),
            LineKind::Inner => self.push(TokenKind::DocInner(content), start, end),
        }
    }

    fn block_comment(&mut self, start: u32) {
        // `/*` already checked.
        self.cursor.bump();
        self.cursor.bump();
        let kind = match (self.cursor.peek(0), self.cursor.peek(1)) {
            // `/**/` and `/***` are ordinary comments.
            (Some('*'), Some('*')) | (Some('*'), Some('/')) => LineKind::Plain,
            (Some('*'), _) => LineKind::Outer,
            (Some('!'), _) => LineKind::Inner,
            _ => LineKind::Plain,
        };
        if kind != LineKind::Plain {
            self.cursor.bump();
        }
        let content_start = self.cursor.offset();
        let mut depth = 1u32;
        let mut has_newline = false;
        let mut content_end = content_start;
        while depth > 0 {
            let at = self.cursor.offset();
            match (self.cursor.peek(0), self.cursor.peek(1)) {
                (None, _) => {
                    let span = self.span(start, start + 2);
                    self.error(
                        Diagnostic::new(codes::E0112, "unterminated block comment")
                            .primary(span, "this comment is never closed")
                            .note("block comments nest: every `/*` needs its own `*/`"),
                    );
                    content_end = at;
                    break;
                }
                (Some('*'), Some('/')) => {
                    self.cursor.bump();
                    self.cursor.bump();
                    depth -= 1;
                    content_end = at;
                }
                (Some('/'), Some('*')) => {
                    self.cursor.bump();
                    self.cursor.bump();
                    depth += 1;
                }
                (Some('\n'), _) => {
                    self.cursor.bump();
                    has_newline = true;
                }
                (Some('\r'), next) => {
                    self.cursor.bump();
                    if next == Some('\n') {
                        self.cursor.bump();
                        has_newline = true;
                    } else {
                        self.lone_cr(at);
                    }
                }
                (Some(_), _) => {
                    self.cursor.bump();
                }
            }
        }
        let end = self.cursor.offset();
        let content = self.source(content_start, content_end).to_string();
        match kind {
            LineKind::Plain => {
                self.out.comments.push(Comment {
                    span: self.span(start, end),
                    block: true,
                });
                // A block comment containing a line end counts as a line end (section 2).
                if has_newline {
                    self.line_end(start);
                }
            }
            LineKind::Outer => self.push(TokenKind::DocOuter(content), start, end),
            LineKind::Inner => self.push(TokenKind::DocInner(content), start, end),
        }
    }

    // ----- identifiers and keywords --------------------------------------------------------

    fn ident(&mut self, start: u32) {
        let mut non_ascii = false;
        while let Some(c) = self.cursor.peek(0) {
            if is_ident_continue(c) {
                non_ascii |= !c.is_ascii();
                self.cursor.bump();
            } else {
                break;
            }
        }
        let end = self.cursor.offset();
        let text = self.source(start, end);
        if non_ascii {
            let span = self.span(start, end);
            self.error(
                Diagnostic::new(
                    codes::E0111,
                    format!("non-ASCII character in identifier `{text}`"),
                )
                .primary(
                    span,
                    "identifiers may only contain ASCII letters, digits and `_`",
                )
                .note("names are translated to the naming style of each target, which needs ASCII"),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        if self.cursor.peek(0) == Some('"') {
            // `f"..."` and `r"..."` (section 5.3a); other prefixes are reserved (rule 5.3.5).
            match text {
                "f" => {
                    let form = Form {
                        interpolated: true,
                        raw: false,
                    };
                    return self.string(start, form);
                }
                "r" => {
                    let form = Form {
                        interpolated: false,
                        raw: true,
                    };
                    return self.string(start, form);
                }
                _ => {}
            }
            let span = self.span(start, end + 1);
            self.error(
                Diagnostic::new(codes::E0113, format!("reserved literal prefix `{text}`"))
                    .primary(span, "an identifier directly before `\"` is reserved")
                    .help(format!("add a space after `{text}` or remove the prefix")),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        let kind = if text == "_" {
            TokenKind::Underscore
        } else {
            TokenKind::keyword(text).unwrap_or(TokenKind::Ident)
        };
        self.push(kind, start, end);
    }

    // ----- numbers -------------------------------------------------------------------------

    fn number(&mut self, start: u32) {
        let mut problems = Vec::new();
        let radix = match (self.cursor.peek(0), self.cursor.peek(1)) {
            (Some('0'), Some(p @ ('x' | 'o' | 'b' | 'X' | 'O' | 'B'))) => {
                self.cursor.bump();
                self.cursor.bump();
                if p.is_ascii_uppercase() {
                    problems.push(format!(
                        "the prefix must be lower case: `0{}`",
                        p.to_ascii_lowercase()
                    ));
                }
                match p.to_ascii_lowercase() {
                    'x' => 16,
                    'o' => 8,
                    _ => 2,
                }
            }
            _ => 10,
        };

        let digits = self.digits(radix, &mut problems);
        if radix != 10 {
            if digits.is_empty() {
                problems.push("expected digits after the prefix".to_string());
            }
            self.suffix(&mut problems);
            let end = self.cursor.offset();
            self.finish_int(start, end, &digits, radix, problems);
            return;
        }

        if digits.len() > 1 && digits.starts_with('0') {
            problems
                .push("leading zeros are not allowed; octal numbers are written `0o7`".to_string());
        }

        let mut is_float = false;
        let mut float_text = digits.clone();
        // A fraction needs a digit after the dot; `1..5` and `1.copy()` are not floats.
        if self.cursor.peek(0) == Some('.') {
            match self.cursor.peek(1) {
                Some(d) if d.is_ascii_digit() => {
                    self.cursor.bump();
                    let fraction = self.digits(10, &mut problems);
                    float_text.push('.');
                    float_text.push_str(&fraction);
                    is_float = true;
                }
                Some(c) if c == '.' || is_ident_start(c) => {}
                _ => {
                    self.cursor.bump();
                    problems
                        .push("a float needs a digit after the dot, for example `1.0`".to_string());
                }
            }
        }
        if let Some('e' | 'E') = self.cursor.peek(0) {
            let sign = matches!(self.cursor.peek(1), Some('+' | '-'));
            let digit_at = if sign { 2 } else { 1 };
            if self
                .cursor
                .peek(digit_at)
                .is_some_and(|c| c.is_ascii_digit())
            {
                self.cursor.bump();
                float_text.push('e');
                if sign {
                    if let Some(s) = self.cursor.bump() {
                        float_text.push(s);
                    }
                }
                let exponent = self.digits(10, &mut problems);
                float_text.push_str(&exponent);
                is_float = true;
            } else {
                self.cursor.bump();
                if sign {
                    self.cursor.bump();
                }
                problems.push("an exponent needs digits, for example `1e10`".to_string());
            }
        }
        self.suffix(&mut problems);
        let end = self.cursor.offset();

        if !problems.is_empty() || !is_float {
            self.finish_int(start, end, &digits, 10, problems);
            return;
        }
        match float_text.parse::<f64>() {
            Ok(value) if value.is_finite() => self.push(TokenKind::Float(value), start, end),
            _ => {
                let span = self.span(start, end);
                self.error(
                    Diagnostic::new(codes::E0108, "float literal out of range")
                        .primary(span, "this value is too large for `Float`")
                        .note("the largest `Float` is about 1.8e308"),
                );
                self.push(TokenKind::Error, start, end);
            }
        }
    }

    /// Reads digits of `radix` with `_` separators. Returns the digits without separators.
    fn digits(&mut self, radix: u32, problems: &mut Vec<String>) -> String {
        let mut out = String::new();
        let mut bad_separator = false;
        while let Some(c) = self.cursor.peek(0) {
            if c.is_digit(radix) {
                out.push(c);
                self.cursor.bump();
            } else if c == '_' {
                let before = self.cursor.prev().is_some_and(|p| p.is_digit(radix));
                let after = self.cursor.peek(1).is_some_and(|n| n.is_digit(radix));
                // Only a separator between two digits is allowed (rule 5.1.2).
                if !(before && after) || out.is_empty() {
                    bad_separator = true;
                }
                self.cursor.bump();
            } else {
                break;
            }
        }
        if bad_separator {
            problems.push("`_` may only stand between two digits".to_string());
        }
        out
    }

    /// Consumes letters and digits glued to a number (reserved for type suffixes, rule 5.1.5).
    fn suffix(&mut self, problems: &mut Vec<String>) {
        let start = self.cursor.offset();
        while self.cursor.peek(0).is_some_and(is_ident_continue) {
            self.cursor.bump();
        }
        let end = self.cursor.offset();
        if end > start {
            let text = self.source(start, end);
            problems.push(format!(
                "unexpected `{text}` after the number; separate them with a space"
            ));
        }
    }

    fn finish_int(
        &mut self,
        start: u32,
        end: u32,
        digits: &str,
        radix: u32,
        problems: Vec<String>,
    ) {
        let span = self.span(start, end);
        if let Some(first) = problems.into_iter().next() {
            let text = self.source(start, end);
            self.error(
                Diagnostic::new(codes::E0107, format!("malformed number literal `{text}`"))
                    .primary(span, first),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        let mut value: u128 = 0;
        for c in digits.chars() {
            let digit = c.to_digit(radix).map(u128::from).unwrap_or(0);
            value = value
                .saturating_mul(u128::from(radix))
                .saturating_add(digit);
            if value > INT_LIMIT {
                break;
            }
        }
        match u64::try_from(value) {
            Ok(value) if u128::from(value) <= INT_LIMIT => {
                self.push(TokenKind::Int(value), start, end)
            }
            _ => {
                self.error(
                    Diagnostic::new(codes::E0106, "integer literal too large")
                        .primary(span, "this value does not fit into `Int`")
                        .note("`Int` is a signed 64-bit integer; the largest value is 9223372036854775807"),
                );
                self.push(TokenKind::Error, start, end);
            }
        }
    }

    // ----- strings -------------------------------------------------------------------------

    /// Lexes a string literal whose opening `"` is next. `start` is where the token starts,
    /// before a prefix (sections 5.3 and 5.3a).
    fn string(&mut self, start: u32, form: Form) {
        let multiline = self.cursor.peek(1) == Some('"') && self.cursor.peek(2) == Some('"');
        let lines = if multiline {
            for _ in 0..3 {
                self.cursor.bump();
            }
            self.multiline_body(start, form)
        } else {
            self.cursor.bump();
            let mut line = Line {
                start: self.cursor.offset(),
                ..Line::default()
            };
            self.single_body(start, form, &mut line);
            vec![line.parts]
        };
        let end = self.cursor.offset();
        let mut parts: Vec<FPart> = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            if i > 0 {
                push_text(&mut parts, "\n");
            }
            for part in line {
                match part {
                    FPart::Text(text) => push_text(&mut parts, &text),
                    expr => parts.push(expr),
                }
            }
        }
        let kind = if form.interpolated {
            TokenKind::FStr(parts)
        } else {
            let mut value = String::new();
            for part in parts {
                if let FPart::Text(text) = part {
                    value.push_str(&text);
                }
            }
            TokenKind::Str(value)
        };
        self.push(kind, start, end);
    }

    /// The content of a `"..."` string after the opening quote, up to and including the closing
    /// quote.
    fn single_body(&mut self, start: u32, form: Form, line: &mut Line) {
        loop {
            let at = self.cursor.offset();
            match self.cursor.peek(0) {
                None | Some('\n') => {
                    self.unterminated(start, at);
                    return;
                }
                Some('\r') if self.cursor.peek(1) == Some('\n') => {
                    self.unterminated(start, at);
                    return;
                }
                Some('"') => {
                    self.cursor.bump();
                    return;
                }
                Some(c) => self.content_char(c, form, line, at),
            }
        }
    }

    /// The content of a `"""` string after the opening quotes, up to and including the closing
    /// quotes, as lines with the indentation removed (section 5.3a, rule 3).
    fn multiline_body(&mut self, start: u32, form: Form) -> Vec<Vec<FPart>> {
        let open_end = self.cursor.offset();
        // The opening `"""` must be the last thing on its line.
        let mut extra = None;
        loop {
            match self.cursor.peek(0) {
                Some(' ' | '\t') => {
                    self.cursor.bump();
                }
                Some('\n') => {
                    self.cursor.bump();
                    break;
                }
                Some('\r') if self.cursor.peek(1) == Some('\n') => {
                    self.cursor.bump();
                    self.cursor.bump();
                    break;
                }
                None => {
                    self.unterminated_multiline(start, open_end);
                    return Vec::new();
                }
                Some('"') if self.closing_quotes() => {
                    // `"""text"""` on one line.
                    let at = self.cursor.offset();
                    for _ in 0..3 {
                        self.cursor.bump();
                    }
                    let span = self.span(open_end, at);
                    self.error(
                        Diagnostic::new(codes::E0116, "a multiline string on one line")
                            .primary(span, "the content must start on the next line")
                            .help("use `\"...\"` for a string on one line"),
                    );
                    return Vec::new();
                }
                Some(_) => {
                    extra.get_or_insert(self.cursor.offset());
                    self.cursor.bump();
                }
            }
        }
        if let Some(at) = extra {
            let end = self.text_line_end(at);
            let span = self.span(at, end);
            self.error(
                Diagnostic::new(codes::E0116, "text after the opening `\"\"\"`")
                    .primary(
                        span,
                        "the opening `\"\"\"` must be the last thing on its line",
                    )
                    .help("start the content on the next line"),
            );
        }

        let mut lines: Vec<Line> = Vec::new();
        loop {
            let mut line = Line {
                start: self.cursor.offset(),
                ..Line::default()
            };
            while let Some(c @ (' ' | '\t')) = self.cursor.peek(0) {
                line.lead.push(c);
                self.cursor.bump();
            }
            if self.closing_quotes() {
                for _ in 0..3 {
                    self.cursor.bump();
                }
                let indent = line.lead;
                return self.strip_indentation(lines, &indent);
            }
            loop {
                let at = self.cursor.offset();
                match self.cursor.peek(0) {
                    None => {
                        self.unterminated_multiline(start, open_end);
                        lines.push(line);
                        return lines.into_iter().map(|l| l.parts).collect();
                    }
                    Some('\n') => {
                        self.cursor.bump();
                        break;
                    }
                    Some('\r') if self.cursor.peek(1) == Some('\n') => {
                        self.cursor.bump();
                        self.cursor.bump();
                        break;
                    }
                    Some('"') if self.closing_quotes() => {
                        for _ in 0..3 {
                            self.cursor.bump();
                        }
                        let span = self.span(at, at + 3);
                        self.error(
                            Diagnostic::new(
                                codes::E0116,
                                "the closing `\"\"\"` is not on its own line",
                            )
                            .primary(span, "this ends the string")
                            .help("move the closing `\"\"\"` to the next line"),
                        );
                        lines.push(line);
                        return lines.into_iter().map(|l| l.parts).collect();
                    }
                    Some('\\')
                        if !form.raw && matches!(self.cursor.peek(1), None | Some('\n' | '\r')) =>
                    {
                        self.cursor.bump();
                        let span = self.span(at, at + 1);
                        self.error(
                            Diagnostic::new(codes::E0104, "`\\` at the end of a line")
                                .primary(span, "not a valid escape")
                                .help("a line end in a `\"\"\"` string is part of the text; remove the `\\`"),
                        );
                    }
                    Some(c) => self.content_char(c, form, &mut line, at),
                }
            }
            lines.push(line);
        }
    }

    /// Removes the indentation of the closing quotes from every line (section 5.3a, rule 3).
    fn strip_indentation(&mut self, lines: Vec<Line>, indent: &str) -> Vec<Vec<FPart>> {
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let mut parts = Vec::new();
            match line.lead.strip_prefix(indent) {
                Some(rest) => push_text(&mut parts, rest),
                None if line.is_blank() => {}
                None => {
                    let end = line.start + u32::try_from(line.lead.len()).unwrap_or(0);
                    let span = self.span(line.start, end);
                    self.error(
                        Diagnostic::new(
                            codes::E0116,
                            "a line of a multiline string is less indented than its closing `\"\"\"`",
                        )
                        .primary(span, "this indentation does not start with the indentation of the closing `\"\"\"`")
                        .help("indent the line at least as far as the closing `\"\"\"`, with the same spaces or tabs"),
                    );
                    push_text(&mut parts, &line.lead);
                }
            }
            for part in line.parts {
                match part {
                    FPart::Text(text) => push_text(&mut parts, &text),
                    expr => parts.push(expr),
                }
            }
            out.push(parts);
        }
        out
    }

    /// Whether the next three characters are `"""`.
    fn closing_quotes(&self) -> bool {
        (0..3).all(|n| self.cursor.peek(n) == Some('"'))
    }

    /// The offset of the end of the line that contains `at`.
    fn text_line_end(&self, at: u32) -> u32 {
        let rest = self.text.get(at as usize..).unwrap_or("");
        let len = rest.find(['\n', '\r']).unwrap_or(rest.len());
        at + u32::try_from(len).unwrap_or(0)
    }

    /// One character (or escape, or interpolation) of the content of a string, at `at`.
    fn content_char(&mut self, c: char, form: Form, line: &mut Line, at: u32) {
        match c {
            '\\' if !form.raw => {
                self.cursor.bump();
                if let Some(c) = self.escape(at) {
                    line.push_char(c);
                }
            }
            '{' if form.interpolated => {
                if self.cursor.peek(1) == Some('{') {
                    self.cursor.bump();
                    self.cursor.bump();
                    line.push_char('{');
                } else {
                    self.interpolation(line, at);
                }
            }
            '}' if form.interpolated => {
                self.cursor.bump();
                if self.cursor.eat('}') {
                    line.push_char('}');
                } else {
                    let span = self.span(at, at + 1);
                    self.error(
                        Diagnostic::new(codes::E0114, "unmatched `}` in an interpolated string")
                            .primary(span, "this `}` closes nothing")
                            .help("write `}}` for the character `}`"),
                    );
                }
            }
            '\r' => {
                self.cursor.bump();
                self.lone_cr(at);
            }
            c => {
                self.cursor.bump();
                if (c.is_control() && c != '\t') || c == '\u{7f}' {
                    let span = self.span(at, self.cursor.offset());
                    self.error(
                        Diagnostic::new(
                            codes::E0109,
                            format!("unescaped control character U+{:04X} in a string", c as u32),
                        )
                        .primary(span, "control characters must be escaped")
                        .help(format!("write it as `\\u{{{:x}}}`", c as u32)),
                    );
                } else {
                    line.push_char(c);
                }
            }
        }
    }

    /// An interpolation `{expr}` of an `f` string; the `{` at `at` is next.
    fn interpolation(&mut self, line: &mut Line, at: u32) {
        self.cursor.bump(); // `{`
        if self.fstring_depth >= MAX_FSTRING_DEPTH {
            let span = self.span(at, at + 1);
            self.error(
                Diagnostic::new(codes::E0314, "interpolated strings are nested too deeply")
                    .primary(span, "this interpolation is too deep")
                    .help("move the inner strings into variables"),
            );
            self.skip_interpolation();
            return;
        }
        if !self.interpolation_closes() {
            // Most often the `}` was forgotten: the interpolation ends where the string does.
            let span = self.span(at, at + 1);
            self.error(
                Diagnostic::new(codes::E0114, "unclosed `{` in an interpolated string")
                    .primary(span, "this `{` is not closed")
                    .help("close the interpolation with `}`, or write `{{` for the character `{`"),
            );
            while let Some(c) = self.cursor.peek(0) {
                if matches!(c, '"' | '\n' | '\r') {
                    break;
                }
                self.cursor.bump();
            }
            return;
        }
        let saved = std::mem::take(&mut self.out.tokens);
        self.fstring_depth += 1;
        let mut depth = 0u32;
        let closed = loop {
            match self.cursor.peek(0) {
                None | Some('\n' | '\r') => break false,
                Some('}') if depth == 0 => break true,
                Some(c) => {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    self.step(c);
                }
            }
        };
        self.fstring_depth -= 1;
        let inner_end = self.cursor.offset();
        let tokens = std::mem::replace(&mut self.out.tokens, saved);
        if !closed {
            let span = self.span(at, at + 1);
            self.error(
                Diagnostic::new(codes::E0114, "unclosed `{` in an interpolated string")
                    .primary(span, "this `{` is not closed on its line")
                    .help("close the interpolation with `}`, or write `{{` for the character `{`"),
            );
            return;
        }
        self.cursor.bump(); // `}`
        let span = self.span(at, self.cursor.offset());
        if tokens.is_empty() {
            self.error(
                Diagnostic::new(codes::E0115, "empty interpolation")
                    .primary(span, "there is no expression between the braces")
                    .help(
                        "write an expression inside the braces, or `{{}}` for the characters `{}`",
                    ),
            );
            return;
        }
        let mut tokens = tokens;
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: self.span(inner_end, inner_end),
        });
        line.parts.push(FPart::Expr(tokens, span));
    }

    /// Whether the interpolation that starts after the `{` before the cursor has its matching `}`
    /// on this line. Nested strings are skipped.
    fn interpolation_closes(&self) -> bool {
        let mut depth = 0u32;
        let mut n = 0;
        loop {
            match self.cursor.peek(n) {
                None | Some('\n' | '\r') => return false,
                Some('{') => depth += 1,
                Some('}') if depth == 0 => return true,
                Some('}') => depth -= 1,
                Some('"') => loop {
                    n += 1;
                    match self.cursor.peek(n) {
                        None | Some('\n' | '\r') => return false,
                        Some('\\') => n += 1,
                        Some('"') => break,
                        Some(_) => {}
                    }
                },
                Some(_) => {}
            }
            n += 1;
        }
    }

    /// Skips an interpolation without lexing it, up to its closing `}` or the end of the line.
    fn skip_interpolation(&mut self) {
        let mut depth = 0u32;
        while let Some(c) = self.cursor.peek(0) {
            match c {
                '\n' | '\r' => return,
                '{' => depth += 1,
                '}' if depth == 0 => {
                    self.cursor.bump();
                    return;
                }
                '}' => depth -= 1,
                _ => {}
            }
            self.cursor.bump();
        }
    }

    fn unterminated_multiline(&mut self, start: u32, open_end: u32) {
        let span = self.span(start, open_end);
        self.error(
            Diagnostic::new(codes::E0103, "unterminated multiline string")
                .primary(span, "this `\"\"\"` is never closed")
                .help("close the string with `\"\"\"` on a line of its own"),
        );
    }

    fn unterminated(&mut self, start: u32, at: u32) {
        let span = self.span(start, at);
        self.error(
            Diagnostic::new(codes::E0103, "unterminated string literal")
                .primary(span, "this string is not closed before the end of the line")
                .help("add the closing `\"`; strings cannot span several lines"),
        );
    }

    /// Reads an escape after `\`. `at` is the offset of the backslash.
    fn escape(&mut self, at: u32) -> Option<char> {
        let c = match self.cursor.peek(0) {
            // The line ends right after `\`: the string is unterminated, reported by the caller.
            None | Some('\n') | Some('\r') => return None,
            Some(c) => c,
        };
        self.cursor.bump();
        match c {
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            '0' => Some('\0'),
            '\\' => Some('\\'),
            '"' => Some('"'),
            'x' => {
                let mut digits = String::new();
                for _ in 0..2 {
                    match self.cursor.peek(0) {
                        Some(d) if d.is_ascii_hexdigit() => {
                            digits.push(d);
                            self.cursor.bump();
                        }
                        _ => break,
                    }
                }
                let value = u32::from_str_radix(&digits, 16).ok();
                match value {
                    Some(v) if digits.len() == 2 && v <= 0x7f => char::from_u32(v),
                    _ => {
                        self.bad_escape(at, "`\\x` needs exactly two hex digits, at most `7F`");
                        None
                    }
                }
            }
            'u' => self.unicode_escape(at),
            other => {
                let span = self.span(at, self.cursor.offset());
                self.error(
                    Diagnostic::new(codes::E0104, format!("unknown escape sequence `\\{other}`"))
                        .primary(span, "not a valid escape")
                        .help("valid escapes are `\\n`, `\\r`, `\\t`, `\\0`, `\\\\`, `\\\"`, `\\xHH` and `\\u{...}`"),
                );
                None
            }
        }
    }

    fn unicode_escape(&mut self, at: u32) -> Option<char> {
        if !self.cursor.eat('{') {
            self.bad_escape(at, "write a Unicode escape as `\\u{1F600}`");
            return None;
        }
        let mut digits = String::new();
        while let Some(d) = self.cursor.peek(0) {
            if d.is_ascii_hexdigit() {
                digits.push(d);
                self.cursor.bump();
            } else {
                break;
            }
        }
        if !self.cursor.eat('}') {
            self.bad_escape(at, "a Unicode escape must end with `}`");
            return None;
        }
        if digits.is_empty() || digits.len() > 6 {
            self.bad_escape(at, "a Unicode escape needs 1 to 6 hex digits");
            return None;
        }
        let value = u32::from_str_radix(&digits, 16).ok();
        match value.and_then(char::from_u32) {
            Some(c) => Some(c),
            None => {
                self.bad_escape(
                    at,
                    "not a Unicode scalar value (surrogate or above U+10FFFF)",
                );
                None
            }
        }
    }

    fn bad_escape(&mut self, at: u32, label: &str) {
        let end = self.cursor.offset();
        let span = self.span(at, end);
        let text = self.source(at, end);
        self.error(
            Diagnostic::new(codes::E0105, format!("invalid character escape `{text}`"))
                .primary(span, label),
        );
    }

    // ----- punctuation ---------------------------------------------------------------------

    fn punctuation(&mut self, start: u32, c: char) {
        use TokenKind::*;
        self.cursor.bump();
        let kind = match c {
            '(' => LParen,
            ')' => RParen,
            '[' => LBracket,
            ']' => RBracket,
            '{' => LBrace,
            '}' => RBrace,
            ',' => Comma,
            ':' => Colon,
            '?' => Question,
            '~' => Tilde,
            '.' => {
                if self.cursor.eat('.') {
                    if self.cursor.eat('=') {
                        DotDotEq
                    } else {
                        DotDot
                    }
                } else if self.cursor.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                    // `.5`: a float needs a digit before the dot (rule 5.2.1).
                    while self.cursor.peek(0).is_some_and(is_ident_continue) {
                        self.cursor.bump();
                    }
                    let end = self.cursor.offset();
                    let text = self.source(start, end);
                    let span = self.span(start, end);
                    self.error(
                        Diagnostic::new(codes::E0107, format!("malformed number literal `{text}`"))
                            .primary(span, "a float needs a digit before the dot")
                            .help(format!("write `0{text}`")),
                    );
                    Error
                } else {
                    Dot
                }
            }
            '=' => self.pick(&[('=', EqEq), ('>', FatArrow)], Eq),
            '-' => self.pick(&[('>', Arrow), ('=', MinusEq)], Minus),
            '+' => self.pick(&[('=', PlusEq)], Plus),
            '*' => self.pick(&[('=', StarEq)], Star),
            '/' => self.pick(&[('=', SlashEq)], Slash),
            '%' => self.pick(&[('=', PercentEq)], Percent),
            '^' => self.pick(&[('=', CaretEq)], Caret),
            '!' => self.pick(&[('=', NotEq)], Bang),
            '&' => self.pick(&[('&', AndAnd), ('=', AmpEq)], Amp),
            '|' => self.pick(&[('|', OrOr), ('=', PipeEq)], Pipe),
            '<' => {
                if self.cursor.eat('<') {
                    self.pick(&[('=', ShlEq)], Shl)
                } else {
                    self.pick(&[('=', Le)], Lt)
                }
            }
            '>' => {
                if self.cursor.eat('>') {
                    self.pick(&[('=', ShrEq)], Shr)
                } else {
                    self.pick(&[('=', Ge)], Gt)
                }
            }
            '\'' => {
                // `'x'` is one mistake, not two: consume up to the closing quote on this line.
                let mut ahead = 0;
                while let Some(next) = self.cursor.peek(ahead) {
                    if next == '\n' || next == '\r' {
                        break;
                    }
                    if next == '\'' {
                        for _ in 0..=ahead {
                            self.cursor.bump();
                        }
                        break;
                    }
                    ahead += 1;
                }
                self.unexpected(start, '\'');
                Error
            }
            other => {
                self.unexpected(start, other);
                Error
            }
        };
        let end = self.cursor.offset();
        self.push(kind, start, end);
    }

    /// Consumes one of the given follow characters and returns its kind, or returns `single`.
    fn pick(&mut self, options: &[(char, TokenKind)], single: TokenKind) -> TokenKind {
        for (next, kind) in options {
            if self.cursor.eat(*next) {
                return kind.clone();
            }
        }
        single
    }

    fn unexpected(&mut self, start: u32, c: char) {
        let end = self.cursor.offset();
        let span = self.span(start, end);
        let shown = if c.is_ascii_graphic() {
            format!("`{c}`")
        } else {
            format!("U+{:04X}", c as u32)
        };
        let mut diagnostic = Diagnostic::new(codes::E0102, format!("unexpected character {shown}"))
            .primary(span, "this character is not valid here");
        diagnostic = match c {
            ';' => diagnostic.help("there is no `;`: put each statement on its own line"),
            '\'' => diagnostic.help("strings use double quotes: `\"text\"`"),
            c if c.is_whitespace() => {
                diagnostic.help("only spaces, tabs and line ends count as whitespace")
            }
            _ => diagnostic,
        };
        self.error(diagnostic);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Plain,
    Outer,
    Inner,
}

/// Starts an identifier. Non-ASCII letters are accepted here only to report E0111 for the whole
/// word instead of one error per character.
/// Appends text to a list of parts, joining it with a text part at the end.
fn push_text(parts: &mut Vec<FPart>, s: &str) {
    if s.is_empty() {
        return;
    }
    match parts.last_mut() {
        Some(FPart::Text(text)) => text.push_str(s),
        _ => parts.push(FPart::Text(s.to_string())),
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (!c.is_ascii() && c.is_alphabetic())
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || (!c.is_ascii() && c.is_alphanumeric())
}
