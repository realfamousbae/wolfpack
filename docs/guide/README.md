# The wolfpack guide

This guide teaches wolfpack step by step, for readers who already know another programming
language. The specification in [`docs/spec/`](../spec/) is the precise definition of the language;
the guide explains the same rules with examples and links to the spec where the details are.

How to install `wolf` is described in the [README](../../README.md). The guide starts with your
first project.

## Chapters

1. [Your first program](01-first-program.md): a project, `wolf run` and the other commands.
2. [Basics](02-basics.md): the shape of a file, values, variables, operators and strings.
3. [Control flow](03-control-flow.md): blocks as values, `if`, loops and a first look at `match`.
4. [Functions and closures](04-functions.md): parameters, results, the three parameter modes and
   closures.
5. [Structs and enums](05-structs-enums.md): your own types, methods and pattern matching.
6. [Lists and maps](06-collections.md): the two collections and their std functions.
7. [Errors](07-errors.md): `Option`, `Result`, `?`, the `Error` interface and panics.
8. [Ownership](08-ownership.md): moves, borrows and exclusive access in full.
9. [Generics and interfaces](09-generics-interfaces.md): type parameters, bounds and interface
   values.
10. [Modules and projects](10-modules-projects.md): imports, visibility, `config`, libraries.
11. [Testing](11-testing.md): `tests` sections, `tests/` and `wolf test`.
12. [Using target libraries](12-extern.md): the `extern` section.
13. [What the compiler generates](13-targets.md): the TypeScript, Go and Rust code.

## The examples

Every program in this guide is complete and is checked by the tests of the compiler
(`crates/wolf_cli/tests/guide.rs`): it compiles, it is formatted as `wolf fmt` writes it, it builds
for TypeScript, Go and Rust, and where the guide shows its output, it prints exactly that output on
all three. The generated code shown in chapter 13 is compared with the real output of
`wolf build`, and examples of errors are checked to produce the error they show.
`wolf explain <code>` describes any error code in detail.
