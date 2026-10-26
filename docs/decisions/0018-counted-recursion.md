# 0018: Counting the calls of functions that can call themselves

Status: proposed (2026-10-26, with D8.16). The behaviour is in chapter 8, section 4.3; the items
below are [INTERNAL] choices recorded so that the author can object.

## Decision

1. **The IR marks the functions that can call themselves**: `Function::recursive` and the flag
   `recursive` of `ExprKind::Closure`, set at the end of lowering (`wolf_ir/src/recursion.rs`).
   Functions, methods, defaults of interfaces and closures are the nodes of a call graph; a node
   is marked when it lies in a strongly connected component with a cycle that can repeat without
   end (item 3). The components come from Tarjan's algorithm, written without recursion.
2. **A call that the IR does not resolve calls every function that could be behind it:**
   - a function value: every closure and every function used as a value whose type has the
     shape of the value's type (a type parameter, `Self`, `Never` and an interface fit any type);
     an immutable local that a closure initializes calls that closure only;
   - an interface method: every implementation and every default with that name, and what the
     implementations of std call (`List.to_string` calls `to_string` and `message`);
   - std and extern code: the function values among the arguments; std functions with bounds
     (`List.sort` where `T: Comparable`, `assert_eq` with `Display`) also call the methods of
     those interfaces, of the types that the arguments mention.
3. **Calls whose type cannot grow are not enough for a counter.** A call of a method of a type
   parameter of the method's own type (`Holder<T>.label` calling `label` of its `T`) reaches a
   type nested in the caller's, and a call of a method of `Self` in a default reaches the same
   type. A component is counted when it has any other call between its nodes, or a cycle of calls
   of `Self` only. Neither Go nor Rust lets a type grow from call to call (polymorphic recursion is
   an instantiation cycle in Go and endless monomorphization in Rust), so a cycle of the other
   kind ends after as many turns as its types have levels. That holds only while the program
   uses `Holder` with types it writes out: where it uses `Holder<Named>` with an interface, or
   `Holder<U>` with a type parameter of other code (which may stand for an interface), the value
   in `T` may be another `Holder`, as deep as the data, and the call into `T` counts as any call.
4. **Go**: `wolfrt.Enter()` and `defer wolfrt.Leave()` start the body; the count is a package
   variable, as a wolfpack program has one goroutine and `go test` runs the tests one after the
   other. The deferred `Leave` runs while a panic unwinds, so a test that overflowed leaves the
   count where it was.
5. **Rust**: `let _depth = wolf_rt::enter();` starts the body; the guard counts down in `Drop`,
   also while a panic unwinds. The count is a thread-local `Cell`, since `cargo test` runs tests on
   several threads. A closure that Rust writes as the function it calls (`|s| describe(s)` as
   `describe`) has no frame of its own and gets no guard.
6. **The limit is 100,000 nested counted calls**, `MAX_FRAMES` of the VM. `Enter` checks before it
   counts, so the call that overflows is not counted and needs no `Leave`.
7. **TypeScript**: the reporter of `wolf test` writes `panic: stack overflow` for a `RangeError`, as
   `report` of the runtime does for a program; it wrote Node's own message before.
8. **Tests**: `tests/run/stack_overflow` (exit code 101 after recursion 5,000 calls deep),
   `tests/run/deep_recursion` (50,000 calls deep through a function, two functions, an interface
   value and a function value; not on TypeScript, whose stack holds about 10,000 calls, so the
   program has a `skip-ts.txt`) and `tests/test/stack_overflow` (six ways to overflow, the last
   a wrapper of interface values 200,000 levels deep, then a test that recurses again).
   `tests/vm/stack_overflow` is gone: `tests/run/stack_overflow` runs in the VM too.

## Reasons

1. Counting every call would change all generated code and cost a counter on calls that cannot
   repeat; chapter 8 asks that code without recursion stays as it was. With these rules the
   examples and the end-to-end programs keep their code except where a function really can call
   itself: `binary_tree`, `recursive_data`, the closure of `deep_data` that calls a function value
   of its own type, and the generic wrappers of five programs that use them with interface values
   or from generic code.
2. Without item 3 every generic wrapper (`impl<T: Named> Named for Holder<T>`, a `to_string` that
   shows a field of type `T`) got a counter, in about a dozen end-to-end programs.
3. Where the graph cannot be precise it says too much: a counter where none is needed costs a few
   instructions, a missing one lets Go stop with its fatal error and Rust abort.
