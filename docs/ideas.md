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
- Variants with a payload as function values: `names.map(Some)` (declined for v1, 2026-10-09).
- Struct update syntax: `User { name: "B", ..old }`.
- `impl` blocks for a concrete instantiation of a generic type (`Stack<Int> { }`).

## Standard library (from chapter 7)

- Bounds on individual methods (`fn contains(self, item: T) -> Bool where T: Equatable`), so that
  `items.contains(x)` and `items.sort()` can be methods instead of free functions in `std.list`.
- Full Unicode case mapping (`to_upper`), once the targets can be made to agree.
- A small helper to create an `Error` from a message without declaring a type.
- `assert_eq(a, b)` that prints both values.
- A debug build flag (`wolf build --debug`) that adds `.wolf` source locations to built-in panics
  (overflow, index out of range).
- A `Bytes` type for byte arrays, so that hashing, encodings and binary files work, and so that
  bytes can cross `extern` (see `docs/plans/native-code.md`, section 3.4).

## Target code (from 2026-10-17)

The full plan is in `docs/plans/native-code.md`.

- Native sources: `.ts`, `.go` and `.rs` files of the project in `native/<target>/`, called through
  `extern` with a `native:` path.
- `Map`, callbacks and records at the `extern` boundary.
- `wolf bind`: `extern` declarations generated from `.d.ts` files, Go packages and Rust sources;
  `wolf check --extern` compares existing declarations with the target code.

## Distribution (from 2026-10-17)

The full plan is in `docs/plans/distribution.md`.

- Package managers: a Homebrew tap, winget, Scoop, and later Debian/RPM packages and the AUR.
- Signed installers: a notarized macOS package and a signed Windows installer.
- `wolf self update`, which replaces the running `wolf` with the latest release.
