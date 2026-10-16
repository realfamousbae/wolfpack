# An internal crate of wolfpack

This crate is a part of the compiler of [wolfpack](https://github.com/realfamousbae/wolfpack), a
small statically typed language that compiles to readable TypeScript, Go and Rust. It is published
only because the command-line package `wolfpack-lang` depends on it.

**It is not a stable API.** Its types and functions change with every version of the compiler,
without notice and without following semantic versioning. Do not depend on it.

To use wolfpack, install the `wolf` command:

```bash
cargo install wolfpack-lang
```

or use an installer from the [releases](https://github.com/realfamousbae/wolfpack/releases).
