//! Names in TypeScript (spec chapter 8, section 3).

/// Words that cannot be used as names in TypeScript, and names predeclared by the target
/// (`ES2022` globals). Such a name gets a `_` suffix.
const RESERVED: &[&str] = &[
    // Keywords and reserved words.
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "await",
    "arguments",
    "eval",
    "constructor",
    "undefined",
    "NaN",
    "Infinity",
    // Globals of ES2022 and Node.js.
    "Array",
    "ArrayBuffer",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float32Array",
    "Float64Array",
    "Function",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "globalThis",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "console",
    "process",
    "require",
    "module",
    "exports",
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
    safe(out)
}

/// A name that is safe in TypeScript: reserved names get a `_` suffix.
pub fn safe(name: String) -> String {
    if RESERVED.contains(&name.as_str()) {
        format!("{name}_")
    } else {
        name
    }
}

/// A type name, which keeps its `PascalCase` spelling.
pub fn type_name(name: &str) -> String {
    safe(name.to_string())
}
