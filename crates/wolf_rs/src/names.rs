//! Names in Rust (spec chapter 8, section 3).

/// Keywords of Rust 2024, strict and reserved. Such a name is written as a raw identifier.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

/// Keywords that cannot be raw identifiers; such a name gets a `_` suffix.
const NOT_RAW: &[&str] = &["self", "Self", "super", "crate", "_"];

/// Names of the Rust prelude that generated code uses; a type of the program with such a name
/// gets a `_` suffix so that it does not hide them.
const PRELUDE: &[&str] = &[
    "Box",
    "Vec",
    "String",
    "Option",
    "Some",
    "None",
    "Result",
    "Ok",
    "Err",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Fn",
    "FnMut",
    "FnOnce",
    "Iterator",
    "IntoIterator",
    "Default",
    "Drop",
    "Send",
    "Sync",
    "Sized",
    "ToString",
    "From",
    "Into",
    "Hash",
    "Debug",
];

/// A name of a value: a function, variable, field, method or module.
pub fn value(name: &str) -> String {
    if NOT_RAW.contains(&name) {
        format!("{name}_")
    } else if KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

/// A name of a type, variant or trait.
pub fn type_name(name: &str) -> String {
    if PRELUDE.contains(&name) || NOT_RAW.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// `max_users` to `MAX_USERS`.
pub fn constant(name: &str) -> String {
    value(&name.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_are_raw_identifiers() {
        assert_eq!(value("type"), "r#type");
        assert_eq!(value("find_user"), "find_user");
        assert_eq!(value("self"), "self_");
        assert_eq!(type_name("Box"), "Box_");
        assert_eq!(constant("max_users"), "MAX_USERS");
    }
}
