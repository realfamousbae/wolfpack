# 0009: Choices of the Go backend

Status: accepted (author, 2026-10-13). Items 1 and 2 changed the text of chapter 8; the other
items are [INTERNAL] choices recorded so that the author can object. Items 10 and 11 were added
on 2026-10-24, item 12 on 2026-10-25, also [INTERNAL].

## Decision

1. **Indexing uses runtime helpers** (`wolfrt.At`, `wolfrt.SetAt`, `wolfrt.RefAt`) instead of
   native `items[i]` with a translated panic message (chapter 8, section 4.3, table).
2. **Function values return `Option` and `Result` in the data form** in Go too, as rule 4.1.4 of
   chapter 8 says for TypeScript. A declared function with an idiomatic result (`(T, bool)`,
   `*T`, `(T, error)`, `error`) is wrapped when it is used as a value.
3. **std has no Go packages.** Its declarations (`Ordering`, `Error`, `ParseError`, `Entry`,
   `FsError`, the constants of `std.math`) and its intrinsics are part of the runtime package
   `wolfrt`, written by hand and emitted piece by piece.
4. **Variants of an enum with payloads are pointers** to their structs (`&ShapeCircle{F0: r}`), and
   `match` on such an enum is a type switch.
5. **Dictionaries are plain functions**: `eqT func(T, T) bool`, `copyT func(T) T`,
   `cmpT func(T, T) wolfrt.Ordering`, passed before the other arguments. `Hashable` becomes the
   constraint `comparable`, interfaces of the program become constraints. The type arguments of a
   call are found by matching the signature against the types of the arguments and the result.
6. **`Equal` and `Copy` methods** are generated for structs and enums whose `==` or `.copy()` the
   program uses, and only for them.
7. **Floating-point operations on two constants** go through `wolfrt.FloatAdd` and its siblings,
   and `-0.0` is `math.Copysign(0, -1)`.
8. **`wolf test` runs `go vet ./...`, then `go test -json`**, and prints the common report from
   the events. A test runs its body in `wolfrt.Test`, which turns a panic into a failure and prints
   the panic message on a marked line. Tests are ordered by the order of the source, not by Go.
9. **`wolf run` builds with `go build -o out/go/bin/<name>`** and runs the binary, with
   `GOTOOLCHAIN=local` and `GOWORK=off`.
10. **A bound by an interface with a `mut self` method is a dictionary of views**, not a
    constraint: `T: Step` is a parameter `asStepT func(*T) step` that gives the place of a value
    as a value of the interface, whose methods change the place. It is
    `func(p *light) step { return p }` for a struct or a payload-less enum, the ref of an enum
    with payloads, and `*p` for an interface value. An interface implementation, which keeps the
    signature of the interface, finds it at run time instead (`wolfrt.As[step, T]`). An enum with
    payloads that implements such an interface has a **ref type**, `shapeRef{p *shape}`, with the
    methods of those interfaces; the enum's own Go interface does not have them, and a value of
    the interface is a ref to a new variable. An interface that mentions `Self` beyond the
    receiver takes the implementing type as its last type parameter (`cycle[Self]`): a bound by
    it is the constraint `T weigh[int64, T]`, or the dictionary `func(*T) cycle[T]` when it has
    a `mut self` method, and its defaults are generic over `Self`.
11. **Bounds that Go cannot state are found at run time.** A bound by an interface of the
    program that only the head of an implementation adds (`Bag<T: Named>: Named` with
    `struct Bag<T>`) leaves the method a Go method, which the interface needs, and finds the
    value as a value of the interface with `wolfrt.As[named, T]`. The type parameters of an
    interface have no constraints by interfaces of the program (`type holds[T any] interface`),
    and its defaults find those bounds the same way.
12. **A chain of conditions** (`if let ... && ...`) is an `if` per item around the rest; a `let`
    on an idiomatic result is `if x, ok := f(); ok`. When the first block can end without
    leaving, it sets a flag, `matched := false ... matched = true`, and the `else` branch runs in
    `if !matched`. **Super-interfaces are embedded** in the Go interface; a constraint leaves out
    the bounds that an embedding implies, and a constraint of several interfaces is written one
    per line, as gofmt does.

## Reasons

1. Go's message for a negative index (`index out of range [-1]`) has no length, so it cannot be
   translated into the canonical `index -1 out of range for length 2`. The helpers give the exact
   message on every path, including assignments and `mut` arguments.
2. What a function value returns is usually stored (`list.map(|n| halve(n))`), and Go function
   types cannot express `(T, bool)` as one value that a generic function such as `wolfrt.ListMap`
   could pass on.
3. Module names of std such as `map` and `string` are not valid Go package names, `math` and `io`
   would collide with Go's own packages, and the runtime needs `Ordering` without import cycles.
4. A `match mut` changes a payload in place (`Status.Snoozed(left) => left += days`); a value
   stored in an interface cannot be changed, a pointer can. The ownership check guarantees that no
   two values share a pointer observably.
5. One Go constraint cannot be satisfied both by `int64` and by a struct with a method (decisions
   register item 14). Functions are the simplest dictionaries and can be method expressions
   (`Version.Compare`, `Task.Equal`).
6. Generating these methods for every type would add code to programs that never compare or copy.
7. Go evaluates constant expressions exactly: `0.1 + 0.2` would be `0.3`, `1.0 / 0.0` would not
   compile, and constants have no negative zero.
8. `go vet` compiles every package with its tests; a failure is a compiler bug (E0802). The JSON
   events give the result of each test without parsing the human output of `go test`.
9. `go run` replaces the exit code of the program by its own.
10. Only the pointer of a type has its `mut self` methods, so no constraint on `T` holds them.
    Go's `[T any, PT interface{ *T; step }]` covers structs and payload-less enums, but an enum
    with payloads is a Go interface: a pointer to an interface has no methods, and a variant
    cannot replace itself in the variable. A dictionary covers every type, as chapter 8, section
    5.3 has it for Go bounds; the ref is the one type an enum with payloads needs, both for its
    values of the interface and for its dictionaries.
11. A Go method cannot constrain the type parameters of its receiver, and `bag[T any]` cannot
    name `holds[T]` when `holds` requires `T named`. The program's type checker has already
    checked the bounds, so the assertion of `wolfrt.As` cannot fail.
12. Go has no block that a `break` leaves without leaving a loop around it, and `goto` cannot jump
    over the declarations of the bindings; a flag keeps the `else` branch written once.

## Consequences

- The generated code is idiomatic Go where the language allows it: `(T, error)` results,
  `if v, ok := f(); ok`, type switches, methods with pointer receivers, `for i := range` loops.
- A stack overflow is fatal in Go and cannot be turned into a panic with the canonical message.
