//! Unit tests for every token form and every lexical error of spec chapter 1.

use wolf_lexer::{TokenKind, lex};
use wolf_span::SourceMap;

use TokenKind::*;

/// Lexes `text` and returns the token kinds without the final `Eof`, and the error codes.
fn run(text: &str) -> (Vec<TokenKind>, Vec<&'static str>) {
    let mut map = SourceMap::new();
    let id = map.add("test.wolf", text.to_string());
    let lexed = lex(id, text);
    let mut kinds: Vec<TokenKind> = lexed.tokens.into_iter().map(|t| t.kind).collect();
    assert_eq!(kinds.pop(), Some(Eof));
    let codes = lexed.diagnostics.iter().map(|d| d.code.id()).collect();
    (kinds, codes)
}

fn kinds(text: &str) -> Vec<TokenKind> {
    let (kinds, codes) = run(text);
    assert!(codes.is_empty(), "unexpected errors {codes:?} in {text:?}");
    kinds
}

fn errors(text: &str) -> Vec<&'static str> {
    run(text).1
}

// ----- tokens ----------------------------------------------------------------------------------

#[test]
fn keywords_and_identifiers() {
    assert_eq!(
        kinds("module fn let var self Self imports config as _ _x"),
        [
            Module, Fn, Let, Var, SelfValue, SelfType, Ident, Ident, Ident, Underscore, Ident
        ]
    );
    assert_eq!(kinds("async await"), [Async, Await]);
}

#[test]
fn punctuation_takes_the_longest_match() {
    assert_eq!(
        kinds("-> => == != <= >= && || << >> <<= >>= .. ..= ."),
        [
            Arrow, FatArrow, EqEq, NotEq, Le, Ge, AndAnd, OrOr, Shl, Shr, ShlEq, ShrEq, DotDot,
            DotDotEq, Dot
        ]
    );
    assert_eq!(
        kinds("+= -= *= /= %= &= |= ^= & | ^ ~ ! ? : , ( ) [ ] { }"),
        [
            PlusEq, MinusEq, StarEq, SlashEq, PercentEq, AmpEq, PipeEq, CaretEq, Amp, Pipe, Caret,
            Tilde, Bang, Question, Colon, Comma, LParen, RParen, LBracket, RBracket, LBrace,
            RBrace
        ]
    );
    assert_eq!(kinds("- >"), [Minus, Gt]);
}

#[test]
fn integers() {
    assert_eq!(kinds("0 7 1_000_000"), [Int(0), Int(7), Int(1_000_000)]);
    assert_eq!(
        kinds("0xff 0o17 0b101 0x1e5"),
        [Int(255), Int(15), Int(5), Int(0x1e5)]
    );
    assert_eq!(kinds("9223372036854775808"), [Int(1 << 63)]);
    assert_eq!(kinds("-5"), [Minus, Int(5)]);
}

#[test]
fn floats() {
    assert_eq!(
        kinds("1.5 0.25 1_0.0_1"),
        [Float(1.5), Float(0.25), Float(10.01)]
    );
    assert_eq!(
        kinds("1e10 2E-3 1.5e+2"),
        [Float(1e10), Float(2e-3), Float(150.0)]
    );
}

#[test]
fn dots_after_numbers() {
    assert_eq!(kinds("1..5"), [Int(1), DotDot, Int(5)]);
    assert_eq!(kinds("0..=n"), [Int(0), DotDotEq, Ident]);
    assert_eq!(kinds("1.copy()"), [Int(1), Dot, Ident, LParen, RParen]);
    assert_eq!(kinds("1.e5"), [Int(1), Dot, Ident]);
}

