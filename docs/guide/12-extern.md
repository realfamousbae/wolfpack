# 12. Using target libraries

The standard library of wolfpack is small. Everything else is reached through the `extern` section:
it declares functions and types of the target languages, with wolfpack signatures, and the compiler
calls them from the generated code.

## Declaring extern functions

`extern` holds one block per target, `ts`, `go` and `rust`. Each declaration has a wolfpack
signature and, after `from`, the path of the item in the notation of that target:

```wolf
module main

extern {
    ts {
        fn upper(s: String) -> String from "String.prototype.toUpperCase"
        fn larger(a: Float, b: Float) -> Float from "Math.max"
    }
    go {
        fn upper(s: String) -> String from "strings.ToUpper"
        fn larger(a: Float, b: Float) -> Float from "math.Max"
    }
    rust {
        fn upper(s: String) -> String from "str::to_uppercase"
        fn larger(a: Float, b: Float) -> Float from "f64::max"
    }
}

main {
    print(upper("wolfpack"))
    print(larger(2.5, 1.5).to_string())
}
```

```output
WOLFPACK
2.5
```

An extern function is called like any function of the module, and it may be `pub`. When a name is
declared for several targets, the signatures must be the same in every block; only the paths differ.

| Target | Path | Examples |
| --- | --- | --- |
| TypeScript | `module#export`, `module#default`, or a global path | `"some-lib#parse"`, `"Math.max"`, `"String.prototype.trim"` |
| Go | `import/path.Name`, or a method expression | `"strings.ToUpper"`, `"(*bytes.Buffer).WriteString"` |
| Rust | a path, methods in function form | `"std::cmp::max"`, `"str::to_uppercase"` |

For a method path, such as `String.prototype.toUpperCase` or `str::to_uppercase`, the first
wolfpack parameter becomes the receiver.

## What crosses the boundary

Only types that every target can convert are allowed in extern signatures: `Int`, `Float`, `Bool`,
`String`, `Void`, `List`, `Option` and `Map` of these, `Result<T, Error>`, opaque types, and
records (below). Map keys must be `Int`, `String` or `Bool`, and map values may not hold opaque
types. Enums, interfaces and other structs are converted by wolfpack code around the extern call.

- A `Map` is copied into the map type of the target on every call: a `Map` in TypeScript, a
  `map[K]V` in Go, a `&HashMap<K, V>` in Rust. A map that comes back is sorted by its keys, the
  same on every target, because Go and Rust maps have no order of their own.
- A parameter may be a **callback**, a function such as `f: fn(String) -> Bool`, whose parameters
  and result are simple types (`Int`, `Float`, `Bool`, `String`, `Void`, lists of them, opaque
  types). The extern code may call it only while the extern function runs, and must not keep it.
  A panic inside the callback stays a panic, even when the extern function returns a `Result`.
- An `Option` parameter arrives as the target's own "a value or nothing": `T | undefined` in
  TypeScript, a pointer `*T` (to a copy, or `nil`) in Go, and `Option<&T>` in Rust, such as
  `Option<&str>` for `Option<String>`. An `Option<Option<T>>` parameter is not allowed, since
  those forms cannot tell its two kinds of `None` apart.
- Integers are checked when they cross: a Go `int32` or a Rust `usize` that does not fit, for
  example, is a panic, not a silent change of the value.
- A declared `Result` catches the failure of the target: a Go `error`, an exception thrown in
  TypeScript or a Rust `Err` becomes `Err(e)`, and `e.message()` is the target's text. Without a
  `Result`, such a failure is a panic.
- Extern functions are not generic and take no `mut` parameters: extern code cannot change
  wolfpack values. Read parameters are passed without a copy, and the extern code must not keep or
  change them; the compiler cannot check that, so it is a promise of the declaration.

## Records

A struct of your module can cross the boundary **by value**, copied field by field, when a target
block binds it to a struct type of the target with `struct Name from "path"`:

