# Plan: calling your own TypeScript, Go and Rust code

Status: **N1 done** (2026-10-19, for 0.2): `native/<target>/`, `native:` paths, E0709, E0710 and
E0806 (spec chapter 7, section 8.2a; chapter 8, section 8a), the example `examples/native-slug`.
N2: `Map` at the boundary and callbacks done (2026-10-19), records done (2026-10-21, D7.15). **N3 is revised** (accepted by the author
2026-10-19, with the recommendations of Q7 to Q11); **N3a done** (2026-10-20): native modules read
from Rust code, without records; **N3b done** (2026-10-21): the Go reader and the agreement of
targets (E0715); records of native modules done (2026-10-21, D7.18), read from Rust and Go;
**N3c done** (2026-10-21, D7.19): the TypeScript reader (`oxc_parser`) and the `number` rule,
`examples/native-slug` without `extern`; **N3d done** (2026-10-21, D7.20, D8.13): methods of
opaque types and `wolf check --native`. The native plan is complete for 0.2:
instead of a command that prints `extern` blocks, native code is
**imported like a module**, and the compiler reads its declarations itself (section 4). Each stage
starts with an edit of `docs/spec/` in its own commit and needs the author's approval, as every
language change does. The open questions at the end are for the author.

### Why the revision

The author's intent for native code is to use code of the target languages **without extra work**:
no precompilation and no hand-written declarations. N1 and N2 give something narrower: your own
target code, called through `extern` lines written by hand, one block per target. Every function is
declared three times (see `examples/native-slug`), and a mistake in a declaration shows up only as
E0806 from `tsc`, `go` or `cargo`. Section 4 removes the declarations for the project's own code.

## 1. Where we are

`extern` (chapter 7, section 8; ADR 0011) already lets a module call **packages** of a target
language:

- functions and opaque types of the target's standard library (`"strings.ToUpper"`,
  `"Math.max"`, `"std::cmp::max"`);
- functions and opaque types of third-party packages listed in `[extern.<target>.dependencies]`
  (npm, Go modules, crates.io), as the `fuzzy` example shows.

What is **not** possible today:

1. **Code of your own project.** `src/` may contain only `.wolf` files (E0612), and a path string
   names a package, not a file. To call a TypeScript function you wrote, you have to publish it as
   a package first.
2. **Declarations without writing them by hand.** Every function needs a hand-written `extern`
   line per target, with a signature that `wolf` cannot check against the target until `tsc`,
   `go build` or `cargo` runs.
3. **Rich values at the boundary.** Only `Int`, `Float`, `Bool`, `String`, `Void`, `List`,
   `Option`, `Result<_, Error>` and opaque types cross it (E0702). `Map`, structs, enums, bytes and
   functions do not.

The plan has three stages, in this order, because each one makes the next one more useful.

| Stage | What it gives | Needs |
| --- | --- | --- |
| N1. Native sources | `.ts`, `.go`, `.rs` files inside the project, called through `extern` | spec chapters 6, 7, 8 |
| N2. A wider boundary | `Map`, functions (callbacks) and records cross `extern` | spec chapters 7, 8 |
| N3. Native modules | `imports { native.slug }`: the compiler reads `native/<target>/` and derives the declarations; no `extern` lines | spec chapters 6, 7, 8; readers of TS, Go and Rust in `wolf` |

## 2. Stage N1: native sources in the project

### 2.1 Layout

A new top-level directory next to `src/`, one subdirectory per target:

```
my-app/
  wolfpack.toml
  src/
    main.wolf
  native/
    ts/
      slug.ts
    go/
      slug/
        slug.go          package slug
    rust/
      slug.rs
```

- `src/` keeps its rule: only `.wolf` files. Native code lives apart, so it is always clear which
  files the compiler checks and which it only copies.
- A target directory may be missing. A module that uses native code of one target only is bound to
  that target, exactly as a module with a one-target `extern` block is today (E0704).
- The layout is fixed, like the rest of the project layout; there is no manifest key for it.

### 2.2 Declaring native functions

