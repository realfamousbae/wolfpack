# Wolfpack ideas

Deferred ideas that are not part of the current language scope.
Do not implement anything listed here without an approved spec change.

## Lexical (from chapter 1)

- String interpolation as `f"Hello, {name}"`. The prefix is already reserved (E0113).
- Raw strings `r"..."` and multiline string literals.
- A `Char` type with `'a'` literals. The `'` character is already reserved.
- Numeric type suffixes (`5i32`) together with sized integer types. Suffixes are already reserved.

## Syntax (from chapter 3)

- Default method bodies in interfaces.
- Super-interfaces: `interface Ordered: Comparable { }`.
- `if let` and `while let`.
- Destructuring `let` for irrefutable patterns: `let Point { x, y } = p`.
- Loop labels, `loop`, `break` with a value.
- Struct update syntax: `User { name: "B", ..old }`.
- `impl` blocks for a concrete instantiation of a generic type (`Stack<Int> { }`).

## Standard library (from chapter 7)

- Bounds on individual methods (`fn contains(self, item: T) -> Bool where T: Equatable`), so that
  `items.contains(x)` and `items.sort()` can be methods instead of free functions in `std.list`.
- Full Unicode case mapping (`to_upper`), once the targets can be made to agree.
- A small helper to create an `Error` from a message without declaring a type.
- `assert_eq(a, b)` that prints both values.
