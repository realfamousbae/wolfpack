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

The installers put the `wolf` command on your `PATH` and then run `wolf setup`, which downloads
the tools that `wolf run` uses for each target: Node.js and TypeScript, Go and Rust. They go into
`~/.wolfpack` (about 1 GB) and do not change versions of these tools that you installed yourself.
Download the installer from the [latest release](https://github.com/realfamousbae/wolfpack/releases/latest):

- **macOS** (Apple silicon and Intel): `wolfpack-<version>-macos.pkg`. The package is not signed
  yet, so macOS refuses to open it with a double-click: open it with a right-click and **Open**, or
  allow it under **System Settings, Privacy & Security**. Rust needs the Command Line Tools of
  Xcode; if macOS offers to install them, accept.
- **Windows** (x64): `wolfpack-<version>-windows-x64-setup.exe`. It installs for your user, without
  administrator rights. Windows SmartScreen warns about an unknown publisher: choose
  **More info** and **Run anyway**.
- **Linux** (x64 and arm64), or macOS from the terminal:

  ```bash
  curl -fsSL https://github.com/realfamousbae/wolfpack/releases/latest/download/install.sh | sh
  ```

Then open a new terminal. `wolf setup --check` shows which tools are installed and where; if a
download failed, `wolf setup` installs what is missing.

To build `wolf` from source instead, you need [Rust](https://rustup.rs) 1.85 or later:

```bash
cargo install --locked --git https://github.com/realfamousbae/wolfpack wolfpack-lang
```

`wolf setup` works the same way afterwards. You can also use tools you installed yourself:
`wolf` looks for them on `PATH` when `wolf setup` has not installed them. Node.js 22, TypeScript
5, Go 1.24 and Rust 1.85 are the oldest versions it supports.

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
| `wolf setup` | install the tools of the targets; `--check` lists them |

The [examples](examples/) of this repository are complete projects; run one with `wolf run` in its
directory.

## Learn more

- [The playground](https://realfamousbae.github.io/wolfpack/) runs the compiler in the browser:
  edit a program, run it, and read the TypeScript, Go and Rust it becomes.
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
