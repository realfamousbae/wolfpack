# Changelog

All notable changes are recorded here at the end of each milestone.

## Unreleased

## M7: Tests (2026-10-09)

- `wolf test [text]` builds the project with its tests (`tests` sections, their helper functions
  and the modules under `tests/`), runs them with `node:test`, and prints one report for every
  target through a generated reporter: one line per test, the failures with their panic messages,
  and a summary; exit code 0 or 1 (spec chapter 8, rule 2.7, new). `npm test` works in the
  generated project.
- Tests and helpers are never part of `wolf build`; in a test build `main` is not run.
- A failed `assert` is a panic at the location of the call:
  `panic at src/main.wolf:19:9: assertion failed: halve(4) == 3`.
- Tests: the tests of all examples pass; a project with passing and failing tests (assertions,
  panics, overflow, helpers, a nested module) checks the report, also with a filter.

## M6: IR and the TypeScript backend (2026-10-09)

- `wolf_ir`: the checked program lowered to a target-neutral IR: resolved calls and methods,
  explicit interface coercions and `?` conversions, blocks that discard their final value,
  patterns in declaration order, evaluated constants, `panic` with its source location and
  `assert` with its source text.
- `wolf_ts`: the TypeScript backend builds a tree and prints it. Structs and payload variants are
  classes, payload-less enums are string unions, `Int` is checked `bigint` arithmetic, `Option`
  uses the hybrid form of ADR 0002, `mut` parameters of value types get a `Ref`, `match mut` and
  `for x in mut` write through to the place. The runtime `wolf_rt.ts` contains only the pieces a
  program uses. `extern` functions for TypeScript (ADR 0007).
- `wolf build` writes `out/ts/`; `wolf run` builds it with `tsc --strict` and runs it with Node.js,
  `--example <name>` for examples. Toolchain errors E0801 and E0802 (new in the spec).
- All 10 examples run and print exactly their `expected.txt`; further programs test overflow
  and panics, exit codes, `Err` from `main`, Unicode slicing, the `Float` format, `Map` order and
  the std functions. CI installs Node.js 22 and TypeScript 5.

## M5: Types, part 2 (2026-10-09)

- Bodies of generic functions, types and `impl` blocks are checked: a type parameter is a type
  of its own whose operations come from its bounds (methods of the bound interfaces, `==` with
  `Equatable`, `.copy()` with `Copyable`).
- Bounds of user generics at every use, in calls and in written types (E0404); generic methods
  in inherent `impl` blocks; interfaces with type parameters and coercion to them.
- Interface implementations: missing, extra and mismatched methods (E0413); implementations that
  cannot be written, such as `Rect: Circle`, `Circle: Equatable` or a second implementation
  (E0431, new); object safety of interfaces used as types (E0416); interface members without
  `self` or with type parameters (E0417).
- Closures: parameter types also come from a later use in the same body (`let f = |x| x.len()`).
- `impl` blocks inherit the bounds of their type (spec, new); a `match` whose scrutinee type is
  learned later in the body is still checked for exhaustiveness.
- New language rule: `fn main` is the last function of `functions` (E0213).
- All 10 examples type-check, including `generic_stack`, `shapes` and `textkit`.

## M4: Types, part 1 (2026-10-09)

- `wolf_types`: the type checker of chapter 4. Declarations: aliases expanded (cycles E0419),
  type argument counts (E0403), coherence of `impl` (E0414), one function per name on a type
  (E0415), infinitely sized structs (E0418). Bodies: bidirectional checking with local inference
  (E0401, E0420), operators (E0405), `Equatable`/`Copyable` derived with the offending field named
  (E0406), map keys (E0407), bounds of std generics (E0404), `?` with coercion to interfaces
  (E0408), struct literals and fields (E0422, E0424, E0607), calls and variant forms (E0428),
  types used as values (E0429), loops (E0430, `while true` is Never), missing results (E0421),
  discarded `Result` and useless statements (E0409, E0425).
- Patterns and exhaustiveness with the usefulness algorithm: missing patterns with an example
  (E0410), unreachable arms (E0411), alternatives (E0412).
- `config` constants: allowed types (E0427), constant expressions (E0426), evaluation with the
  integer rules of chapter 4 (E0423), overrides from `wolfpack.toml` (E0613 for alias types).
- Scope per ADR 0006: the seven examples without user generics type-check; generic bodies wait
  for M5.
- Spec: Void blocks discard their final value, `while true` is Never, variant forms, `()` and
  blocks as statements, codes E0426–E0430. std: `Bool.to_string`.
- Tests: a UI project per type rule (E0401 per place where it occurs), std and a large valid
  program type-check cleanly, inference cases, deep nesting through the checker.

## M3: Projects and modules (2026-10-08)

- `wolf_project`: `wolfpack.toml` with exact key positions in errors (E0611), `[config.*]`
  overrides (E0613), the fixed layout (E0612), project discovery from any subdirectory, and
  `wolf new <name> --kind cli|lib`.
- `wolf_std`: the std modules as `.wolf` declarations (`intrinsic` functions and types), with the
  prelude re-exporting the built-in types.
- `wolf_resolve`: two-pass name resolution over the project and std: imports and the import
  graph (E0602–E0605, E0615, E0616, E0620), visibility (E0607, E0608, library API), local scopes
  and shadowing (E0606), unknown names and types with suggestions (E0609, E0402), naming style
  (E0601), module lines (E0610), entry points (E0614), prelude names (E0617), unused variables
  (E0618), sections of test modules (E0619), `intrinsic` outside std (E0701). Failed imports do
  not cause follow-up errors. The resolution table maps names, types, patterns, shorthand fields
  and declarations to what they name, for the type checker.
- New codes in the spec: E0004 (no manifest), E0619, E0620; `intrinsic type` in std.
- `wolf check` checks the project; `wolf check <file.wolf>` stays as a lex-and-parse mode.
- Tests: UI projects for every code of chapter 6, all 10 examples check cleanly (also from a
  subdirectory), `wolf new` projects check cleanly, std resolves cleanly, deep nesting through the
  resolver on the compiler stack.

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
