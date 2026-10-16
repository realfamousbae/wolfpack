# Wolfpack

Wolfpack is a small, statically typed programming language whose compiler, `wolf`, translates
your program into **readable TypeScript, Go and Rust**. You write the program once; the compiler
checks it and generates a project in the target language that looks as if a person had written it,
and that builds with that language's own tools.

```wolf
module main

types {
    enum Shape {
        Circle(Float)
        Rect { w: Float, h: Float }
    }
}

functions {
    fn area(s: Shape) -> Float {
        match s {
            Shape.Circle(r) => 3.0 * r * r
            Shape.Rect { w, h } => w * h
        }
    }

    fn main() {
        let shapes = [Shape.Circle(1.0), Shape.Rect { w: 2.0, h: 3.0 }]
        for s in shapes {
            print(area(s).to_string())
        }
    }
}
```

```output
3.0
6.0
```

This program prints the same output on all three targets.

What makes wolfpack different:

- **Every file has the same shape.** A file is a list of sections in a fixed order (`imports`,
  `types`, `impl`, `functions`, `tests`, ...), so you always know where to look.
- **Ownership without lifetimes.** Values have one owner, and parameters say whether a function
  reads, changes or takes a value (`mut`, `own`, marked at the call too). The compiler checks
  this, which lets it emit borrowed references in Rust and plain shared objects in Go and
  TypeScript, without defensive copies.
- **One meaning on every target.** 64-bit integers that panic on overflow, strings measured in
  Unicode characters, maps that keep insertion order, the same panic messages: the differences of
  the three languages are hidden, and the tests of the compiler compare the output of every example
  on all three.
- **Errors you can read.** Every diagnostic has a code, points at the exact place and often
  suggests a fix; `wolf explain E0501` describes any code with a wrong and a fixed example.

## Install

You need [Rust](https://rustup.rs) 1.85 or later to build `wolf` itself:

```bash
cargo install --locked --git https://github.com/realfamousbae/wolfpack wolfpack-lang
```

This installs the `wolf` command into `~/.cargo/bin`. To run programs, install the tools of the
targets you want to use:

| Target | Tools |
| --- | --- |
| TypeScript (the default) | [Node.js](https://nodejs.org) 22 or later, and TypeScript: `npm install -g typescript` |
| Go | [Go](https://go.dev/dl) 1.24 or later |
| Rust | Rust 1.85 or later, which you already have |

## Your first program

```text
$ wolf new hello
created project `hello`
$ cd hello
$ wolf run
Hello, wolfpack!
$ wolf run --target go
Hello, wolfpack!
$ wolf run --target rust
Hello, wolfpack!
```

`wolf new` creates a project with `src/main.wolf`, and `wolf run` builds and runs it. Edit
`src/main.wolf` and run it again. The generated code is in `out/<target>/`;
`wolf build --target all` writes all three.

| Command | |
| --- | --- |
| `wolf check` | check the project without generating code |
| `wolf build` | generate the target project in `out/<target>/` |
| `wolf run` | build and run the program |
| `wolf test` | run the tests of the project |
| `wolf fmt` | format the source files |
| `wolf explain <code>` | explain an error code |

The [examples](examples/) of this repository are complete projects; run one with `wolf run` in its
directory.

## Learn more

- [The playground](playground/README.md) runs the compiler in the browser: edit a program, run
  it, and read the TypeScript, Go and Rust it becomes.
- [The guide](docs/guide/README.md) teaches the language step by step, from the first program to
  ownership, generics, modules and target libraries.
- [The specification](docs/spec/) is the exact definition of the language.
- [The examples](examples/README.md) show the language on small programs.
- [Decisions](docs/decisions/) record why the language and the compiler are the way they are.

## Status

Wolfpack is a young language. The compiler has a lexer, a parser, name resolution, a type checker
with generics and interfaces, an ownership checker, and backends for TypeScript, Go and Rust, plus
`extern` declarations for libraries of the target languages, a formatter and `wolf explain`. A
bytecode virtual machine (`wolf run --vm`) is planned next. The [changelog](CHANGELOG.md) lists
what each milestone added.

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the
[MIT license](LICENSE-MIT), at your option.
