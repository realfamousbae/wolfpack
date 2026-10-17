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
`String`, `Void`, `List` and `Option` of these, `Result<T, Error>`, and opaque types. Maps,
structs, enums and interfaces are converted by wolfpack code around the extern call.

- Integers are checked when they cross: a Go `int32` or a Rust `usize` that does not fit, for
  example, is a panic, not a silent change of the value.
- A declared `Result` catches the failure of the target: a Go `error`, an exception thrown in
  TypeScript or a Rust `Err` becomes `Err(e)`, and `e.message()` is the target's text. Without a
  `Result`, such a failure is a panic.
- Extern functions are not generic and take no `mut` parameters: extern code cannot change
  wolfpack values. Read parameters are passed without a copy, and the extern code must not keep or
  change them; the compiler cannot check that, so it is a promise of the declaration.

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
