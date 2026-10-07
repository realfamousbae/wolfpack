//! Tokens produced by the lexer.

use std::fmt;

use wolf_span::Span;

/// The kind of a token, with the value of literals.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// An identifier. Contextual keywords (`imports`, `as`, `from`, ...) are identifiers too.
    Ident,
    /// An integer literal. The value is at most 2^63 (spec chapter 1, section 5.1).
    Int(u64),
    /// A float literal.
    Float(f64),
    /// A string literal, with escapes already resolved.
    Str(String),
    /// An outer doc comment (`///` or `/** */`) with its text after the marker.
    DocOuter(String),
    /// An inner doc comment (`//!` or `/*! */`) with its text after the marker.
    DocInner(String),
    /// One or more line ends (spec chapter 1, section 7).
    Newline,

    /// `module`
    Module,
    /// `extern`
    Extern,
    /// `impl`
    Impl,
    /// `fn`
    Fn,
    /// `pub`
    Pub,
    /// `let`
    Let,
    /// `var`
    Var,
    /// `type`
    Type,
    /// `struct`
    Struct,
    /// `enum`
    Enum,
    /// `interface`
    Interface,
    /// `if`
    If,
    /// `else`
    Else,
    /// `match`
    Match,
    /// `while`
    While,
    /// `for`
    For,
    /// `in`
    In,
    /// `return`
    Return,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `mut`
    Mut,
    /// `own`
    Own,
    /// `self`
    SelfValue,
    /// `Self`
    SelfType,
    /// `true`
    True,
    /// `false`
    False,
    /// `async` (reserved for the future)
    Async,
    /// `await` (reserved for the future)
    Await,

    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `,`
    Comma,
    /// `.`
    Dot,
    /// `..`
    DotDot,
    /// `..=`
    DotDotEq,
    /// `:`
    Colon,
    /// `?`
    Question,
    /// `_`
    Underscore,
    /// `=`
    Eq,
    /// `->`
    Arrow,
    /// `=>`
    FatArrow,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `&=`
    AmpEq,
    /// `|=`
    PipeEq,
    /// `^=`
    CaretEq,
    /// `<<=`
    ShlEq,
    /// `>>=`
    ShrEq,

    /// Text that could not be lexed. A diagnostic has been reported for it.
    Error,
    /// The end of the file. Always the last token.
    Eof,
}

impl TokenKind {
    /// The keyword for an identifier text, if it is a reserved word (spec chapter 1, section 4).
    pub fn keyword(text: &str) -> Option<TokenKind> {
        Some(match text {
            "module" => TokenKind::Module,
            "extern" => TokenKind::Extern,
            "impl" => TokenKind::Impl,
            "fn" => TokenKind::Fn,
            "pub" => TokenKind::Pub,
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "type" => TokenKind::Type,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "interface" => TokenKind::Interface,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "mut" => TokenKind::Mut,
            "own" => TokenKind::Own,
            "self" => TokenKind::SelfValue,
            "Self" => TokenKind::SelfType,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "async" => TokenKind::Async,
            "await" => TokenKind::Await,
            _ => return None,
        })
    }

    /// The fixed spelling of keywords and punctuation, if the kind has one.
    pub fn spelling(&self) -> Option<&'static str> {
        use TokenKind::*;
        Some(match self {
            Module => "module",
            Extern => "extern",
            Impl => "impl",
            Fn => "fn",
            Pub => "pub",
            Let => "let",
            Var => "var",
            Type => "type",
            Struct => "struct",
            Enum => "enum",
            Interface => "interface",
            If => "if",
            Else => "else",
            Match => "match",
            While => "while",
            For => "for",
            In => "in",
            Return => "return",
            Break => "break",
            Continue => "continue",
            Mut => "mut",
            Own => "own",
            SelfValue => "self",
            SelfType => "Self",
            True => "true",
            False => "false",
            Async => "async",
            Await => "await",
            LParen => "(",
            RParen => ")",
            LBracket => "[",
            RBracket => "]",
            LBrace => "{",
            RBrace => "}",
            Comma => ",",
            Dot => ".",
            DotDot => "..",
            DotDotEq => "..=",
            Colon => ":",
            Question => "?",
            Underscore => "_",
            Eq => "=",
            Arrow => "->",
            FatArrow => "=>",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            EqEq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            AndAnd => "&&",
            OrOr => "||",
            Bang => "!",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Tilde => "~",
            Shl => "<<",
            Shr => ">>",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            AmpEq => "&=",
            PipeEq => "|=",
            CaretEq => "^=",
            ShlEq => "<<=",
            ShrEq => ">>=",
            Ident | Int(_) | Float(_) | Str(_) | DocOuter(_) | DocInner(_) | Newline | Error
            | Eof => return None,
        })
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(spelling) = self.spelling() {
            return write!(f, "`{spelling}`");
        }
        match self {
            TokenKind::Ident => write!(f, "identifier"),
            TokenKind::Int(_) => write!(f, "integer literal"),
            TokenKind::Float(_) => write!(f, "float literal"),
            TokenKind::Str(_) => write!(f, "string literal"),
            TokenKind::DocOuter(_) | TokenKind::DocInner(_) => write!(f, "doc comment"),
            TokenKind::Newline => write!(f, "new line"),
            TokenKind::Error => write!(f, "invalid token"),
            _ => write!(f, "end of file"),
        }
    }
}

/// A token: its kind and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Where the token is in the source.
    pub span: Span,
}

/// An ordinary comment. It has no meaning for the language, but is kept for `wolf fmt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Where the comment is, including its markers.
    pub span: Span,
    /// `true` for `/* */`, `false` for `//`.
    pub block: bool,
}
