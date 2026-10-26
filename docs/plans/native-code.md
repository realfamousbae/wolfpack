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

**Proposed on 2026-10-26, waiting for the author** (spec drafts, no code): stage N4, packages of
npm, Go and crates.io read as **package modules** (section 4.9; D6.10, D6.11, D7.42, D7.43,
D8.30); `wolf check --extern` (section 4.10; D8.31, D8.32); and the TypeScript paths of written
`extern`, written down as built and with members of exports (section 4.11; D7.40, D7.41). Open
questions Q12 to Q25.

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
crates.io (until stage N4, section 4.9, which is proposed).

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

### 4.9 Packages of npm, Go and crates.io (stage N4, proposed)

Not part of N3: reading them needs the fetched packages. **Proposed on 2026-10-26, waiting for the
author**: spec chapter 6, section 1b (D6.10, D6.11), chapter 7, section 8.8 (D7.42, D7.43) and
chapter 8, section 2d (D8.30).

#### What the user writes

```toml
[targets]
enabled = ["go"]

[extern.go.dependencies]
"github.com/agnivade/levenshtein" = "v1.2.0"
```

```
module main

imports {
    go.levenshtein
}

main {
    print(levenshtein.compute_distance("kitten", "sitting").to_string())
}
```

#### The design in short

1. **Import form.** A module path whose root is a target names a package of that target's
   dependencies: `ts.fastest_levenshtein`, `go.levenshtein`, `rust.strsim`, with parts
   `ts.date_fns.locale`, `go.text.unicode.norm`, `rust.regex.bytes`. The second segment is the
   module name of the dependency, derived from its name (`-` and `.` become `_`, npm scopes and Go
   major versions dropped) or given in the manifest:
   `"gopkg.in/yaml.v3" = { version = "v3.0.1", name = "yaml" }`. `ts`, `go` and `rust` become
   reserved roots in `src/` (Q12, Q13).
2. **One package, one target.** The three ecosystems never agree on names (`distance`,
   `ComputeDistance`, `levenshtein` in the `fuzzy` example), so there is no "one module, three
   packages". A package module binds every module that reaches it to its target (E0704). A
   portable program keeps `extern` lines or wraps each package in a file of `native/<target>/`,
   which works today; Q18 is the cheaper form of that wrapper.
3. **Fetching before checking.** The readers need the files, so `wolf check` fetches the packages
   of a target when the project imports a package module of it and they are missing or the
   dependencies changed; `wolf fetch` does it alone; `--offline` forbids it (E0803); the language
   server never fetches (Q14). The tools say where the files are: `npm install` (into
   `out/ts/node_modules`), `go mod download` and `go list -m -json all` (the `Dir` in the module
   cache), `cargo metadata` (the `manifest_path` and the enabled features). The task note "read the
   crate's source in `~/.cargo/registry/src`" holds only for a user's own Rust: the Rust of
   `wolf setup` runs with its own `CARGO_HOME` in the wolfpack home, so `wolf` must ask Cargo and
   never guess the path. Projects without package modules keep checking without any tool.
4. **Readers.** The readers of N3 are reused; what each needs in addition is in the table below.
   Every item that does not map is "not available" with the reason, shown by
   `wolf check --native go.levenshtein` and reported as E0713 only when used; a file of a package
   that cannot be parsed is not E0714, since the user cannot fix it.
5. **Versions** come from `[extern.<target>.dependencies]` as today. What is read is what the tool
   resolved; npm and Cargo ranges may resolve differently on two machines, because `out/` is not
   in git and wolfpack has no lock file (Q16).

#### What the readers can do with real packages

Measured on 2026-10-26 with `wolf` 0.5.1, by copying the packages of `examples/fuzzy` into
`native/` of a scratch project and running `wolf check --native`:

| Package | Read today | Missing for packages |
| --- | --- | --- |
| Go `github.com/agnivade/levenshtein` v1.2.0 | `compute_distance(a: String, b: String) -> Int`: the whole API | finding the directory in the module cache; files with build constraints (`// +build`, `_linux.go`) left out instead of skipped silently |
| crate `strsim` 0.11.1 | 9 of 14 functions (`levenshtein`, `jaro`, `osa_distance`, `sorensen_dice`, ...); the 5 generic ones not available; `hamming` returns the alias `HammingResult`, which the reader makes opaque | the crate root from `cargo metadata`; `pub mod` files, `pub use` of the crate's own items, `cfg(feature)` as Cargo enables it, types of other modules of the crate (none of this is needed for `strsim`, all of it for `regex`) |
| npm `fastest-levenshtein` 1.0.16 | nothing: its `mod.d.ts` is `declare const distance: (a: string, b: string) => number;` and `export { closest, distance };`, two forms the reader skips | the `.d.ts` entry from `package.json`, `declare const` of a function type, export lists, re-exports inside the package, `@types/<name>`; and `distance` would return `Float`, since a package has no other target to settle `number` (Q17) |

What stays infeasible, honestly:

- **The TypeScript type system.** Generics, overloads, conditional and mapped types, `export =`
  with namespaces (the style of `@types/node` and `@types/lodash`) are out of reach for a reader of
  declarations; such packages give few names or none. Small ES-module packages with plain
  functions and classes are the realistic target.
