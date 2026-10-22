# Chapter 0: Overview, chapter list and decisions register

Status: **APPROVED** (author, 2026-10-07).

Convention used in every chapter of this spec:

- **[PLAN]** marks content taken from `PLAN.md` (already decided by the author).
- **[PROPOSAL]** marks content proposed by the drafter. It is not binding until the author approves.
- Each chapter ends with a "Decisions for the author" table. A chapter is approved only when every
  row has an answer.

## 1. Chapters

| # | File | Content |
| --- | --- | --- |
| 0 | `00-overview.md` | This file: chapter list, decisions register, cross-target gaps, examples matrix |
| 1 | `01-lexical.md` | Source text, tokens, comments, literals, statement termination |
| 2 | `02-file-skeleton.md` | The nine sections, ordering, duplication and emptiness rules |
| 3 | `03-grammar.md` | Full EBNF, operator precedence, known parser ambiguities |
| 4 | `04-types.md` | Built-in and user types, generics, interfaces, inference, `match` exhaustiveness |
| 5 | `05-ownership.md` | Moves, parameter modes, exclusivity, closures |
| 6 | `06-modules-project.md` | Module paths, imports, visibility, `wolfpack.toml`, project layout |
| 7 | `07-std-extern.md` | `std` modules, `intrinsic`, `extern` blocks and the boundary rules |
| 8 | `08-targets.md` | Mapping to TypeScript, Go and Rust, per-target rules |
| 9 | `09-diagnostics.md` | Diagnostic code ranges and message style (codes themselves live in code) |
| 10 | `10-vm.md` | The bytecode virtual machine: `wolf run --vm`, behaviour, what it cannot run (M13; draft) |

Order of work: 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, then the 10 example projects (written in parallel as
each chapter lands, finished at the end). **[PROPOSAL]**

## 2. Decisions register

Open questions from `PLAN.md` section 14, plus decisions made in this session.
Recommendations are the drafter's; "plan default" is what `PLAN.md` says.

| # | Question | Status | Answer / recommendation |
| --- | --- | --- | --- |
| 1 | Crates.io name | **closed** (author, 2026-10-16) | The command-line package is `wolfpack-lang` with the binary `wolf`; the internal crates are published as `wolfpack-<name>` (`wolfpack-diag`, ...). Publication waits until the repository is public |
| 2 | `Int` size | **closed** (ADR 0001) | 64-bit everywhere, `bigint` in TS. Deviates from plan default |
| 3 | Bytecode vs WebAssembly | open (chapter 10, D10.1) | Plan default: own stack bytecode and VM. Proposed in chapter 10 for M13 |
| 4 | `config` overridable from manifest | **closed** (plan default) | Constants, overridable from `[config]` in `wolfpack.toml`. Follow-up for chapter 6: add a `[config]` table to the manifest and decide which types may be overridden |
| 5 | Methods: `impl` section vs inside `struct` | **closed** (plan default) | Separate `impl` section (matches PLAN section 5) |
| 6 | Closure captures | **closed** (plan default) | Copies and moves only |
| 7 | `Option<T>` in Go | **closed** (plan default), details pending | Pointer for structs, pair for primitives. Follow-up for chapter 8: this conflicts with generic mapping, see gap G4 |
| 8 | Declaration order inside a section | **closed** (plan default) | Free, formatter does not reorder |
| 9 | Integer overflow | **closed** (plan default), details pending | Panic on every target. Follow-up for chapter 8: checked arithmetic in Go and `overflow-checks = true` in generated Rust (gap G8) |
| 10 | Docs language | **closed** | English everywhere (author, 2026-10-07) |
| 11 | Process of M0 | **closed** | Chapter by chapter with approval (author, 2026-10-07) |
| 12 | Gap directions G1 to G10 | **closed** | See section 3, column "Decision" (author, 2026-10-07). G4 is recorded in ADR 0002 |
| 13 | Interactions of G5, G6 and G2 | **closed** (chapter 4) | (a) generic methods only in inherent `impl` blocks, never in interfaces; (b) interface values support `.copy()` but not `==`. Original problem: | (a) A generic method signature inside an `interface` has no Go mapping and makes the Rust trait not object-safe. (b) `Box<dyn Trait>` is neither `Clone` nor `PartialEq` for free, so `.copy()` and structural `==` on interface values need extra rules. Chapter 4 presents options to the author |
| 14 | Go constraint satisfied by both `int64` and structs | **closed** (chapter 8) | Dictionary passing in the Go backend (chapter 8, section 5.4) |
| 15 | Shared `std` `Error` type for `?` | **closed** | `Error` is a built-in interface; `?` coerces a concrete error to it (chapter 4, section 5; chapter 7) |

