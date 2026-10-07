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

/// Every code in use, in order.
pub const ALL: &[Code] = &[
    E0001, E0002, E0003, E0101, E0102, E0103, E0104, E0105, E0106, E0107, E0108, E0109, E0110,
    E0111, E0112, E0113,
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