- **Rust APIs built on traits.** `impl AsRef<str>`, `impl Into<String>`, `Cow<'_, str>`, generic
  parameters and borrowed results (`Regex::find` returns `Match<'h>`) are not available; items made
  by macros are not seen. `Regex::new` and `Regex::is_match` would be. A later widening of rule 3
  (`impl AsRef<str>` and `impl Into<String>` as `String`, `Cow<str>` results as `String`) would
  help many crates; it is not proposed now.
- **Go interfaces and other packages' types.** `io.Reader`, `context.Context` and `time.Duration`
  in a signature make it not available; generic Go functions too.
- **rustdoc JSON** would be complete for Rust but needs nightly, and `tsc` as a reader would be
  complete for TypeScript but needs Node.js for every check; both are alternatives of D7.43, not
  the recommendation.

#### Stages

| Step | Content | Size (rough) |
| --- | --- | --- |
| N4a | Package module paths in name resolution (generalizing `native_modules.rs` in `wolf_cli`), the manifest table form and module names (`wolf_project`, E0611), `wolf fetch`, automatic fetching, `--offline`, `wolf-fetched.json`, E0803 for `go` and `cargo` (`wolf_cli`); the Go reader on the module cache with the file rules of D7.43; `--native` for package modules | 1,000 to 1,500 lines with tests |
| N4b | Crates: the library root from `cargo metadata`, `pub mod` files and inline modules, `pub use` of own items, `cfg(feature)` from the enabled features, types across modules of the crate (`wolf_native`) | 600 to 900 lines with tests |
| N4c | npm: the `.d.ts` entry (`exports`, `types`, `index.d.ts`, `@types/<name>`), `declare const` functions, export lists, re-exports inside the package (`wolf_native`, `wolf_cli`) | 600 to 900 lines with tests |

Recommendation (Q15): **N4a with Go first, then N4b, then N4c.** The Go and Rust readers already
read real packages almost as they are (the table above), while TypeScript needs a new kind of
reading and gives `Float` for every `number`. The alternative is TypeScript first, because it is
the default target and npm is the largest ecosystem; it costs the most and helps the least per
package. Tests need fetched packages: unit tests of the readers use small fixture trees in the
repository; one end-to-end test per target fetches a real package over the network, as the
`fuzzy` example already does.

Every step starts with the author's answer to its decisions; nothing of it is visible before.

### 4.10 `wolf check --extern` (proposed)

**Proposed on 2026-10-26, waiting for the author**: spec chapter 8, section 8c (D8.31), codes
E0717 and E0718 (chapter 7, section 9), and E0810 (D8.32).

Written `extern` declarations stay necessary: for the standard libraries, for renaming, and for
portable programs over packages. Today a wrong one is found only by `tsc`, `go build` or `cargo`,
and outside projects with native code it is even reported as E0802, "a bug in wolf" (a misspelt
global such as `"a.b.c.d"` passes `wolf check` and fails `tsc`).

1. `wolf check --extern [module]` compares each written declaration with the item that its path
   names, where a reader can read it: `native:` paths now, package paths after N4a to N4c,
   standard libraries never.
2. Each declaration **matches**, is **not found** (E0717), **differs** (E0718), or is **not
   checked** with the reason. Only what the reader fully understands can be an error: a generic
   function, a type outside the tables of section 8.7, or an item under `#[cfg]` is "not checked",
   because the tool of the target may still accept the call.
3. The fit is that of written declarations (chapter 8, section 8), which is narrower than that of
   native modules: a Go `Int` parameter is `int64`, a TS `Int` parameter a `number`, a Rust read
   `String` parameter `&str`. Where no written declaration can fit, the help points to the native
   module, which takes the types as they are.
4. Output as `--native` and `--bytecode`: a listing on stdout (one line per declaration and a
   summary), the errors on stderr, the exit code of `wolf check`.
5. Separately (D8.32), a rejection by a tool of a project with written `extern` and no native code
   becomes E0810, whose help names `wolf check --extern`; E0802 stays for projects without
   `extern`, where it really is a bug of `wolf`.

| Step | Content | Size (rough) |
| --- | --- | --- |
| X1 | The flag and the listing (`wolf_cli`); a comparator of written declarations against `wolf_native::Module` with the rules of chapter 8, section 8 (`wolf_native`); the readers keep the names they skip (cfg, build constraints) so that they give "not checked" instead of "not found"; E0717, E0718 with `wolf explain` texts and UI tests (`wolf_diag`) | 700 to 1,000 lines with tests |
| X2 | Package paths, on top of N4a to N4c: the same comparator on package modules | 100 to 200 lines |
| X3 | E0810 instead of E0802 (`wolf_cli`, `wolf_diag`) | about 50 lines with tests |

X1 needs nothing of N4 and can come first.

### 4.11 TypeScript member paths of written `extern` (spec gap)

