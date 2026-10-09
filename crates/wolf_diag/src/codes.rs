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

/// No `wolfpack.toml` in the current directory or its parents.
pub const E0004: Code = Code::new("E0004", "no `wolfpack.toml` found");

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
/// `fn main` is not the last function.
pub const E0213: Code = Code::new("E0213", "`main` is not the last function");
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

/// Invalid manifest.
pub const E0611: Code = Code::new("E0611", "invalid manifest");
/// Invalid project layout.
pub const E0612: Code = Code::new("E0612", "invalid project layout");
/// Invalid config override.
pub const E0613: Code = Code::new("E0613", "invalid config override");

/// Type mismatch.
pub const E0401: Code = Code::new("E0401", "type mismatch");
/// Unknown type name.
pub const E0402: Code = Code::new("E0402", "unknown type name");
/// Wrong number of type arguments.
pub const E0403: Code = Code::new("E0403", "wrong number of type arguments");
/// A type does not implement an interface required by a bound.
pub const E0404: Code = Code::new("E0404", "bound not satisfied");
/// Operator not defined for these types.
pub const E0405: Code = Code::new("E0405", "operator not defined for these types");
/// Type is not `Equatable` or not `Copyable`.
pub const E0406: Code = Code::new("E0406", "type is not `Equatable` or not `Copyable`");
/// Map key type is not `Hashable`.
pub const E0407: Code = Code::new("E0407", "map key type is not `Hashable`");
/// `?` in a function with an incompatible result type.
pub const E0408: Code = Code::new(
    "E0408",
    "`?` in a function with an incompatible result type",
);
/// A `Result` value is discarded.
pub const E0409: Code = Code::new("E0409", "unhandled `Result` value");
/// Non-exhaustive `match`.
pub const E0410: Code = Code::new("E0410", "non-exhaustive `match`");
/// Unreachable `match` arm.
pub const E0411: Code = Code::new("E0411", "unreachable `match` arm");
/// Alternatives in a pattern bind different names or types.
pub const E0412: Code = Code::new("E0412", "alternatives bind different names or types");
/// Missing, extra or mismatched method in an interface implementation.
pub const E0413: Code = Code::new("E0413", "wrong methods in an interface implementation");
/// `impl` block outside the module of its type.
pub const E0414: Code = Code::new("E0414", "`impl` outside the module of its type");
/// Two functions with the same name on one type.
pub const E0415: Code = Code::new("E0415", "two functions with the same name on one type");
/// Interface that is not object-safe used as a type.
pub const E0416: Code = Code::new("E0416", "interface is not object-safe");
/// Generic method in an interface, or an interface member without `self`.
pub const E0417: Code = Code::new("E0417", "invalid interface member");
/// Struct that contains itself.
pub const E0418: Code = Code::new("E0418", "infinitely sized struct");
/// Cycle in type aliases or in `config` constants.
pub const E0419: Code = Code::new("E0419", "cycle in type aliases or constants");
/// Type cannot be inferred.
pub const E0420: Code = Code::new("E0420", "type cannot be inferred");
/// Function may end without a value.
pub const E0421: Code = Code::new("E0421", "function may end without a value");
/// Struct literal or pattern with a missing, unknown or duplicate field.
pub const E0422: Code = Code::new("E0422", "wrong fields in a struct literal or pattern");
/// Integer literal out of range, or a panic while evaluating a constant.
pub const E0423: Code = Code::new("E0423", "integer out of range or constant evaluation panic");
/// Unknown field or method.
pub const E0424: Code = Code::new("E0424", "unknown field or method");
/// Expression statement whose value is not used.
pub const E0425: Code = Code::new("E0425", "expression value is not used");
/// `config` initializer that is not a constant expression.
pub const E0426: Code = Code::new("E0426", "not a constant expression");
/// `config` constant whose type is not `Int`, `Float`, `Bool` or `String`.
pub const E0427: Code = Code::new("E0427", "constant of a type that is not allowed");
/// Wrong number of arguments, or a variant written in the wrong form.
pub const E0428: Code = Code::new("E0428", "wrong number of arguments or wrong variant form");
/// A type or a module used as a value.
pub const E0429: Code = Code::new("E0429", "type or module used as a value");
/// `break` or `continue` outside a loop.
pub const E0430: Code = Code::new("E0430", "`break` or `continue` outside a loop");

