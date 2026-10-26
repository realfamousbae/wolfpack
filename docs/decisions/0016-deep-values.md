# 0016: Values a million levels deep

Status: accepted (author, 2026-10-23, D4.14). Item 1 was decided by the author and is in
chapter 4, section 7.4, rule 3; the other items are [INTERNAL] choices recorded so that the author
can object.

## Decision

1. **`==`, `.copy()` and dropping work on values at least 1,000,000 levels deep**, on every target
   and in the VM. How deep the program's own functions may recurse stays unspecified (chapter 10,
   section 2, rule 7).
2. **The VM and the TypeScript runtime never recurse over a value.** Equality keeps a list of the
   pairs still to compare; a copy first copies a list, map or object with the parts of the original
   in it and then replaces each part by its own copy; in the VM, freeing an object, list, map or
   closure moves its parts into a list before it is freed (`Drop` for `Obj` and `Closure`).
3. **Go and Rust keep the code they had and get a deeper stack.** The Go entry wrappers and the test
   wrapper call `debug.SetMaxStack` with 2 GiB; the Rust ones run the program or the test on a
   thread with a stack of 2 GiB, then 512 MiB, then on the current thread, whichever the system
   gives first. The thread keeps the name of the thread it replaces, so a stack overflow still says
   `thread 'main'`.
4. **The Rust runtime `Map` has a written `Clone` and `PartialEq`**, plain loops instead of iterator
   adapters, so that a value deep through maps needs fewer frames per level.
5. **The test is `tests/run/deep_data`**: an enum chain, a struct deep through `List`, through
   `Option` and through `Map`, and a function value that holds a million others, each built by a
   loop and compared with its copy, on the three targets and in the VM.
6. **The VM crate is optimized in the dev profile** (`opt-level = 2`), so that the tests, which run
   whole programs in the VM of a debug `wolf`, take seconds instead of half a minute.

## Reasons

1. Before, the outcome of one program depended on where it ran: a loop that built a chain of 100,000
   links could copy it on Go and in the VM, panicked with `stack overflow` on TypeScript and
   aborted on Rust; with 3,000,000 links the VM aborted and lost all the output it had buffered.
2. Code that never recurses is cheap in the runtimes, which are ours and written once. In generated
   Go and Rust it would mean a hand-made `Drop`, `Clone` and `PartialEq` for every recursive type,
   much longer than the derived ones, against the rule that generated code reads as if a person had
   written it (chapter 8, section 1, rule 7).
3. The sizes were measured on the test: a debug Rust build needs about 700 MiB of stack for a
   million levels through `List`, and Go about 640 MB through `Map`, which Go's own limit refuses
   because a goroutine's stack doubles as it grows. 2 GiB leaves room for types with more fields.
4. The cost: only the part of a stack that a program reaches takes memory, but a program whose own
   recursion never ends now takes up to 2 GiB on Go and Rust before it stops (Go also copies its
   stack as it grows), where it took at most 1 GB on Go and 8 MiB on Rust. TypeScript and the VM
   stop it early as before. Since 0.6 Go and Rust stop it too, after 100,000 nested calls of the
   functions that can call themselves (D8.16, ADR 0018).
