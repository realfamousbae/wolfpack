# Changelog

All notable changes are recorded here at the end of each milestone.

## Unreleased

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
