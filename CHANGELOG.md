# Changelog

All notable changes are recorded here at the end of each milestone.

## Unreleased

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