The `extern` block stays the only way in. A path string gets a **marker for project code**; the rest
of the path keeps the notation of the target (open question Q1 discusses the marker):

```
extern {
    ts {
        pub fn slugify(s: String) -> String from "native:slug#slugify"
    }
    go {
        pub fn slugify(s: String) -> String from "native:slug.Slugify"
    }
    rust {
        pub fn slugify(s: String) -> String from "native:slug::slugify"
    }
}
```

All rules of extern functions apply unchanged: the boundary types, `Result` and panics, the
read-parameter contract, opaque types (`type Index from "native:search.Index"`).

### 2.3 What the compiler does

| Target | Copied to | The call |
| --- | --- | --- |
| TS | `out/ts/src/native/**` | `import { slugify } from "../native/slug.js"` (relative to the module) |
| Go | `out/go/native/**` (packages of the generated module) | `import "<module path>/native/slug"` |
| Rust | `out/rust/src/native/**`, plus a generated `mod native;` with the module tree | `crate::native::slug::slugify` |

1. Files are copied **byte for byte**. The compiler does not parse or rewrite them, so the user's
   formatting and comments survive and the generated project still looks hand-written.
2. A native file may use the dependencies in `[extern.<target>.dependencies]` and the standard
   library of its language. It must not depend on `wolf_rt`, whose contents are internal.
3. `wolf build`, `wolf run` and `wolf test` copy the files for the target being built. The
   playground has no native code.
4. Native files are not tested by `wolf test`. A target's own tests (for example `#[cfg(test)]` in
   a `.rs` file) run only when the user runs the target's tool in `out/<target>/`.

### 2.4 New checks

The compiler checks what it can without understanding the target language:

- the file or package named by a native path exists (a new E07xx code);
- a native path is used only for a target whose `native/<target>/` exists (the same code);
- a native file name does not collide with a generated name (`wolf_rt`, a module of `src/`)
  (another new E07xx code);
- `native/` contains no other directories than `ts`, `go` and `rust` (E0612 extended).

Type mismatches between the declaration and the native function are still reported by `tsc`,
`go build` or `cargo`. Stage N3 moves this check into `wolf`.

### 2.5 Done when

- An example `examples/native_slug/` calls one native function per target and prints the same
  output on all three, in the end-to-end tests.
- `wolf explain` texts for the new codes; the guide gets a chapter "Your own target code".

## 3. Stage N2: a wider boundary

Each item is independent and can be approved alone. The order is by usefulness.

### 3.1 `Map<K, V>`

`K` and `V` are boundary types, `K` also `Hashable`.

| Target | Parameter | Result |
| --- | --- | --- |
| TS | `Map` (insertion order kept) | any `Map` |
| Go | `map[K]V` | `map[K]V`, **sorted by key** when converted |
| Rust | `&HashMap<K, V>` built on the fly, or the `IndexMap` of `wolf_rt` if a flag says so | `HashMap`, `BTreeMap`, `IndexMap` (anything iterable as pairs) |

Wolfpack maps keep insertion order (ADR 0003). A Go map has no order, so a result coming from Go
gets the key order; the spec must say so. A Rust `HashMap` result likewise (Q4).

### 3.2 Functions (callbacks)

A parameter of a function type, for example `fn each(items: List<String>, f: fn(String) -> Void)`.

- TS: a plain function. Go: a `func`. Rust: `&dyn Fn(..) -> ..` (or `impl Fn`, chosen per path).
- The extern code may call it only **during the call**, and must not keep it; this extends the
  read-parameter contract of D7.8 and cannot be checked by the compiler.
- A panic in the callback crosses the extern code as the target's panic or exception and becomes a
  wolfpack panic again on return.
- Results of a function type (extern code returning a closure) stay out: they would need a way to
  own target closures.

### 3.3 Records

A struct whose fields are all boundary types can be passed **by value** when its declaration says
which target type it maps to:

```
extern {
    go {
        type Point from "native:geo.Point" { x: Float, y: Float }
    }
}
```

The generated wrapper copies field by field (`X`, `Y` in Go by the naming rules of chapter 8).
TS needs no target type: the value is a plain object. Enums stay out of N2.

