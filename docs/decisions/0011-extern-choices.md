# 0011: Choices of `extern` (M11)

Status: accepted (author, 2026-10-16). Items 1 to 5 were decided by the author and changed
chapters 7 and 8; the other items are [INTERNAL] choices recorded so that the author can object.

## Decision

1. **Integer conversions at the boundary.** In Go an `Int` result may be of any Go integer type
   (`wolfrt.FromTargetInt`), `Int` parameters stay `int64`. In Rust `Int` parameters and results
   may be of any integer type (`wolf_rt::to_target_int`, `wolf_rt::from_target_int`). A failed
   conversion is `panic: integer too large for extern code` for a parameter and
   `panic: integer overflow` for a result, as in TypeScript.
2. **A Go opaque type may be a pointer**: `type Regex from "*regexp.Regexp"`.
3. **`wolf run` and `wolf test` fetch the dependencies**: `npm install` for TypeScript; Go and
   Cargo download modules and crates themselves.
4. **The example of M11 is `fuzzy`**, with `fastest-levenshtein` (npm),
   `github.com/agnivade/levenshtein` (Go) and `strsim` (crates.io). Its end-to-end tests fetch the
   packages, locally and in CI.
5. **E0803** reports a failed `npm install` with the output of npm (chapter 8, section 9).
6. **Each extern function is a wrapper** in the generated code: a function with the wolfpack name
   that converts the arguments, calls the target and converts the result. Calls in user code
   stay ordinary calls.
7. **Opaque types are type aliases**: `export type Pattern = RegExp`, `type Pattern =
   *regexp.Regexp`, `pub type Pattern = std::path::PathBuf;`. A struct with an opaque field gets no
   generated equality or copy.
8. **Method paths call the first parameter**: `p0.MatchString(p1)` in Go, `p0.test(p1)` in TS; Rust
   keeps the function form (`std::path::Path::starts_with(p0, p1)`), which deref coercion accepts.
9. **Results of the target's own types**: Rust converts strings with `.into()` (so `&str` and
   `Cow<str>` work), lists with `into_iter().map(..).collect()` (so iterators work), `Option` with
   `map`, `Result` errors with their `Display` text (`wolf_rt::extern_error`); the wrapper allows
   `clippy::useless_conversion`. TS turns an exception into `Err` when the result is a `Result`,
   and `null` into `None`.
10. **Target binding (E0704) is checked when building**, for every module of the build: the roots
    (the entry module, the examples or the tests) and the modules of `src/` that no root imports.
    The error is reported at the `extern` section of the module that misses a declaration, with
    the import chain from the root.

## Reasons

1. Libraries return `int` and `usize`; a Go conversion of a parameter needs the parameter's type,
   which wolfpack does not know.
2. Many Go constructors return pointers.
3. A project with dependencies cannot build without them.
4. Levenshtein distance is one function from two strings to an integer in all three ecosystems.
   SHA-256 needs byte arrays, which do not cross the boundary.
5. A missing package or network is not a bug of `wolf`, so E0802 would mislead.
6. One place per function holds the conversions; the rest of the program does not know that the
   function is extern.
7. An alias keeps the name the program uses and the type the target knows.
8. A method call is how a person writes it in Go and TS.
9. The target's function decides the exact type; these forms accept the common ones without
   knowing it.
10. A module the build contains must compile, even if nothing imports it.

## Consequences

- Known gaps: Go cannot convert `Int` parameters or lists of integers of other types; a Go
  function taking `int` needs a wrapper package. TypeScript has no Node.js type declarations, so
  Node globals such as `Buffer` and `node:` modules are not available to extern declarations.
- Rust calls use full paths (`strsim::levenshtein(..)`) instead of the `use` declarations that
  chapter 8, section 8.1 shows; the behaviour is the same.
