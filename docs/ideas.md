# Wolfpack ideas

Deferred ideas that are not part of the current language scope.
Do not implement anything listed here without an approved spec change.

## Lexical (from chapter 1)

- String interpolation as `f"Hello, {name}"`. The prefix is already reserved (E0113).
- Raw strings `r"..."` and multiline string literals.
- A `Char` type with `'a'` literals. The `'` character is already reserved.
- Numeric type suffixes (`5i32`) together with sized integer types. Suffixes are already reserved.
