# 0013: The entry point is a `main` section

Status: accepted (author, 2026-10-17). Items 3 and 4 are [INTERNAL] choices recorded so that the
author can object.

## Decision

1. **`main` is a section, not a function.** It stands between `impl` and `functions` and holds the
   statements of the program: `main { ... }`. `fn main` in `functions` is an error (E0214); the old
   rule "`fn main` is the last declaration" (E0213) is retired.
2. **The header is flexible.** `main`, `main(args)`, `main -> Result<Void, Error>` and
   `main(args: List<String>) -> Result<Void, Error>` are all valid. The only parameter is the list
   of command-line arguments, the same value as `std.io.args()`; its type may be left out. Anything
   else is E0614.
3. **The parser desugars the section into an ordinary `fn main`** (a `Section` of kind `Main` with
   one `FnDecl`), so name resolution, type checking, ownership and the IR are unchanged. A
   parameter written without a type gets an empty type span, which `wolf fmt` uses to print what
   was written.
4. **The entry wrappers pass the arguments.** The runtime of each target has `runMain` (TS, always
   passes the arguments), `RunMainArgs` and `RunMainArgsResult` (Go), `run_main_args` and
   `run_main_args_result` (Rust), next to the existing runners.

## Reasons

1. The entry point is the first thing a reader looks for; as a section above everything else it
   is at the same place in every program, and it needs no `fn`, name or empty parameter list.
2. Programs that ignore the arguments and the result pay nothing for them.
3. Desugaring keeps the change in the parser, the formatter and one check.