## 3. Cross-target gaps already visible in PLAN.md

The core is defined as the intersection of the three targets **[PLAN]**. The following points are
where the intersection is not yet defined by `PLAN.md`. Each must get an explicit rule in the
named chapter, otherwise differential tests will fail late. The direction for every gap was decided
by the author on 2026-10-07; the exact rules are written in the named chapters.

| # | Gap | Problem | Decision | Chapter |
| --- | --- | --- | --- | --- |
| G1 | String length and slicing | Go strings index bytes, Rust slices on UTF-8 byte boundaries, JS counts UTF-16 units | `String` length, indices and slices are measured in Unicode scalar values on every target | 4, 7 |
| G2 | `==` on structs | TS `===` compares references | `==` is structural for everything that is comparable. Backends generate equality functions where needed | 4, 8 |
| G3 | `Map` key types | TS `Map` keys objects by identity | Restrict keys to `Int`, `String`, `Bool` (and enums without data) in v1 | 4, 8 |
| G4 | `Option` / `Result` representation | `Option<Option<T>>` and `Option<Void>` collapse in `T \| undefined`. Go `(T, error)` is not a first-class value, so `List<Result<..>>` or a `Result` field has no mapping; `E` must implement Go's `error` | Hybrid: the idiomatic form in function results where it is unambiguous (`(T, error)` in Go, `T \| undefined` in TS); a tagged value when used as data (fields, collection elements, nesting) | 4, 8 |
| G5 | Generic methods in `impl` | Go methods cannot have type parameters | Generic methods are allowed. The Go backend emits them as free generic functions | 4, 8 |
| G6 | Interfaces as value types | If an interface can be a parameter or field type, Rust needs `dyn` and object safety rules | Interfaces are usable both as generic bounds and as value types (`List<Shape>`). Rust uses `Box<dyn Trait>`; chapter 4 defines the object-safety rules for interfaces used as values | 4 |
| G7 | `==`, hash, copy on generic `T` | Constraints like `Comparable` must exist in the language, not only as names in examples | Define a small fixed set of built-in interfaces (`Comparable`, `Hashable`, `Copy`) | 4, 7 |
| G8 | Overflow panic | Go wraps silently, Rust wraps in release unless `overflow-checks` is on | Backends emit checked arithmetic (or set the Cargo profile flag) | 8 |
| G9 | `Float` to string | Rust prints full digits, JS and Go print `1e+21` | Define one canonical formatting in `std.string` and implement it in each backend, or avoid printing floats in examples until M12 | 7, 8 |
| G10 | `Int` boundary in TS | JS `length` and indices are `number`, `Int` is `bigint` | Conversion rule at the `std` boundary, defined in chapter 8 | 8 |

## 4. Examples coverage matrix

Each example is a full project, as required by the layout in `PLAN.md` section 6 and the end-to-end
tests in section 12: `examples/<name>/wolfpack.toml`, `src/main.wolf` (or `src/lib.wolf`),
`expected.txt`, plus further modules where relevant. Example output must avoid anything covered by
the gaps above (floats printed only through the canonical formatting, no non-ASCII string
slicing, and so on). **[PROPOSAL]**

Milestone criteria the 10 examples must serve:

| Criterion | Milestone | Needed |
| --- | --- | --- |
| Examples without generics pass type check | M4 | at least 4 |
| Multi-file, multi-module example resolves | M3 | at least 1 (ideally 2, one `lib`) |
| `tests` sections run and fail clearly | M7 | at least 3 with tests |
| Ownership: valid and invalid code | M8 | at least 2 using `mut` / `own` |
| Generics, interfaces, closures | M5 | at least 3 |
| `extern` for all three targets | M11 | at least 1 |
| `lib` kind | M3 | at least 1 |

The concrete list of 10 programs is proposed in chapter 2 or after chapter 4, once the type system
is approved. Candidates: hello world, FizzBuzz, calculator with `Result`, word counter (std.string,
Map), todo list with enums and `match`, geometry with interfaces, generic stack, binary tree
(recursive types), bank with ownership (`mut`/`own`), `lib` with a multi-module consumer.

## 5. Decisions for the author

| # | Decision | Answer |
| --- | --- | --- |
| D0.1 | Approve the chapter list in section 1 | approved |
| D0.2 | Answer open questions 4 to 9 in the register (section 2) | plan defaults kept |
| D0.3 | Approve the direction for each gap G1 to G10 (section 3) | approved; G1 scalar values, G4 hybrid, G5 and G6 allowed (against the drafter's recommendation to forbid) |
| D0.4 | Approve the coverage matrix (section 4) | approved |