### 3.4 Bytes

Byte arrays (`Uint8Array`, `[]byte`, `Vec<u8>`) need a `Bytes` type in `std` first. That is a
language feature of its own and goes to `docs/ideas.md`; the boundary rule follows it. A draft
of both is in the spec (2026-10-26, waiting for the author): chapter 4, section 4, rule 7
(D4.30), chapter 7, sections 6a and 8.4 (D7.30 to D7.33), chapter 8, sections 4.2a and 8
(D8.25).

### 3.5 Done when

One example per item, on all three targets, in the end-to-end tests; the ADR of N2 lists the
conversions and their costs.

## 4. Stage N3 (revised): native modules without declarations

### 4.1 What the user writes

```
module main

imports {
    native.slug
}

main {
    print(slug.slugify("Hello, World!"))
}
```

`native.slug` is a **native module**: the files `native/ts/slug.ts`, the Go package
`native/go/slug/` and `native/rust/slug.rs`, whichever exist. There is no `extern` block. The
compiler reads those files, finds what they export, and gives each exported function, struct and
type a wolfpack declaration. `slug.slugify` is then an ordinary call, checked by `wolf check` like
any other.

`extern` with `native:` paths (N1) stays: it is still the way to rename a function, to call a
method in function form, to choose a type the reader would not, and to call packages of npm, Go and
crates.io.

### 4.2 Readers: what `wolf` understands of each language

The readers look only at **declarations**, never at bodies, and need no tool of the target: `wolf
check` keeps working without Node.js, Go or Cargo installed.

| Target | Reader | Reads |
| --- | --- | --- |
| Rust | the `syn` crate | `pub fn`, `pub struct`, `pub enum` without payloads (later), `pub type` |
| Go | a small parser of Go declarations written for `wolf` (top-level `func` and `type` lines only) | exported functions, struct types, other named types |
| TS | the `oxc_parser` crate | `export function`, `export interface`, `export type`, `export class` |

An item that a reader cannot map (a generic function, an untyped TS parameter, a type from another
package) is **not visible** in wolfpack. Using it is an error that says why
(`slug.parse is not available: its parameter has no type in native/ts/slug.ts`), so nothing is
skipped silently.

### 4.3 Mapping target types to wolfpack types

| Wolfpack | TS | Go | Rust |
| --- | --- | --- | --- |
| `Int` | `bigint`; `number` see Q8 | any integer type | any integer type |
| `Float` | `number` | `float32`, `float64` | `f32`, `f64` |
| `Bool`, `String` | `boolean`, `string` | `bool`, `string` | `bool`, `&str`, `String` |
| `List<T>` | `T[]` | `[]T` | `&[T]`, `Vec<T>` |
| `Map<K, V>` | `Map<K, V>` | `map[K]V` | `&HashMap`, `HashMap`, `BTreeMap` |
| `Option<T>` | `T \| undefined` | `(T, bool)`; `*T` see Q9 | `Option<T>` |
| `Result<T, Error>` | see Q8 (exceptions) | `(T, error)`, `error` | `Result<T, E: Display>` |
| callback | `(a: A) => R` | `func(A) R` | `impl Fn(A) -> R`, `&dyn Fn(A) -> R` |
| record | `interface` / `type` of plain fields | struct of exported fields | struct of `pub` fields |
| opaque type | `class`, other types | other named types, `*T` | other `pub` types |

Names follow the wolfpack conventions: `countWords` and `CountWords` become `count_words`,
`Slugify` becomes `slugify`. Two target names that map to one wolfpack name are an error.

### 4.4 One function, several targets

A native module may exist for one target or for several. For each exported name, the signatures
derived from the targets that have it must be **the same** wolfpack signature (a new E07xx code
shows both). Where TS is ambiguous (`number`), the other targets decide. A name that exists only in
some targets binds the calling module to those targets, exactly as a one-target `extern` block does
today (E0704).

### 4.5 Records and opaque types

