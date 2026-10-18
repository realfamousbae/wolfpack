# Changelog

All notable changes are recorded here at the end of each milestone.

## 0.2.0 (unreleased)

- Interpolated strings `f"{name} has {items.len()} items"`, raw strings `r"\d+"` and multiline
  strings `"""` whose indentation is that of the closing quotes (spec chapter 1, section 5.3a).
- The built-in interface `Display` (`to_string`), implemented by `Int`, `Float`, `Bool`, `String`
  and by your own types; interpolated strings and `assert_eq` use it.
- Destructuring `let Point { x, y } = p`, `if let` and `while let`.
- `loop` with `break value`, and labelled loops: `outer: for ...`, `break outer`, `continue outer`.
  `loop` is now a reserved word.
- The update syntax `User { name: "B", ..old }`.
- `error(message)` makes an `Error` without a type of its own; `assert_eq(a, b)` shows both values
  when it fails.
- New diagnostics E0114 to E0116, E0315, E0316 and E0433 to E0438.
- The example `syntax-tour`; the guide shows the new forms in chapters 2, 3, 5, 7, 9 and 11.
- `wolf fmt` keeps the content of `"""` strings and moves them with the code.

## 0.1.1 (2026-10-17)

- Installation from a terminal on every system: `install.sh` (macOS, Linux) checks the download
  against `SHA256SUMS.txt`, falls back to `wget`, retries, refuses unsupported systems (musl, old
  glibc, 32-bit ARM) with a clear message, updates the profiles of sh, bash, zsh and fish once,
  and takes options (`--version`, `--dir`, `--no-modify-path`, `--no-setup`). The new
  `install.ps1` does the same on Windows (`irm ... | iex`, PowerShell 5.1 and 7, no administrator
  rights). The release workflow tries both scripts on clean machines.
- `wolfpack-lang` is on crates.io: `cargo install --locked wolfpack-lang`. The internal crates say
  that they are not a stable API.
- The playground is published on GitHub Pages, with a smaller compiler (size-optimized, LTO); its
  buttons wait until the compiler is loaded.
- A new `README.md`: every way to install for each system, options, update, removal and
  troubleshooting.
- Plans in `docs/plans/`: calling the project's own TypeScript, Go and Rust code, and package
  managers and signed installers.
- Branches: `master` is the default branch and holds releases; work lands in `night` first.

## 0.1.0 (2026-10-16)

- `wolf setup` installs pinned, checked versions of Node.js, TypeScript, Go and Rust into
  `~/.wolfpack/toolchains`, and `wolf` uses them before the tools on `PATH` (spec chapter 8,
  section 2a; E0804). `wolf setup --check` lists the tools.
- Release installers: a `.pkg` for macOS (universal), a per-user installer for Windows and
  `install.sh` for Linux and macOS; each runs `wolf setup`. The release workflow builds them on a
  tag, tries each one on a clean machine and creates a draft release.
- The language is written `wolfpack` in lower case, the repository is `wolfpack`, and the crates
  are `wolfpack-lang` and `wolfpack-*`, ready for crates.io.
- Fix: on Windows, tools on `PATH` with `.exe` and `.cmd` names are found, and built programs are
  run by their `.exe` name.

## M12: Polish (2026-10-16)

- `wolf fmt` and `wolf fmt --check`: one canonical layout (spec chapter 3, section 8a); the
  examples and run programs are formatted with it.
- `wolf explain <code>`: a text with a wrong and a fixed example for every code, embedded in the
  compiler; the tests check that each wrong example reports its code first and each fixed one
  checks and builds.
- The language guide, `docs/guide/`, in 13 chapters, and a `README.md` with installation and a
  first program. Every program in them is checked, formatted, built for all three targets and run
  on each where its output is shown; the generated code of chapter 13 is compared with the build.
- The playground, `playground/`: a static page with the compiler in WebAssembly (`wolf_wasm`),
  errors while typing, runs of the generated TypeScript in a worker, the generated TypeScript, Go
  and Rust, formatting, `wolf explain` and links that hold the program.
- `wolf_driver`: the in-memory compiler pipeline, used by `wolf_cli` and `wolf_wasm`.
- Spec: rules of `wolf explain`; a method cannot have the name of a field of its struct (E0432);
  `map.get` is a free function, `remove` a method, an assigning `match` arm a block.
- Fix: a string literal passed to a read parameter of a type parameter builds in Rust.
- Decisions: ADR 0012.
- Tests: explain texts, the guide and the README, the playground functions natively and its
  examples, a run program for generic literals, a UI test for E0432; CI builds `wolf_wasm` for
  WebAssembly.

## M11: `extern` (2026-10-16)

- Checks of extern declarations: types at the boundary (E0702), one signature per name across
  target blocks (E0703), path strings in the form of each target (E0705), no generic functions,
  `self` or `mut` parameters (E0706), no operations on opaque types besides extern functions
  (E0707), no duplicates in a block (E0708).
- Target binding: a build rejects every module whose extern names are not declared for the
  target, with the import chain (E0704).
- All three backends: opaque types as type aliases (also Go pointers), method paths called on the
  first parameter, checked integer conversions, `Result` and `Option` results (TS exceptions
  become `Err`, `null` becomes `None`; Rust errors keep their `Display` text), string and list
  results of the target's own types in Rust.
