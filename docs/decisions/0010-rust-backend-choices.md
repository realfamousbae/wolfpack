# 0010: Choices of the Rust backend

Status: proposed. Items 1 to 3 are open questions for the author; the other items are [INTERNAL]
choices recorded so that the author can object. Items 12 to 15 change backends of earlier
milestones; ADR 0009 stays as accepted.

## Decision

1. **`#![allow(clippy::type_complexity)]` at the crate root.** Function values are
   `Box<dyn Fn(&i64) -> i64>`, and a list of them (`&[Box<dyn Fn(&i64) -> i64>]`) passes the
   complexity limit of clippy. The alternative is a generated `type` alias per function type.
2. **Recursion through `Option` or `Result` boxes the whole field**: `next: Box<Option<Node>>`.
   The form rustc suggests is `Option<Box<Node>>`.
3. **`--target all` is accepted only by `wolf build`**; `wolf run` and `wolf test` reject it with
   E0002, as `PLAN.md` mentions `all` only for the build.
4. **Value contexts.** Every expression is emitted for one of four wants: an owned value, a read
   (by value for copy types, by reference otherwise), a `&mut` place, or an operand. Arguments
   follow the declared parameter: read `String`, `List<T>`, interfaces and function types are
   `&str`, `&[T]`, `&dyn Trait` and `&dyn Fn`; copy types are passed by value.
5. **Fields that make a type recursive are boxed** (`Box<Tree>`), found from the types each type
   holds by value; patterns see through the box with `&*field`, assignments write `*field`.
6. **Derives follow the fields**: `Clone` and `PartialEq` when every field allows them; enums
   without payloads derive `Clone, Copy, PartialEq, Eq, Hash`.
7. **Interface values** are `Box<dyn Trait>`; `.copy()` calls a `copy_dyn` method that every
   implementation gets, and `impl Clone for Box<dyn Trait>` uses it. `own self` methods take
   `self: Box<Self>` (chapter 4), and a call on a concrete value boxes the receiver.
8. **`#![allow(arithmetic_overflow, unconditional_panic)]` at the crate root**, so that an
   overflow or an out-of-range index in constant expressions is a panic at run time, as on the
   other targets, and not a compile error.
9. **Idiomatic forms**: `if let` for a `match` with one arm and an empty `_`, `while let` for a
   loop around such a `match`, `format!` and `println!` with inline arguments for string
   concatenation, `is_none()` for `== None`, a function instead of a closure that only calls it,
   `_name` for parameters the body does not use, `impl Default` for a `new()` without parameters.
   Clippy lints about the structure of the source (`manual_find` for a loop with an early
   `return`) are not rewritten: the code follows the program.
10. **The printer follows rustfmt's heuristics** (widths 60 for calls and chains, 18 for struct
    literals, 100 overall; overflow of the last argument by `can_be_overflowed_expr`, the
    `choose_rhs` rule for `let`), so that `cargo fmt --check` finds nothing.
11. **`wolf run` builds with `cargo build`** (`CARGO_TARGET_DIR=out/rust/target`) and runs the
    binary; `wolf test` runs `cargo test` once per Cargo target (`--lib`, `--bin`, `--test`) with
    `--exact --test-threads=1`, and builds the common report from the test lines and the marked
    panic messages.
12. **Go: fields recursive through `Option` or `Result` are pointers**, `*wolfrt.Option[node]`
    (the Go analogue of item 5).
13. **Go: a copied interface gets a `Copy<Name>` method** (`CopyAnimal() Animal`) that every
    implementing type provides through its `Copy`. It is exported, so implementations in other
    packages can provide it.
14. **Go: a plain enum whose methods take a pointer receiver** becomes a pointer to a copy when
    it is used as an interface value.
15. **TypeScript: `match mut` on a list element** names the element with a `const`, so that
    TypeScript can narrow it; the object is the same, so changes of its payload stay in the list.
    **Driver: `--out` is resolved against the current directory**, because the tools run in the
    output directory.

## Reasons

1. Aliases would add a name per function type that the program never wrote; the lint is about
   style, not correctness.
2. One form for every boxed field keeps construction (`Box::new(value)`), reading and moving the
   same as for direct recursion. `Option<Box<Node>>` is more idiomatic and does not allocate for
   `None`, but needs a conversion at every `Some`, in patterns and in `?`.
3. Running every target at once would mix three outputs and three exit codes in one report.
4. Wolfpack's read mode is a borrow in Rust; `&str` and `&[T]` accept every string and list
   argument without conversions.
5. Rust needs a known size; a box is the smallest change that gives it.
6. Deriving only what the fields allow keeps the generated types valid for every program.
7. `Box<dyn Trait>` is the only form of a value whose type is unknown at compile time; `Clone`
   cannot be a supertrait of a trait object.
8. Wolfpack defines these cases as panics (chapter 7); rustc rejects them when it can prove them.
9. Each form is what clippy and a Rust programmer expect, so the code passes
   `cargo clippy -- -D warnings` on the examples and the run programs.
10. A generated project should already be formatted.
11. `cargo run` mixes Cargo's messages into the output; one `cargo test` per target keeps the
    order of the source and the test names exact.
12. Go rejects a struct that contains itself through a generic struct.
13. Go has no way to copy the value behind an interface without knowing its type.
14. A method with a pointer receiver is not in the method set of the value, and a constant has no
    address; a copy keeps the value semantics of copy types.
15. `(items[$i] as Status)` is not narrowed by TypeScript. A relative `--out` gave the binary
    path and `CARGO_TARGET_DIR` relative to the wrong directory.

## Consequences

- The examples and the programs of `tests/run/` pass `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings`; CI builds them with Rust 1.85.
- In Go, E0003 (internal error) remains for a copied interface value implemented by an enum with
  payloads or by a generic type.
- CI uses the current stable Rust for clippy; a new clippy lint can fail the end-to-end tests
  without a change in the compiler.
