//! Names in Go (spec chapter 8, section 3).

/// Keywords and predeclared names of Go, and names with a special meaning in a package. Such a
/// name gets a `_` suffix.
const RESERVED: &[&str] = &[
    // Keywords.
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    // Predeclared names.
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    "true",
    "false",
    "iota",
    "nil",
    "append",
    "cap",
    "clear",
    "close",
    "complex",
    "copy",
    "delete",
    "imag",
    "len",
    "make",
    "max",
    "min",
    "new",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    // Functions with a special meaning.
    "main",
    "init",
];

/// `find_user` to `findUser`; an `_` before a digit is kept (`a_b_1` to `aB_1`).
pub fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            match chars.peek() {
                Some(next) if next.is_ascii_alphabetic() && !out.is_empty() => {
                    let next = *next;
                    chars.next();
                    out.push(next.to_ascii_uppercase());
                }
                _ => out.push('_'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A name that is safe in Go: reserved names get a `_` suffix.
pub fn safe(name: String) -> String {
    if RESERVED.contains(&name.as_str()) {
        format!("{name}_")
    } else {
        name
    }
}

/// The first letter upper-cased (exported) or lower-cased.
pub fn case(name: &str, exported: bool) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let first = if exported {
        first.to_ascii_uppercase()
    } else {
        first.to_ascii_lowercase()
    };
    safe(std::iter::once(first).chain(chars).collect())
}

/// A function, variable, field or method name: `FindUser` if exported, else `findUser`.
pub fn value_name(name: &str, exported: bool) -> String {
    case(&camel(name), exported)
}

/// A type, variant or interface name: `FileStorage` if exported, else `fileStorage`.
pub fn type_name(name: &str, exported: bool) -> String {
    case(name, exported)
}

/// A local variable or parameter.
pub fn local(name: &str) -> String {
    safe(camel(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_visibility() {
        assert_eq!(value_name("find_user", true), "FindUser");
        assert_eq!(value_name("find_user", false), "findUser");
        assert_eq!(type_name("FileStorage", false), "fileStorage");
        assert_eq!(value_name("a_b_1", false), "aB_1");
    }

    #[test]
    fn reserved_names_get_a_suffix() {
        assert_eq!(local("len"), "len_");
        assert_eq!(local("type"), "type_");
        assert_eq!(value_name("main", false), "main_");
        assert_eq!(type_name("Error", false), "error_");
        assert_eq!(type_name("Error", true), "Error");
    }
}