```wolf fragment
module main

extern {
    go {
        struct Point from "native:geo.Point"
        fn moved(p: Point, dx: Float) -> Point from "native:geo.Moved"
    }
}

types {
    struct Point {
        x: Float
        y: Float
    }
}

main {
    let p = moved(Point { x: 1.0, y: 2.0 }, 0.5)
    print(f"{p.x} {p.y}")
}
```

The struct stays an ordinary wolfpack struct, with its literals, fields and methods. The target
type has the same fields, named the target's way: `max_size` is `maxSize` in TypeScript and
`MaxSize` in Go. A record is not generic (`wolf explain E0711`), and its fields have the types of
extern signatures, without opaque types: an opaque value cannot be copied. It may be a parameter or
a result, and inside a `List`, an `Option` result or a `Map`.

## Opaque types

`type Name from "path"` declares a type of the target that wolfpack knows nothing about. Such a
value can be stored, moved and passed to extern functions, but it has no fields, no methods, no
`==` and no `.copy()`:

```wolf
module main

extern {
    go {
        type Regex from "*regexp.Regexp"
        fn compile(pattern: String) -> Regex from "regexp.MustCompile"
        fn matches(r: Regex, text: String) -> Bool from "(*regexp.Regexp).MatchString"
    }
}

main {
    let digits = compile("^[0-9]+$")
    print(matches(digits, "2026").to_string())
}

// file: wolfpack.toml
[package]
name = "digits"
version = "0.1.0"
kind = "cli"

[targets]
default = "go"
enabled = ["go"]
```

## Modules bound to targets

The program above declares its extern names only for Go, so its module can be built only for Go: it
is **bound** to Go. A module that imports a bound module is bound too. A build for a target checks
every module the program reaches; one that has no declarations for that target is an error that
shows the chain of imports leading to it (`wolf explain E0704`).

So a portable library declares its extern names for all three targets. A program for a single
target can use one block and list only that target in `[targets]`.

## Packages of the target languages

Libraries that are not part of a target's standard library are listed in `wolfpack.toml`, per
target, with the version syntax of that target's package manager:

```toml
[extern.ts.dependencies]
"fastest-levenshtein" = "^1.0.16"

[extern.go.dependencies]
"github.com/agnivade/levenshtein" = "v1.2.0"

[extern.rust.dependencies]
"strsim" = "0.11"
```

They are copied into `package.json`, `go.mod` and `Cargo.toml` of the generated projects, and
fetched when the program is built and run. The example `examples/fuzzy` of the repository uses
exactly these three packages.

## Your own target code

Sometimes the function you need is not in any package: you want to write it yourself, in the target
language. Put such code in the directory `native/` of the project, next to `src/`, with one
directory per target: `native/ts/` holds `.ts` files, `native/go/` Go packages (one directory per
package) and `native/rust/` `.rs` files. A path that starts with `native:` then names that code,
in the form of each target:

```wolf
module main

extern {
    ts {
        fn shout(s: String) -> String from "native:shout#shout"
    }
    go {
        fn shout(s: String) -> String from "native:shout.Shout"
    }
    rust {
        fn shout(s: String) -> String from "native:shout::shout"
    }
}

main {
    print(shout("hello"))
}

// file: native/ts/shout.ts
export function shout(s: string): string {
  return s.toUpperCase() + "!";
}

// file: native/go/shout/shout.go
package shout

import "strings"

func Shout(s string) string {
	return strings.ToUpper(s) + "!"
}

// file: native/rust/shout.rs
pub fn shout(s: &str) -> String {
    s.to_uppercase() + "!"
}
```

```output
HELLO!
```

| Target | Path | Names |
| --- | --- | --- |
| TS | `native:<file>#<export>` | the export of `native/ts/<file>.ts` (`text/slug` for a subdirectory) |
| Go | `native:<package dir>.<Name>` | the function or type of the package in `native/go/<package dir>/` |
| Rust | `native:<module path>::<name>` | the item of the module `native/rust/<module>.rs` |

