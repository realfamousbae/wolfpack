# Wolfpack ideas

Deferred ideas that are not part of the current language scope.
Do not implement anything listed here without an approved spec change.

## Lexical (from chapter 1)

- A `Char` type with `'a'` literals. The `'` character is already reserved.
- Numeric type suffixes (`5i32`) together with sized integer types. Suffixes are already reserved.

## Syntax (from chapter 3)

- `break label value`: leaving a labelled outer `loop` with a value (0.2 has `break label` and
  `break value` separately).
- Variants with a payload as function values: `names.map(Some)` (declined for v1, 2026-10-09).
- The update syntax `..base` for enum variants with named fields (E0434 in 0.2).
- `impl` blocks for a concrete instantiation of a generic type (`Stack<Int> { }`).

## Standard library (from chapter 7)

- Full Unicode case mapping (`to_upper`), once the targets can be made to agree.
- A debug build flag (`wolf build --debug`) that adds `.wolf` source locations to built-in panics
  (overflow, index out of range).
- A `Bytes` type for byte arrays, so that hashing, encodings and binary files work, and so that
  bytes can cross `extern` (see `docs/plans/native-code.md`, section 3.4).

## Target code (from 2026-10-17)

The full plan is in `docs/plans/native-code.md`. Native sources, `Map`, callbacks and records at
the boundary, and native modules (declarations read from the project's own target code) are done
in 0.2.

- Packages of npm, Go and crates.io read like native modules, without `extern` lines (section 4.9
  of the plan): the readers need the fetched packages (`node_modules/**/*.d.ts`, the Go module
  cache, rustdoc JSON).
- `wolf check --extern`: compare written `extern` declarations with the target code.

## Distribution (from 2026-10-17)

The full plan is in `docs/plans/distribution.md`. `wolf self update` is done in 0.2; Homebrew,
Scoop, winget, `.deb`/`.rpm` packages and static Linux builds after 0.3.

- The AUR (`wolfpack-bin`).
- Signed installers: a notarized macOS package and a signed Windows installer.

## The virtual machine (from 2026-10-22, chapter 10, D10.5 and D10.7)