#[test]
fn strings_and_escapes() {
    assert_eq!(kinds(r#""""#), [Str(String::new())]);
    assert_eq!(
        kinds(r#""a\n\r\t\0\\\"b""#),
        [Str("a\n\r\t\0\\\"b".to_string())]
    );
    assert_eq!(kinds(r#""\x41\u{1F600}\u{e9}""#), [Str("A😀é".to_string())]);
    assert_eq!(kinds("\"tab\there\""), [Str("tab\there".to_string())]);
    assert_eq!(kinds("\"привет {}\""), [Str("привет {}".to_string())]);
}

#[test]
fn comments() {
    assert_eq!(kinds("// plain\nx"), [Ident]);
    assert_eq!(kinds("//// four slashes\nx"), [Ident]);
    assert_eq!(kinds("/**/ x /*** y */ z"), [Ident, Ident]);
    assert_eq!(kinds("/* a /* nested */ b */ x"), [Ident]);
    assert_eq!(
        kinds("/// outer\n//! inner\nx"),
        [
            DocOuter(" outer".into()),
            Newline,
            DocInner(" inner".into()),
            Newline,
            Ident
        ]
    );
    assert_eq!(
        kinds("/** block */ /*! inner */"),
        [DocOuter(" block ".into()), DocInner(" inner ".into())]
    );
    let mut map = SourceMap::new();
    let id = map.add("t", String::new());
    let lexed = lex(id, "a // c\n/* d */ b");
    assert_eq!(lexed.comments.len(), 2);
    assert!(!lexed.comments[0].block);
    assert!(lexed.comments[1].block);
}

#[test]
fn newlines_collapse() {
    assert_eq!(
        kinds("\n\na\n\n// c\n\nb\n"),
        [Ident, Newline, Ident, Newline]
    );
    assert_eq!(kinds("a\r\nb"), [Ident, Newline, Ident]);
    // A block comment with a line end counts as a line end, one without as a space.
    assert_eq!(kinds("a /* x\n */ b"), [Ident, Newline, Ident]);
    assert_eq!(kinds("a /* x */ b"), [Ident, Ident]);
}

#[test]
fn byte_order_mark_only_at_start() {
    assert_eq!(kinds("\u{feff}x"), [Ident]);
    assert_eq!(errors("x \u{feff}"), ["E0102"]);
}

#[test]
fn spans_are_byte_ranges() {
    let mut map = SourceMap::new();
    let id = map.add("t", String::new());
    let lexed = lex(id, "ab \"é\"");
    let spans: Vec<_> = lexed
        .tokens
        .iter()
        .map(|t| (t.span.start, t.span.end))
        .collect();
    assert_eq!(spans, [(0, 2), (3, 7), (7, 7)]);
}

// ----- errors ----------------------------------------------------------------------------------

#[test]
fn e0102_unexpected_character() {
    assert_eq!(errors("@"), ["E0102"]);
    assert_eq!(errors("a; b"), ["E0102"]);
    assert_eq!(errors("'a'"), ["E0102"]);
    assert_eq!(errors("'a"), ["E0102"]);
    assert_eq!(errors("a\u{a0}b"), ["E0102"]);
    assert_eq!(errors("\u{7}"), ["E0102"]);
}

#[test]
fn e0103_unterminated_string() {
    assert_eq!(errors("\"abc"), ["E0103"]);
    assert_eq!(errors("\"abc\nx"), ["E0103"]);
    assert_eq!(errors("\"abc\\"), ["E0103"]);
    // The next line is lexed normally.
    let (kinds, _) = run("\"abc\nx");
    assert_eq!(kinds, [Str("abc".into()), Newline, Ident]);
}

#[test]
fn e0104_unknown_escape() {
    assert_eq!(errors(r#""\q""#), ["E0104"]);
    assert_eq!(errors(r#""\'""#), ["E0104"]);
}

#[test]
fn e0105_invalid_escape() {
    for text in [
        r#""\u{D800}""#,
        r#""\u{}""#,
        r#""\u{110000}""#,
        r#""\u{1234567}""#,
        r#""\u41""#,
        r#""\u{41""#,
        r#""\x80""#,
        r#""\x4""#,
    ] {
        assert_eq!(errors(text), ["E0105"], "{text}");
    }
}

#[test]
fn e0106_integer_too_large() {
    assert_eq!(errors("9223372036854775809"), ["E0106"]);
    assert_eq!(
        errors("99999999999999999999999999999999999999999"),
        ["E0106"]
    );
    assert_eq!(errors("0xFFFF_FFFF_FFFF_FFFF"), ["E0106"]);
}

#[test]
fn e0107_malformed_numbers() {
    for text in [
        "007", "00", "1_", "1__0", "0x", "0x_1", "0X1", "1.", "1. ", ".5", "1e", "1e+", "5i32",
        "1.0f", "0b102", "0xfg", "1.5_",
    ] {
        assert_eq!(errors(text), ["E0107"], "{text}");
    }
}

#[test]
fn e0108_float_out_of_range() {
    assert_eq!(errors("1.0e999"), ["E0108"]);
    assert!(errors("1.0e-999").is_empty());
}

#[test]
fn e0109_control_character_in_string() {
    assert_eq!(errors("\"a\u{7}b\""), ["E0109"]);
    assert_eq!(errors("\"a\u{7f}b\""), ["E0109"]);
}

#[test]
fn e0110_lone_carriage_return() {
    assert_eq!(errors("a\rb"), ["E0110"]);
    assert_eq!(errors("// x\ry\n"), ["E0110"]);
    assert_eq!(errors("\"a\rb\""), ["E0110"]);
}

#[test]
fn e0111_non_ascii_identifier() {
    assert_eq!(errors("naïve"), ["E0111"]);
    assert_eq!(errors("привет"), ["E0111"]);
}

#[test]
fn e0112_unterminated_block_comment() {
    assert_eq!(errors("/* a"), ["E0112"]);
    assert_eq!(errors("/* a /* b */"), ["E0112"]);
}

#[test]
fn e0113_reserved_prefix() {
    assert_eq!(errors(r#"f"x""#), ["E0113"]);
    assert_eq!(errors(r#"r"x""#), ["E0113"]);
    assert!(errors(r#"f "x""#).is_empty());
}

#[test]
fn all_errors_are_reported_in_order() {
    let mut map = SourceMap::new();
    let text = "@ 007 \"\\q\" naïve\n#";
    let id = map.add("t", text.to_string());
    let lexed = lex(id, text);
    let codes: Vec<_> = lexed.diagnostics.iter().map(|d| d.code.id()).collect();
    assert_eq!(codes, ["E0102", "E0107", "E0104", "E0111", "E0102"]);
    for d in &lexed.diagnostics {
        assert!(d.primary_span().is_some());
    }
}

#[test]
fn invalid_utf8_is_detected() {
    let error = wolf_lexer::decode(b"ab\xffcd".to_vec()).unwrap_err();
    assert_eq!(error.offset, 2);
    assert_eq!(error.lossy, "ab\u{fffd}cd");
    let mut map = SourceMap::new();
    let id = map.add("t", error.lossy.clone());
    let d = error.diagnostic(id);
    assert_eq!(d.code.id(), "E0101");
    assert_eq!(d.primary_span().map(|s| (s.start, s.end)), Some((2, 5)));
    assert_eq!(
        wolf_lexer::decode(b"ok".to_vec()).ok().as_deref(),
        Some("ok")
    );
}
