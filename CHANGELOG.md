# Changelog

All notable changes are recorded here at the end of each milestone.

## Unreleased

## M2: Parser and skeleton (2026-10-08)

- `wolf_ast`: the syntax tree for spec chapters 2 and 3, with node ids for later phases and a
  span-free text dump.
- `wolf_parser`: recursive descent with a Pratt parser for expressions, the newline rules of
  chapter 1 (innermost delimiter decides, trailing operators and leading `.` continue a line),
  struct-literal restriction in conditions, `>>` splitting in type arguments.
- Skeleton checks E0201–E0212 (order, duplicates, empty sections, misplaced declarations, unknown
  sections, extern targets, inherent impls, test names, doc comment placement) and parser errors
  E0301–E0314.
- Error recovery: a lexer error silences the rest of its line, an error skips to the end of the
  line or the closing delimiter, a section header inside a block closes the open blocks.
- Nesting limit of 256 levels (E0314, new in the spec); the compiler runs on a 64 MiB thread so
  that the limit always fits on the stack.
- `wolf check` now lexes and parses; `--emit ast` prints the tree.
- Tests: AST snapshots of all 16 example files, grammar unit tests, nesting tests for every kind
  of deep input, a never-panic test on generated token soup, and UI tests for every code.

## M1: Foundation (2026-10-08)

- Cargo workspace with lints (`unsafe_code` forbidden; `unwrap`, `expect`, `panic` denied outside
  tests) and CI on GitHub Actions (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`).
- `wolf_span`: source map, byte spans, line and column lookup counting Unicode scalar values.
- `wolf_diag`: the code registry (E0001–E0003, E0101–E0113), diagnostics with labels, help and
  notes, rustc-style text output and JSON lines output (`--message-format json`).
- `wolf_lexer`: hand-written lexer for spec chapter 1 with all lexical errors, error recovery and
  comment tracking.
- `wolf check [paths]`: lexes `.wolf` files and reports lexical errors. Until M3 it takes files or
  directories directly instead of a project.
- Tests: lexer unit tests, a token snapshot, a check that every example lexes cleanly, a
  never-panic test on generated input, UI tests for every code, and a libFuzzer target in `fuzz/`.

## M0: Specification (2026-10-07)

- Language specification in `docs/spec/`, chapters 0 to 9: lexical structure, file skeleton,
  grammar and precedence, type system, ownership, modules and project layout, standard library and
  `extern`, mapping to TypeScript, Go and Rust, diagnostics.
- Decision records `docs/decisions/0001` to `0005`: 64-bit `Int` with `bigint` in TS, hybrid
  `Option`/`Result` representation, insertion-ordered `Map`, payload-less enums as copy types,
  TS classes and one Go package per module with a generated runtime.
- Ten example projects in `examples/` with expected output, covering every milestone criterion.
- Deferred ideas collected in `docs/ideas.md`.
- No compiler code yet.