/// Implementation that cannot be written.
pub const E0431: Code = Code::new("E0431", "implementation that cannot be written");
/// Use of a moved value.
pub const E0501: Code = Code::new("E0501", "use of a moved value");
/// Use of a value that may have been moved.
pub const E0502: Code = Code::new("E0502", "use of a value that may have been moved");
/// Move out of a field or a list element.
pub const E0503: Code = Code::new("E0503", "move out of a field or an element");
/// Move out of a borrowed value.
pub const E0504: Code = Code::new("E0504", "move out of a borrowed value");
/// Change of an immutable binding.
pub const E0505: Code = Code::new("E0505", "change of an immutable binding");
/// `mut` argument or `mut self` receiver that is not a mutable place.
pub const E0506: Code = Code::new("E0506", "not a mutable place");
/// Missing or unexpected `mut` or `own` marker at the call site.
pub const E0507: Code = Code::new("E0507", "missing or unexpected `mut` or `own` marker");
/// Overlapping places in one call while one of them is changed or moved.
pub const E0508: Code = Code::new("E0508", "overlapping places in one call");
/// Change or move of a place that a `for` loop or a `match` arm borrows.
pub const E0509: Code = Code::new("E0509", "access to a borrowed place");
/// Capture of a borrowed value by a closure.
pub const E0510: Code = Code::new("E0510", "capture of a borrowed value");
/// Change of a captured value inside a closure.
pub const E0511: Code = Code::new("E0511", "change of a captured value");

/// Name does not follow the naming style.
pub const E0601: Code = Code::new("E0601", "name does not follow the naming style");

/// Unknown module in an import.
pub const E0602: Code = Code::new("E0602", "unknown module in an import");

/// Imported name does not exist or is not public.
pub const E0603: Code = Code::new("E0603", "imported name does not exist or is not public");

/// Import cycle.
pub const E0604: Code = Code::new("E0604", "import cycle");

/// Name declared or imported twice in a module.
pub const E0605: Code = Code::new("E0605", "name declared or imported twice in a module");

/// Name declared twice in the same block.
pub const E0606: Code = Code::new("E0606", "name declared twice in the same block");

/// Private item used outside its module.
pub const E0607: Code = Code::new("E0607", "private item used outside its module");

/// Type less visible than the declaration using it.
pub const E0608: Code = Code::new("E0608", "type less visible than the declaration using it");

/// Unknown name.
pub const E0609: Code = Code::new("E0609", "unknown name");

/// `module` line does not match the file path.
pub const E0610: Code = Code::new("E0610", "`module` line does not match the file path");

/// Missing `main` or wrong signature.
pub const E0614: Code = Code::new("E0614", "missing `main` or wrong signature");

/// Unused import.
pub const E0615: Code = Code::new("E0615", "unused import");

/// Import of an entry module or of the module itself.
pub const E0616: Code = Code::new("E0616", "import of an entry module or of the module itself");

/// Redeclaration of a prelude name.
pub const E0617: Code = Code::new("E0617", "redeclaration of a prelude name");

/// Unused local variable.
pub const E0618: Code = Code::new("E0618", "unused local variable");

/// Section not allowed in a module of `tests/`.
pub const E0619: Code = Code::new("E0619", "section not allowed in a module of `tests/`");

/// `pub` import outside `src/lib.wolf`.
pub const E0620: Code = Code::new("E0620", "`pub` import outside `src/lib.wolf`");

/// `intrinsic` outside std.
pub const E0701: Code = Code::new("E0701", "`intrinsic` outside `std`");

/// A tool of the target is not installed.
pub const E0801: Code = Code::new("E0801", "target tool not installed");
/// A tool of the target rejected the generated code.
pub const E0802: Code = Code::new("E0802", "target tool rejected the generated code");

/// Every code in use, in order.
pub const ALL: &[Code] = &[
    E0001, E0002, E0003, E0004, E0101, E0102, E0103, E0104, E0105, E0106, E0107, E0108, E0109,
    E0110, E0111, E0112, E0113, E0201, E0202, E0203, E0204, E0205, E0206, E0207, E0208, E0209,
    E0210, E0211, E0212, E0213, E0301, E0302, E0303, E0304, E0305, E0306, E0307, E0308, E0309,
    E0310, E0311, E0312, E0313, E0314, E0401, E0402, E0403, E0404, E0405, E0406, E0407, E0408,
    E0409, E0410, E0411, E0412, E0413, E0414, E0415, E0416, E0417, E0418, E0419, E0420, E0421,
    E0422, E0423, E0424, E0425, E0426, E0427, E0428, E0429, E0430, E0431, E0501, E0502, E0503,
    E0504, E0505, E0506, E0507, E0508, E0509, E0510, E0511, E0601, E0602, E0603, E0604, E0605,
    E0606, E0607, E0608, E0609, E0610, E0611, E0612, E0613, E0614, E0615, E0616, E0617, E0618,
    E0619, E0620, E0701, E0801, E0802,
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