Everything else about extern declarations stays the same: the types allowed at the boundary,
`Result` for errors, opaque types and the binding of modules to targets. `wolf build` copies the
files of `native/<target>/` into the generated project unchanged and imports them from there:
`src/native/` for TypeScript and Rust (where `wolf` also writes the `mod.rs` files that declare the
modules), and `native/` for Go, as packages of the generated module. A Go package of `native/go/`
that imports another one uses the module path of the generated project: the package name, such as
`my-app/native/text`, or `wolfpack.local/<name>` when Go's standard library has a package of that
name.

`wolf check` reports a `native:` path whose file, package or module is missing (`wolf explain
E0709`). Whether the function exists and has the declared types is checked by `tsc`, `go` and
`cargo` when the program is built; when they reject a project with native code, `wolf` shows their
message with the code E0806: look at the native file and at its declaration. Tests written in the
native files are not run by `wolf test`. The example `examples/native-slug` of the repository
calls its own code on all three targets.

## Native modules: no declarations at all

Writing an `extern` line for every function is the price of control. For code of your own project
there is a shorter way: import the file as a **native module**, and the compiler reads its
declarations itself.

```wolf fragment
module main

imports {
    native.text.slug
}

main {
    print(slug.slugify("Hello World"))
}
```

```rust
// native/rust/text/slug.rs
pub fn slugify(title: &str) -> String {
    title.to_lowercase().replace(' ', "-")
}
```

`native.text.slug` is `native/rust/text/slug.rs`. Its `pub fn` items become functions of the
module, its `pub` types become opaque types, and `wolf check` checks every call against them, with
no tool of the target installed. The Rust types map to wolfpack types:

| Wolfpack | Rust |
| --- | --- |
| `Int` | any integer type, checked when it crosses |
| `Float` | `f64`, `f32` |
| `Bool`, `String` | `bool`; `&str`, `String`, `&String` |
| `List<T>` | `&[T]`, `Vec<T>`, `&Vec<T>` |
| `Map<K, V>` | `&HashMap`, `HashMap`; as a result also `BTreeMap` |
| `Option<T>`, `Result<T, Error>` | `Option<T>`; `Result<T, E>` as a result, with `E: Display` |
| a callback | `impl Fn(..)`, `&dyn Fn(..)`, or `F: Fn(..)` |

A `pub struct` whose fields are all `pub` and of these types becomes a **record**: a wolfpack
struct of the module, which you build, read and compare like your own, and which is copied field by
field when it crosses. Any other `pub` type is an opaque type.

```wolf fragment
let s = slug.Size { w: 4, h: 2.5 }
print(f"{slug.area(s)}")
```

An item that does not fit, for example one with a `&mut` parameter or an `async fn`, is simply not
available, and using it says why (`wolf explain E0713`).

The same module can have Go code: `native.text.slug` is also the package in
`native/go/text/slug/`. Its exported functions become functions of the module, with their names in
`snake_case` (`Slugify` is `slugify`, `ParseURL` is `parse_url`):

```go
// native/go/text/slug/slug.go
package slug

import "strings"

func Slugify(title string) string {
	return strings.ReplaceAll(strings.ToLower(title), " ", "-")
}
```

| Wolfpack | Go |
| --- | --- |
| `Int` | any integer type, checked when it crosses |
| `Float` | `float64`, `float32` |
| `Bool`, `String` | `bool`, `string` |
| `List<T>`, `Map<K, V>` | `[]T`, `map[K]V` |
| `Option<T>`, `Result<T, Error>` | the results `(T, bool)` and `(T, error)`; `error` alone |
| a callback | `func(..)` with `int64`, `float64`, `bool`, `string` |
| an opaque type | `*X` for a struct type, `X` for another named type |

A struct whose fields are all exported is a record here too, with its fields in `snake_case`
(`MaxSize` is `max_size`, `ID` is `id`); the generated code uses the Go names. When both files
declare a function or a record, they must agree on its wolfpack signature or fields; if they do
not, it is not available and the import says how they differ (`wolf explain E0715`). A program
that imports a native module is built for the targets that have its code; the compiler reads Rust
and Go in this version, TypeScript comes next. Native modules cannot be re-exported from a library
(`wolf explain E0712`).
