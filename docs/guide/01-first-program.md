# 1. Your first program

## A new project

Every wolfpack program lives in a project. `wolf new` creates one:

```text
$ wolf new hello
created project `hello`
$ cd hello
```

The project has a fixed layout, the same for every project:

```text
hello/
  wolfpack.toml       the manifest: name, version, kind, targets
  src/
    main.wolf         the program
  tests/              integration tests (chapter 11)
  examples/           usage examples
  .gitignore          ignores out/, where the generated code goes
```

`wolfpack.toml` describes the project:

```toml
[package]
name = "hello"
version = "0.1.0"
kind = "cli"

[targets]
default = "ts"
enabled = ["ts", "go", "rust"]
```

`kind = "cli"` makes it a program, whose code starts in `src/main.wolf`. The other kind, `lib`, is
a library (chapter 10). `[targets]` says which languages the project is built for and which one is
used when a command does not name one.

`src/main.wolf` holds a minimal program:

```wolf
module main

functions {
    fn main() {
        print("Hello, wolfpack!")
    }
}
```

```output
Hello, wolfpack!
```

Even this small file shows how every wolfpack file is built:

- It starts with a `module` line that names the module. A module is one file, and its name is its
  path below `src/`: `src/main.wolf` is `main`, `src/users/model.wolf` would be `users.model`.
- Everything else is in **sections**. Functions go into the `functions` section; chapter 2 shows the
  others.
- `fn main()` is where the program starts. It is the last function of its section.
- `print` writes a string and a line break to standard output.

## Running it

```text
$ wolf run
Hello, wolfpack!
```

`wolf run` translates the program into the default target language, TypeScript, builds it with the
tools of that language and runs it. Pick another target with `--target`:

```text
$ wolf run --target go
Hello, wolfpack!
$ wolf run --target rust
Hello, wolfpack!
```

Each target needs its own tools: Node.js 22 and TypeScript for `ts`, Go 1.24 for `go`, Rust 1.85 for
`rust` (or later versions). A program prints the same output on every target; that is a promise of
the language, and the tests of the compiler check it.

## The other commands

| Command | What it does |
| --- | --- |
| `wolf check` | checks the project and reports errors, without generating code |
| `wolf build` | writes the generated project to `out/<target>/`; `--target all` builds every enabled target |
| `wolf run` | builds and runs the program |
| `wolf test` | runs the tests of the project (chapter 11) |
| `wolf fmt` | formats the source files; `wolf fmt --check` only lists the files that need it |
| `wolf explain <code>` | explains an error code with a wrong and a fixed example |
| `wolf new <name>` | creates a project; `--kind lib` creates a library |

The commands find the project by looking for `wolfpack.toml` in the current directory and its
parents, so they work from any directory inside the project.

The code in `out/` is meant to be read: it looks like code a person would write in that language,
and it can be built with the usual tools (`npm`, `go build`, `cargo build`) without `wolf`.
Chapter 13 shows what it looks like.

## When something is wrong

Change `print` to `prnt` and check the project:

```wolf error E0609
module main

functions {
    fn main() {
        prnt("Hello, wolfpack!")
    }
}
```

```text
$ wolf check
error[E0609]: unknown name `prnt`
  --> src/main.wolf:5:9
  |
5 |         prnt("Hello, wolfpack!")
  |         ^^^^ not found in this scope
  |
  = help: did you mean `print`?

error: aborting due to 1 error
```

Every error has a code, points at the exact place, and often suggests a fix. The compiler reports
all the errors it finds, not only the first one. For a longer description of an error, ask for it
by its code:

```text
$ wolf explain E0609
```

The next chapter goes through the building blocks of a file.