Chapter 7, section 8.2 said only "`module#export` for a module export; a path without `#` is a
global". **Written down on 2026-10-26** in chapter 7, section 8.2b (D7.40, behaviour of 0.5.1,
nothing changes): globals take any number of names (`"Math.max"`, `"console.error"`) and are called
as written; `.prototype.` makes a method; after `#` exactly one name is allowed
(`"node:path#posix.join"` is E0705), except for `native:` paths, which accept `#X.name`,
`#X.prototype.name` and `#X.constructor` because native modules generate them for classes; Node.js
modules and globals need `"@types/node"` in `[extern.ts.dependencies]`, since the generated
`tsconfig.json` has only the ES2022 library (without it `tsc` fails and `wolf` says E0802).

**Proposed** (D7.41): the member forms after `#` for every module, `"node:path#posix.join"`,
`"some-lib#Parser.parse"`, `"some-lib#Parser.prototype.parse"`, `"some-lib#Parser.constructor"`;
and one meaning everywhere: `prototype` only before the last name, `constructor` only last and a
`new` for globals too. Two 0.5.1 behaviours this fixes: `"RegExp.constructor"` is emitted as
`RegExp.constructor(p0)`, which calls JavaScript's `Function`, and `"String.prototype.trim.call"`
as `p0.trim.call()`, which loses the receiver; `tsc` accepts the first one. The change is small:
the path check in `wolf_types` (`ts_path`), the branch of `wolf_ts` that already calls members of
native exports, extended to every export and to global constructors, the E0705 explain text and
its UI test, about 60 to 100 lines.

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
| Q12 | The import form of a package module (4.9; D6.10) | The target as the root: `ts.fastest_levenshtein`, `go.levenshtein`, `rust.strsim`; it shows the binding in the import and matches `[extern.<target>.dependencies]`, `extern { go { } }` and `native/go/`. It reserves `ts`, `go` and `rust` in `src/`, a breaking change like `native` in 0.2. Alternatives: registry names (`npm.`, `go.`, `crates.`), one root `pkg.`, or a string import (a grammar change) |
| Q13 | The module name of a dependency (D6.11) | Derived from the package name, with an optional `name` in a table form of the dependency; for Go the last element of the module path. Alternative: the Go `package` clause (`sqlite3` instead of `go_sqlite3`), known only after fetching |
| Q14 | Does `wolf check` fetch? (D8.30) | Yes, when a package module is imported and its packages are missing or stale, as `cargo check` and `go vet` do; `--offline` forbids it; the language server never fetches. Alternative: only `wolf fetch` fetches, as with `npm` |
| Q15 | The first stage of N4 | N4a with Go, then crates (N4b), then npm (N4c): the readers already read real Go and Rust packages (4.9). Alternative: npm first, since TypeScript is the default target |
| Q16 | A lock file | None now: the versions are what the tools resolve in `out/`, which may differ between machines for npm and Cargo ranges. Later, perhaps a `wolfpack.lock` kept in git, or keeping `package-lock.json` and `Cargo.lock` next to `wolfpack.toml` |
| Q17 | TS `number` in package modules (D7.42) | Always `Float`: no other target can settle it. Code that needs an `Int` writes one `extern` line. Alternative: a key of the dependency in the manifest that reads `number` as `Int` for the whole package |
| Q18 | Portable code over packages | Later, not proposed now: a native file that re-exports from a package (`export { distance } from "fastest-levenshtein"` in TS, `pub use strsim::levenshtein as distance;` in Rust) is read through into the package, so a native module over three packages needs one line per target; Go has no re-exports and keeps a small wrapper function |
| Q19 | Standard libraries as package modules | No for now (chapter 7, section 8.8, rule 9). Go's standard library could be read from `GOROOT` with the same reader; Rust's needs the `rust-src` component, which the `minimal` profile of `wolf setup` does not install; TypeScript's `lib.*.d.ts` declares `Math` as an interface plus a `var`, which the reader does not read |
| Q20 | `wolf check --extern` as a flag (D8.31) | A flag. Alternative: always compare `native:` paths in `wolf check`, once the readers are known never to give a false error |
| Q21 | An unknown module of `wolf check --extern <module>` | E0002, as for `--bytecode`. Alternative: a code of its own, as E0716 for `--native` |
| Q22 | Tool rejections of projects with written `extern` (D8.32) | A new code E0810, so E0802 stays "a bug of `wolf`". Alternative: widen E0806 |
| Q23 | `@types/node` | Document that `node:` modules and Node.js globals need it in `[extern.ts.dependencies]` (D7.40). Later, `wolf` could add it to the generated `package.json` when a path starts with `node:` or names `console` or `process`, at the cost of an `npm install` for such projects |
| Q24 | Constructors in written TS `extern` (D7.41) | `"X.constructor"` gives `new X(...)`, the form native modules already generate. Alternative: a prefix, `"new some-lib#Parser"` |
| Q25 | Cargo features | The table form of D6.11 could take `features = ["..."]` later, copied into `Cargo.toml`; until then a crate gets its default features, and the reader follows what Cargo enables |

Q7 to Q11 are answered (author, 2026-10-19): the recommendations are accepted. Q12 to Q25 are
proposed (2026-10-26), waiting for the author.
