# Plan: calling your own TypeScript, Go and Rust code

Status: **N1 done** (2026-10-19, for 0.2): `native/<target>/`, `native:` paths, E0709, E0710 and
E0806 (spec chapter 7, section 8.2a; chapter 8, section 8a), the example `examples/native-slug`.
N2: `Map` at the boundary done (2026-10-19); callbacks and records are still a plan, and so is N3. Each stage starts with an edit of `docs/spec/` in its own commit and
needs the author's approval, as every language change does. The open questions at the end are for
the author.

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
| N3. `wolf bind` | `extern` declarations generated from the target's own code | a new command (PLAN.md section 10, the guide) |

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
language feature of its own and goes to `docs/ideas.md`; the boundary rule follows it.

### 3.5 Done when

One example per item, on all three targets, in the end-to-end tests; the ADR of N2 lists the
conversions and their costs.

## 4. Stage N3: `wolf bind`

A command that reads target code and prints `extern` declarations for it:

```bash
wolf bind ts fastest-levenshtein
wolf bind go github.com/agnivade/levenshtein
wolf bind rust native
```

1. **Input.**
   - TS: the `.d.ts` files of an npm package (after `npm install`) or of `native/ts/`. Read with a
     TypeScript parser crate (`oxc` or `swc`), no Node.js needed.
   - Go: a small Go program shipped with `wolf`, run with the Go toolchain of `wolf setup`; it uses
     `go/packages` and `go/types` and prints the exported functions and types as JSON.
   - Rust: `native/rust/` parsed with `syn`. Crates from crates.io need rustdoc JSON, which is
     still nightly-only in Rust; until it is stable, crates are left to hand-written declarations.
2. **Output.** An `extern` block on standard output, or appended to a chosen `.wolf` file with
   `--write <file>`. Items whose types cannot cross the boundary are listed as comments with the
   reason, so nothing is skipped silently.
3. **Checking.** `wolf check --extern` reuses the same readers to compare existing declarations
   with the target code and reports a mismatch at the declaration, before `tsc`, `go build` or
   `cargo` would.
4. Names follow the wolfpack conventions (`ComputeDistance` becomes `compute_distance`); the path
   string keeps the target's name.

`wolf bind` changes no language rule, but it is a new command of `wolf` (PLAN.md section 10), so it
needs the author's approval like any other part of the tool's interface.

### 4.1 Done when

`wolf bind` reproduces the hand-written declarations of the `fuzzy` example and of the N1 example,
in tests that run without network access (fixtures of `.d.ts`, Go and Rust files).

## 5. Out of scope

- Calling code of **another** target (Go code from a TypeScript build) through WebAssembly, FFI or
  a subprocess. Possible in principle, but every target pair needs its own runtime bridge.
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
