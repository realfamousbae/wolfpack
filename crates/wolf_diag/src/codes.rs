//! The list of all diagnostic codes.
//!
//! This is the only place where codes are defined (spec chapter 9). A code is never reused: when an
//! error is removed, its code moves to [`RETIRED`]. Every code listed in [`ALL`] has a UI test.

use crate::Code;

/// Cannot read a file or directory.
pub const E0001: Code = Code::new("E0001", "cannot read a file or directory");
/// Invalid command-line usage of `wolf`.
pub const E0002: Code = Code::new("E0002", "invalid command-line usage");
/// Internal compiler error.
pub const E0003: Code = Code::new("E0003", "internal compiler error");

/// The file is not valid UTF-8.
pub const E0101: Code = Code::new("E0101", "file is not valid UTF-8");
/// Unexpected character.
pub const E0102: Code = Code::new("E0102", "unexpected character");
/// Unterminated string literal.
pub const E0103: Code = Code::new("E0103", "unterminated string literal");
/// Unknown escape sequence.
pub const E0104: Code = Code::new("E0104", "unknown escape sequence");
/// Invalid Unicode or ASCII escape.
pub const E0105: Code = Code::new("E0105", "invalid character escape");
/// Integer literal too large.
pub const E0106: Code = Code::new("E0106", "integer literal too large");
/// Malformed number literal.
pub const E0107: Code = Code::new("E0107", "malformed number literal");
/// Float literal out of range.
pub const E0108: Code = Code::new("E0108", "float literal out of range");
/// Unescaped control character in a string.
pub const E0109: Code = Code::new("E0109", "unescaped control character in a string");
/// Carriage return without a line feed.
pub const E0110: Code = Code::new("E0110", "carriage return without line feed");
/// Non-ASCII character in an identifier.
pub const E0111: Code = Code::new("E0111", "non-ASCII character in an identifier");
/// Unterminated block comment.
pub const E0112: Code = Code::new("E0112", "unterminated block comment");
/// Reserved literal prefix.
pub const E0113: Code = Code::new("E0113", "reserved literal prefix");

/// The file does not start with `module`.
pub const E0201: Code = Code::new("E0201", "the file does not start with `module`");
/// Section out of order.
pub const E0202: Code = Code::new("E0202", "section out of order");
/// Duplicate section.
pub const E0203: Code = Code::new("E0203", "duplicate section");
/// Empty section or extern block.
pub const E0204: Code = Code::new("E0204", "empty section or extern block");
/// Declaration in the wrong section.
pub const E0205: Code = Code::new("E0205", "declaration in the wrong section");
/// Unknown section name.
pub const E0206: Code = Code::new("E0206", "unknown section name");
/// Declaration outside any section.
pub const E0207: Code = Code::new("E0207", "declaration outside any section");
/// Malformed `module` line.
pub const E0208: Code = Code::new("E0208", "malformed `module` line");
/// Unknown or duplicate extern target block.
pub const E0209: Code = Code::new("E0209", "unknown or duplicate extern target block");
/// Second inherent `impl` block for a type.
pub const E0210: Code = Code::new("E0210", "second inherent `impl` block for a type");
/// Duplicate test name.
pub const E0211: Code = Code::new("E0211", "duplicate test name");
/// Doc comment in a position where it documents nothing.
pub const E0212: Code = Code::new(
    "E0212",
    "doc comment in a position where it documents nothing",
);
/// Unexpected token.
pub const E0301: Code = Code::new("E0301", "unexpected token");
/// Unclosed delimiter.
pub const E0302: Code = Code::new("E0302", "unclosed delimiter");
/// Binary operator at the start of a line.
pub const E0303: Code = Code::new("E0303", "binary operator at the start of a line");
/// `else` on a new line.
pub const E0304: Code = Code::new("E0304", "`else` on a new line");
/// Chained comparison.
pub const E0305: Code = Code::new("E0305", "chained comparison");
/// Struct literal in a condition without parentheses.
pub const E0306: Code = Code::new("E0306", "struct literal in a condition without parentheses");
/// Range outside `for` or index brackets.
pub const E0307: Code = Code::new("E0307", "range outside `for` or index brackets");
/// Assignment used as an expression or to a non-place.
pub const E0308: Code = Code::new(
    "E0308",
    "assignment used as an expression or to a non-place",
);
/// Misplaced `self` parameter.
pub const E0309: Code = Code::new("E0309", "misplaced `self` parameter");
/// Closure with a result type but without a block body.
pub const E0310: Code = Code::new(
    "E0310",
    "closure with a result type but without a block body",
);
/// Type arguments without a dot in an expression.
pub const E0311: Code = Code::new("E0311", "type arguments without a dot in an expression");
/// Float literal in a pattern.
pub const E0312: Code = Code::new("E0312", "float literal in a pattern");
/// Two statements on one line.
pub const E0313: Code = Code::new("E0313", "two statements on one line");
/// Nesting too deep.
pub const E0314: Code = Code::new("E0314", "nesting too deep");

/// Every code in use, in order.
pub const ALL: &[Code] = &[
    E0001, E0002, E0003, E0101, E0102, E0103, E0104, E0105, E0106, E0107, E0108, E0109, E0110,
    E0111, E0112, E0113, E0201, E0202, E0203, E0204, E0205, E0206, E0207, E0208, E0209, E0210,
    E0211, E0212, E0301, E0302, E0303, E0304, E0305, E0306, E0307, E0308, E0309, E0310, E0311,
    E0312, E0313, E0314,
];

/// Codes that were used once and must never be used again.
pub const RETIRED: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_sorted() {
        for pair in ALL.windows(2) {
            assert!(
                pair[0].id() < pair[1].id(),
                "{} before {}",
                pair[0].id(),
                pair[1].id()
            );
        }
        for code in ALL {
            assert!(!RETIRED.contains(&code.id()));
            assert_eq!(code.id().len(), 5);
            assert!(code.id().starts_with('E'));
        }
    }
}