A struct whose fields are all mappable becomes a **record** of the native module: a wolfpack struct
`slug.Options { max_len: Int }` that the user can build and read, copied field by field at the
boundary (the conversions of N2 and of D7.15). Any other exported type becomes an opaque type. This
replaces the hand-written binding `struct Name from "path"` of D7.15 for native code (Q10).

### 4.6 Seeing what the compiler derived

`wolf check --native` (or a later `wolf bind native.slug`) prints the declarations derived from each
target, as an `extern` block, with the items left out and why. This is the former N3 command, now a
view of what the compiler already does.

### 4.7 Stages

| Step | Content |
| --- | --- |
| N3a | The import form, native modules in name resolution, the Rust reader (`syn`), the mapping of 4.3 for Rust; `examples/native-slug` without `extern` for Rust |
| N3b | The Go reader |
| N3c | The TS reader (`oxc_parser`) and the `number` rule |
| N3d | Agreement between targets (4.4), records and opaque types (4.5), `wolf check --native` |

Every step starts with its spec edit. The playground has no native modules; the VM of M13 does not
run them either (a diagnostic there).

### 4.8 Costs

- `syn` and `oxc_parser` are new dependencies of `wolf_cli` only, not of the playground; `oxc` is
  large, so the binary and build time grow.
- The Go reader is new code to maintain (declarations only, a few hundred lines).
- The readers cover the declarations people write in practice, not every corner of three languages;
  the rest stays reachable through explicit `extern`.

### 4.9 Packages of npm, Go and crates.io

Not part of N3: reading them needs the fetched packages (`node_modules/**/*.d.ts`, the Go module
cache, rustdoc JSON, which is nightly-only for Rust). The same readers can serve them later; until
then packages keep explicit `extern` declarations.

## 5. Out of scope

- Calling code of **another** target (Go code from a TypeScript build) through WebAssembly, FFI or
  a subprocess (variant B of the discussion of 2026-10-19). Possible in principle, but every target
  pair needs its own runtime bridge.
- Code of languages that are not targets, such as Python or C, through a C ABI or a subprocess
  (variant C). A separate project.
- Async functions and promises at the boundary: the language has no async yet (PLAN, after M13).
- Inline target code inside `.wolf` files (`ts { ... }` bodies). It would make modules unreadable
  for people who know only one of the targets.

## 6. Open questions for the author

| # | Question | Recommendation |
| --- | --- | --- |
| Q1 | The marker for project code in a path string | `native:` as a prefix. It cannot be a package name in any of the three ecosystems, and it reads well. Alternatives: a leading `./`, or a separate keyword (`from native "slug#slugify"`) |
| Q2 | Directory name `native/` | `native/`. Alternatives: `ffi/`, `target/` (taken by Cargo), `host/` |
| Q3 | Should `wolf test` also run the native tests of the target? | No in N1; a flag `wolf test --native` later |
| Q4 | Order of a map that comes from Go or from a Rust `HashMap` | Sorted by key, so the output is the same on every run |
| Q5 | Is the callback contract of 3.2 (call only during the extern call, never keep) acceptable? | Yes; it is the same kind of promise as D7.8 |
| Q6 | When: after M13, or between M13 and the LSP? | N1 right after M13; N2 and N3 by demand |
| Q7 | The import form of a native module | `imports { native.slug }`, like any module; `native` cannot be a module of `src/` already (E0710) |
| Q8 | TS `number`, and TS exceptions | `number` is `Float` unless another target of the same function says `Int`; `bigint` is `Int`. A TS function cannot declare that it throws, so its result is never a `Result`; a JSDoc tag `@throws` could make it one later |
| Q9 | Go `*T` results | `*T` of a record or an opaque type is that type, and `nil` is a panic; `Option` in Go is `(T, bool)` |
| Q10 | Records of D7.15 (`struct Name from "path"`) | Superseded (author, 2026-10-21): records of D7.15 are implemented, and native modules derive records (D7.18) |
| Q11 | Scope of N3 | Only the project's own `native/` code; packages later (4.9) |

Q7 to Q11 are answered (author, 2026-10-19): the recommendations are accepted.