- `[extern.<target>.dependencies]` goes into `package.json`, `go.mod` and `Cargo.toml`;
  `wolf run` and `wolf test` install TS packages with `npm install` (E0803 when it fails).
- New example `fuzzy` with a package of each target language.
- Fix: a package named `deps`, `examples`, `build` or `incremental` gets a Rust binary named
  `<name>-bin`, since Cargo reserves these names.
- Spec: integer conversions at the boundary, pointer types in Go paths, fetching dependencies,
  E0803.
- Decisions: ADR 0011.
- Tests: a UI test for each new code, run programs for the extern boundary and for E0704 and
  E0803, `fuzzy` runs and passes its tests on every target.

## M10: The Rust backend (2026-10-14)

- `wolf_rs`: the Rust backend builds a tree and prints it as rustfmt formats it. Each module is a
  file (`src/users/storage.rs` with `pub mod` declarations in `src/users.rs`); std and the
  runtime are the module `wolf_rt`, which holds only the pieces a program uses. `wolf build`,
  `wolf run` and `wolf test` take `--target rust`, and `rust` is the default when `wolfpack.toml`
  says so. `wolf build --target all` builds every enabled target.
- Borrowing follows the modes: read parameters are `&T`, `&str`, `&[T]`, `&dyn Trait` or
  `&dyn Fn`, `mut` parameters `&mut T`, `own` parameters `T`; locals are `let mut` only where they
  change. Recursive fields are boxed, interface values are `Box<dyn Trait>` with a `copy_dyn`
  method for `.copy()`, `own self` interface methods take `self: Box<Self>`.
- Native `Option`, `Result` and `?`, `match` with `if let` and `while let` where they fit,
  `format!` for string concatenation, derives of `Clone` and `PartialEq` where the fields allow
  them, checked arithmetic through `overflow-checks = true`, indexing through `items.at(i)`, the
  canonical panic messages through a panic hook.
- `wolf test` on Rust runs `cargo test` per Cargo target and prints the same report as on
  TypeScript. E0801 and E0802 name `cargo`.
- Fixes in the earlier backends: Go holds fields recursive through `Option` by pointer, copies
  interface values through a generated `Copy<Name>` method, and puts plain enums with `mut`
  methods behind pointers as interface values; TypeScript narrows `match mut` on list elements;
  `--out` is resolved against the current directory.
- Decisions: ADR 0010.
- Tests: every example and every run program gives on Rust the output it gives on TypeScript,
  with `cargo fmt --check` and `cargo clippy -- -D warnings` clean; the test reports and filters
  match; tests stay out of the normal build; the output is deterministic; E0801 and E0802 for
  `cargo`; new run programs for recursive data, interface values and `own self` interface
  methods; CI builds the generated Rust with Rust 1.85.

## M9: The Go backend (2026-10-13)

- `wolf_go`: the Go backend builds a tree and prints it as `gofmt` formats it. Each module is a
  package; std and the runtime are the package `wolfrt`, which holds only the pieces a program
  uses. `wolf build`, `wolf run` and `wolf test` take `--target go`, and `go` is the default when
  `wolfpack.toml` says so.
- Idiomatic results: `Option<T>` is `(T, bool)` or `*T`, `Result<T, E>` is `(T, error)` or
  `error` when `E` implements `Error`; `?` becomes `if err != nil { return ..., err }`, and a
  `match` on such a result becomes `if v, ok := f(); ok`. Everywhere else the data forms
  `wolfrt.Option[T]` and `wolfrt.Result[T, E]` are used.
- Structs with pointer receivers when a method takes `mut self`, `mut` parameters as pointers,
  enums without payloads as constants, enums with payloads as sealed interfaces with a pointer to
  a struct per variant, `match` as type switches, value switches or `if` chains (a `break` inside
  a `switch` gets a label), generic functions with dictionaries for `Equatable`, `Copyable` and
  `Comparable`, generated `Equal` and `Copy` where they are used, checked integer arithmetic.
- `wolf test` on Go runs `go vet` and `go test -json` and prints the same report as on
  TypeScript. E0801 and E0802 name `go`.
- The IR keeps the bounds of `impl` blocks on the type's parameters.
- Decisions: ADR 0009.
- Tests: every example and every run program gives on Go the output it gives on TypeScript, with
  `gofmt -l` and `go vet` clean; the test reports match; E0801 and E0802 for `go`.

## M8: Ownership (2026-10-10)

- `wolf_own`: the ownership rules of chapter 5, checked on the IR after type checking, as part of
  `wolf check`. A flow-sensitive walk of each body tracks moved variables through branches and
  loops: use after a move (E0501), use after a move on some paths or in a previous iteration
  (E0502), moves of fields and elements (E0503), moves out of borrowed values (E0504), changes of
  immutable bindings (E0505), `mut` arguments that are not places (E0506), call-site markers
  (E0507), overlapping places in one call (E0508), access to places borrowed by `for` and `match`
  (E0509), closures capturing borrowed values (E0510) or changing captured ones (E0511).
- The IR carries source spans and the functions of built-in types.
- Spec: `own` is never written before a temporary; the example of chapter 9 is fixed.
- Tests: a pair for every rule, a project with the error and a correct project
  (`tests/valid/`); all examples and run programs still check and run.

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
