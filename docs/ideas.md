# Wolfpack ideas

Deferred ideas that are not part of the current language scope.
Do not implement anything listed here without an approved spec change.

## Lexical (from chapter 1)

- Format specifications in interpolated strings (`f"{x:>5}"`, `f"{price:.2}"`); for now widths and
  precision are std functions.
- A `Char` type with `'a'` literals. The `'` character is already reserved.
- Numeric type suffixes (`5i32`) together with sized integer types. Suffixes are already reserved.

## Syntax (from chapter 3)

- Default method bodies in interfaces.
- Super-interfaces: `interface Ordered: Comparable { }`.
- Chains in conditions: `if let Some(x) = a && x > 0` (taken in 0.2 without chains).
- `break label value`: leaving a labelled outer `loop` with a value (0.2 has `break label` and
  `break value` separately).
- Variants with a payload as function values: `names.map(Some)` (declined for v1, 2026-10-09).
- The update syntax `..base` for enum variants with named fields (E0434 in 0.2).
- `impl` blocks for a concrete instantiation of a generic type (`Stack<Int> { }`).

## Standard library (from chapter 7)

- Bounds on individual methods (`fn contains(self, item: T) -> Bool where T: Equatable`), so that
  `items.contains(x)` and `items.sort()` can be methods instead of free functions in `std.list`.
- Full Unicode case mapping (`to_upper`), once the targets can be made to agree.
- `Display` for `List`, `Option` and `Map` when their elements are `Display`, so that `f"{items}"`
  works.
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
