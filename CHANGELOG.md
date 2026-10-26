# Changelog

All notable changes are recorded here at the end of each milestone.

## Unreleased

- Fixed: the tests of `tests/` of a `cli` program did not build on Go and Rust (spec chapter 8,
  section 2.1, rule 2). On Go they are now the external test package `main_test` in the root
  directory (`mylib_test` for a library), which may import the package `main`, in files named
  `tests_<path>_test.go`, so that `tests/main.wolf` no longer overwrites the test file of
  `src/main.wolf`; helpers of different files of `tests/` no longer collide in that one package.
  On Rust, which cannot use a binary crate from Cargo's integration tests, they are modules
  `crate::tests::<path>` in `src/tests/`, declared in the `#[cfg(test)] mod tests` of
  `src/main.rs`. A Rust test function whose name a function or an import of its scope already
  has (`test "twice"` next to the imported `twice`, also in the `tests` section of a module of
  `src/`) gets a `_` suffix: before, the test function hid the function it called.
- Fixed in the Rust backend: an example or a test of `tests/` of a library that imports a name
  declared in `src/lib.wolf` itself (not re-exported from another module) named it
  `crate::<name>` instead of `<library>::<name>`. A test project now covers such names in
  `tests/` and `examples/` on every target and in the VM, and the guide checks the report of
  `wolf test` for the `shouter` library of chapter 11 on every target.
- Fixed in the Rust backend: a file in a directory of `tests/` of a library, such as
  `tests/deep/inner.wolf`, is declared as a `[[test]]` in `Cargo.toml`, since Cargo does not find
  it by itself; `wolf test` failed with "no test target named `deep_inner`".
- Fixed in the Go backend: a test that wrote text without a final line feed (`io.write("abc")`)
  and then failed was reported as `test failed`, since the failure marker was glued to that
  text; the panic message is reported again.
- Fixed in the TypeScript backend: the `.test.ts` file of a removed or renamed file of `tests/`
  stayed in `out/ts/test/`, and the next `wolf test` could fail in `tsc`. Go and Rust already
  removed such files.
- Fixed in the Rust backend: `print("")` was printed as `println!("")`, which clippy rejects, and
  a test whose body is only `assert_eq` got an extra block that `rustfmt --check` removes. The
  test project `rust_style` checks both with clippy and rustfmt.

## 0.5.1 (2026-10-26)

- E0608 also covers the super-interfaces of a `pub` interface (spec chapter 6, section 4, rule 5):
  a `pub` interface cannot extend a private one, and the message names the interface as an
  interface.
- `wolf self uninstall` on Windows removes `wolf.exe` and the directories of the installer
  script: the command that removes them after `wolf` exits was quoted for a program, not
  for `cmd`, which rejected it. CI now runs the tests of `wolf self uninstall` on Windows and
  macOS too, the Windows one with the user's `Path` in the registry.

## 0.5.0 (2026-10-25)

- Chains in conditions (spec chapter 3, section 5, rule 14a; D3.10 and D5.9): `if` and `while`
  join `let` items and `Bool` items with `&&`, as in `if let Some(user) = find(id) &&
  user.active && let Some(mail) = user.mail`. The items are tried from left to right, the
  bindings of a `let` are visible to the items after it and to the first block, and an `||`
  outside parentheses in a condition with a `let` is the new error E0317. Each `let` keeps its
  borrow through the later items and the block (E0509), and a value that a later item moves may
  have been moved in the `else` branch (E0502). The IR has a node of its own for a chain,
  `IfChain`, so that the `else` branch is written once: TypeScript emits a labelled block,
  Rust a labelled block left with `break 'chain_ value` (and labels the loops that a `break`
  inside it leaves, E0695), Go a flag when the first block can end normally, and the VM jumps.
  `wolf fmt` breaks a long chain after every `&&`.
- Super-interfaces (spec chapter 4, sections 8.2 rule 6, 8.3 rule 7 and 8.5 rule 1; D4.20):
  `interface Greeter: Named + Display`. A type that implements `Greeter` implements each
  super-interface in its own block (E0446 otherwise, also when the head of a generic block lacks
  a bound that the implementation of the super-interface needs), and wherever a type is known to
  implement `Greeter` it has their methods: in defaults, through bounds and on values of
  `Greeter`, which also meet a bound by `Named`. Super-interfaces are interfaces of the program,
  `Error`, `Comparable` and `Display`; `Equatable`, `Copyable` and `Hashable`, cycles and repeats
  are E0445, and a method with the name of a method of a super-interface is E0447. An interface
  with `Comparable` among its super-interfaces is a bound only, and a value of `Greeter` is not
  converted to a value of `Named` (E0401). The IR keeps the super-interfaces of each interface
  and adds those of a bound to it (`T: Greeter + Named + Display`), so that TypeScript and Go
  pass the dictionaries the calls need; TypeScript writes `extends`, Go embeds the interfaces,
  Rust writes super-traits and `impl Named for Box<dyn Greeter>`, and each drops the bounds that
  others imply.
- `wolf check --bytecode [<name>]` (spec chapter 10, section 1a; D10.9), a disassembler: it
  prints the bytecode that the VM runs for the functions of `src/`, in the order of the source
  and each closure after its function (`shapes.area closure 1`), or only for the functions named
  `area`, `Shape.area`, `shapes.area` or `main`. A module that cannot run in the VM is a comment
  that says why; a name that names no function is E0002. The functions of the VM now have their
  full names (`main.sum`) in listings and internal errors.
- `wolf self uninstall [--yes]` (spec chapter 8, section 2c; D8.15): removes `wolf`, the
  `PATH` lines that the terminal scripts marked with their comment (never other lines) and the
  entry in the user's `Path` on Windows, and asks before it removes the wolfpack home, which
  `--yes` removes without asking and a script keeps; only what `wolf` put there goes. For a `wolf`
  of a package manager it prints the command of the manager, for the Windows setup program it
  starts its uninstaller, and on Windows the renamed `wolf.old.exe` is removed by a hidden command
  after `wolf` exits. A step that fails, or a directory that cannot be written such as
  `/usr/local/bin` of the macOS package, is E0808 with the commands that remove it.
- Fixed in the Rust backend: a block that implements an interface and defines no method, such as
  one that takes every default (`Dog: Named {}`), wrote no `impl`, so the type did not implement
  the trait. Go writes a constraint of several interfaces with one element per line, as gofmt
  does, and `wolf fmt` writes a block that defines nothing as `Dog: Named {}`.

## 0.4.1 (2026-10-24)

- Fixed in the Go backend, which now runs what the other targets and the VM ran in 0.4.0 (the
  `skip-go.txt` files are gone): a type parameter bounded by an interface with a `mut self`
  method (Go rejected it: the methods with pointer receivers are not in the method set of the
  type), an interface that mentions `Self` beyond the receiver (an internal error), and a `mut
  self` method of an interface implemented by an enum with payloads (a variant cannot replace
  itself). Such a bound is now a dictionary, `asStepT func(*T) step`, that gives the place as a
  value of the interface; an enum with payloads gets a ref type, `shapeRef`, which points to its
  variable and has the methods of those interfaces; an interface with `Self` is generic over it,
  `cycle[Self]` (ADR 0009, item 10). A `mut self` default called by the interface's name on a
  field, an element or a `mut` parameter (`Step.twice(mut self.light)`) changed a copy.
- Fixed in the Rust backend: a struct literal of a generic struct bound by `let` or `var` lost
  the written type, so a field value coerced to an interface (`let h: Holder<Step> = Holder {
  item: Shape.Line(2) }`, also inside a generic enum or another struct literal) made a
  `Holder<Box<Shape>>`, and rustc rejected it where a `Holder<Box<dyn Step>>` was expected. The
  `let` now writes the type, as for a generic variant or call.
- Fixed: a bound that only the head of an interface implementation adds (`Bag<T: Named>: Named`
  with `struct Bag<T>`, also for `Display` and `Comparable`). Go made such a method a function of
  the package, so the type did not implement the interface; it is a method again and finds the
  value as a value of the interface at run time (`wolfrt.As`), and the type parameters of an
  interface have no Go constraints by interfaces of the program, so that `holder[H any]` can
  implement `holds[H]` (ADR 0009, item 11). TypeScript cast the value, which a payload-less enum
  failed at run time, and wrote its `Display` without its own `to_string`; the value now gets
  the dictionaries of the head where its type is known (`$rt.withDicts`).
- Fixed on TypeScript and Go: a type parameter that only the bound of another one names
  (`fn show<T: Named, H: Holds<T>>(h: H)`) got no dictionary on TypeScript and `struct{}` on Go;
  it is now found from the interface arguments of the other one, also when that one is a type
  parameter itself and when the function is a value.
- Fixed in the Go backend: `Some` of a value coerced to an interface (`let o: Option<Named> =
  Some(Shape.Line(4))`) made an option of the value's own type, which Go rejected where an
  option of the interface was expected.
- Fixed in the Rust backend: a map literal whose values are coerced to an interface (`["a":
  Shape.Dot, "b": Dog { title: "Max" }]` as `Map<String, Named>`) took the type of its first
  value; such values, and closures, are now cast to the value type, as in a list.
- Spec chapter 8, sections 5.1 to 5.4, describe the mapping of interfaces and bounds as it is
  built: the receivers of Go methods, the ref types of enums with payloads, interfaces that
  mention `Self`, and the dictionaries of Go and TypeScript.

## 0.4.0 (2026-10-24)

- Fixed in the Rust backend: an interface value passed for a type parameter bounded by its
  interface (`fn hello<T: Named>(x: T)` called with a `Named`) was rejected, because
  `Box<dyn Named>` did not implement the trait; it now does, by calling the value it holds. A
  generic struct with an interface bound that implements an interface (`Labeled<T: Named>:
  Named`) was rejected too (its `copy_dyn` needs `T: Clone`), and a generic function used as a
  function value failed Clippy's `redundant_closure`.
- Fixed in the Go backend: a `where T: Named` bound by a user interface on a method of a generic
  type gave Go code that calls `x.Name()` on `T any`; such a method is now a function of the
  package with the constraint. A type with a `mut self` method no longer gives every method a
  pointer receiver, so its values meet a bound by an interface of its other methods. A `mut
  self` default called on a payload-less enum, `Step.twice(mut light)`, changed a copy.
- Fixed in the TypeScript backend: a `mut self` method of an interface called on a payload-less
  enum (a default it gets, or `Step.twice(mut light)`) changed a wrapper and not the variable;
  the wrapper now holds the place (`$rt.viewOf`).
- Fixed in the TypeScript backend: a payload-less enum passed for a type parameter bounded by a
  user interface (`hello(Color.Red)` with `fn hello<T: Named>(x: T)`) was rejected by tsc,
  because the enum is a string. Such a bound is now a dictionary parameter, `$asNamedT`, that
  gives the value as a value of the interface (the enum's wrapper class, or the value itself), so
  methods, defaults and `Named.greeting(x)` work through `T`, generic interfaces too, and `T`
  stays the enum in lists, comparisons and results. With `E: Error`, `f"{r}"` of a
  `Result<Int, E>` writes the enum's message, or its `to_string` if it is also `Display`. The
  dictionary of an interface with a `mut self` method takes the place of the value, so that
  `x.step()` through `T` changes `x`, also an element of a list or a field. An implementation of
  an interface by a generic type finds the dictionaries of the type's bounds in the value
  (`Labeled<T: Named>: Named` calling `self.item.name()`, `Shown<T: Display>: Display` writing
  `self.item`): they are fields such as `$asNamedT`, which `==` skips. A default of an interface
  with a bounded type parameter (`interface Holder<T: Named>`) takes them after the receiver.
  Still a cast, right for classes only: a bound that only the head of an interface
  implementation adds (`Box<T: Named>: Named` with `struct Box<T>`).
- Fixed: interfaces that mention `Self` beyond the receiver (`fn same(self, other: Self) -> Bool`,
  `fn next(self) -> Self`), which are bounds only. TypeScript rejected a class that implements one
  whose method returns `Self` (TS2416), and a payload-less enum could not meet such a bound; the
  interface is now `interface Same<Self>`, which a class implements as `Same<Dog>`, a bound by it
  is a dictionary like the others, and its defaults are generic over `Self`. Rust rejected the
  defaults that keep `Self` in a list or replace `self` (`self = self.next()`); such a trait is now
  `Sized`.
- Fixed in the Rust backend: a generic interface used as a value (`let h: Holder<Color> = ...`)
  lost its type arguments in `copy_dyn` and `Clone` (`Box<dyn Holder>`); and a `mut` list
  parameter whose body changes only its elements failed Clippy's `ptr_arg`.
- Fixed in the TypeScript backend: a `mut` argument that is a field of `self` in a method of a
  struct (`self.light.step()` with a `mut self` method of a payload-less enum) read and set the
  field of the reference object itself, whose getter sees `this` as the reference; and a `mut
  self` method of a payload-less enum that calls another one on `self` stopped `wolf` with an
  internal error.
- Not yet on Go (the end-to-end programs that need them have a `skip-go.txt` that says so): a type
  parameter bounded by an interface with a `mut self` method, an interface that mentions `Self`
  beyond the receiver, and a `mut self` method of an interface implemented by an enum with
  payloads. The other targets and the VM run them.
- Default methods in interfaces (spec chapter 4, sections 8.2 and 8.3; D4.19): a method of an
  interface may have a body, which a type that implements the interface gets when its block
  leaves the method out; a replacement is written `override fn`, and `Named.greeting(x)` calls
  the default itself, also from a replacement. The default is checked once over `Self`, and the
  defaults a type gets count as its methods for E0415 and E0432. New diagnostics: E0443 for a
  replacement without `override` and an `override` that replaces nothing, E0444 for
  `Interface.method(x)` of a method without a default. Rust keeps the default as a trait method
  `greeting_default`, Go and TypeScript as a function per default with a method in each type that
  gets it, and the VM dispatches to it.
- Fixed in the Rust backend: an implementation of a generic interface, `Pair: Holder<Int>`, was
  written without the interface's type arguments.
- Fixed: a list, a map, an option or a result inside a generic implementation of `Display`
  (`Box<T: Display>: Display` holding `[1, 2]`) was written `<1,2>` by TypeScript and stopped
  the Go program, because such a method has no static type for its value; both now write it as
  everywhere else, `<[1, 2]>`, finding the type at run time.
- Format specifications in interpolations (spec chapter 1, section 5.3a; chapter 7, section 4.4;
  D4.18): `f"{price:>8.2}"`, `f"{n:05}"`, `f"{name:*^10}"`. A width counts Unicode scalar values,
  numbers align to the right and other values to the left, a precision rounds a `Float` to the
  nearest with ties to even on its exact value (TypeScript, whose `toFixed` rounds halves up, has
  its own code), and `0` pads a number with zeros after its sign. New diagnostics: E0117 for a
  malformed specification, a `0` with a fill or an alignment, a width over 1000 or a precision
  over 100, and E0441 for a precision on a type other than `Float` or a `0` on a type that is not
  a number.
- Fixed in the Rust backend: a method of a negative number literal, `(-7).to_string()`, lost its
  parentheses; and a program that writes a value close to π (`3.14159`) no longer fails Clippy's
  `approx_constant`.
- `Display` of collections (spec chapter 4, section 9, rule 7; chapter 7, section 4.1; D4.17):
  `List<T>` and `Option<T>` are `Display` when `T` is, `Map<K, V>` when `K` and `V` are, and
  `Result<T, E>` when `T` is and `E` is `Display` or implements `Error`. They are written as the
  literal that would build them: `[1, 2, 3]`, `Some("a")`, `["Ann": 31]`, `[:]`, `Ok(12)`,
  `Err(cannot parse x)`; a `String` inside is in quotes with escapes, and a value of the interface
  `Display` is written by its own `to_string`. So `f"{items}"`, `items.to_string()` and
  `assert_eq` of two lists work, on every target and in the VM. In generic code the type argument
  decides. A collection whose contents are not `Display` is E0404 (E0401 for a coercion), with
  a note that names what it needs.
- Methods with bounds of their own (spec chapter 4, section 8.6; D4.16): a method of a generic
  type may write `where T: Equatable` after its result type and then exists only for the types
  that have the bound; a call with another type is E0404 (E0406 for `Equatable` and `Copyable`).
  The new diagnostic E0442 is a `where` that is not allowed: on a free function, in an interface
  or an interface implementation, or naming a parameter that is not one of the `impl` block.
  **Breaking**: `contains`, `index_of`, `get`, `sort`, `min` and `max` are now methods of
  `List<T>`, and the free functions of `std.list` with these names are removed:
  `list.sort(mut items)` is `items.sort()`, `list.contains(items, x)` is `items.contains(x)`.
  `sort_by`, `sum` and `range` stay functions of `std.list`.
- Fixed in the Rust backend: a list literal of two or more closures (`[|| 1, || 2]`) outside a
  typed `let`, and closures in two fields of one type parameter (`Pair { first: || 1, second:
  || 2 }`), were rejected by rustc, because each closure has its own type; `contains` and
  `index_of` of a `List<String>` with a string literal (`words.contains("fig")`) were rejected
  too.
- Fixed: an interface implementation whose head adds bounds (`Box<T: Display>: Display`)
  counted for every type argument, so `f"{Box { value: || 1 }}"` was accepted. It is now E0404
  (E0401 for a coercion), with a note that names the bounds.
- Fixed in the Go backend: an interface implementation of a generic type whose parameters have
  the bounds `Display`, `Comparable`, `Equatable` or `Copyable`, from the head of the
  implementation (`Box<T: Display>: Display`) or from the type's declaration (`struct Sorted<T:
  Comparable>`), took their dictionaries as extra parameters, so the type did not implement the
  interface and Go rejected the code. The method now keeps the signature of the interface and
  finds these operations from the type argument at run time (`wolfrt.Show`, `Compare`, `Equal`
  and `Copy`).
- Fixed: type arguments written after a type and a dot (spec chapter 3, section 5, rule 2) were
  ignored: `let e = List.<Int>.new()` with nothing else in the body that tells the type was
  E0420, and so were `Map.<String, Int>.new()`, `Stack.<Int>.new()` of a user type,
  `Tree.<Int>.Leaf` and `Option.<String>.None`. They now give the type its arguments; a wrong
  number of them is E0403, and an unknown associated function after them
  (`Stack.<Int>.empty()`) is reported once, as E0609, not also as E0429.
- Fixed in the Rust backend: a `let` of a generic type of the program built by a call or by a
  variant without a payload (`let s: Stack<Int> = Stack.new()` followed only by `s.size()`,
  `let t: Tree<Int> = Tree.Leaf` followed only by a `match`) was rejected by rustc, which could
  not infer the type argument; the `let` now writes its type.
- Package managers (spec chapter 8, sections 2a and 2b; `docs/plans/distribution.md`): Homebrew
  (`brew install realfamousbae/wolfpack/wolfpack`) and Scoop (the bucket
  `realfamousbae/scoop-wolfpack`) have 0.3.0 already; winget manifests are ready for its first
  pull request. Each release now also has static Linux builds (`...-musl.tar.gz`), which
  `install.sh` picks on Alpine and on systems older than Ubuntu 22.04, and `.deb` and `.rpm`
  packages of them. The new workflow `packages.yml` updates the tap and the bucket and opens the
  winget pull request when a release is published. `wolf self update` names the command of
  Scoop, winget and the system's package manager instead of replacing their `wolf`.
- `wolf lsp`, a language server for editors (ADR 0017): the diagnostics of `wolf check` as you
  type, also before saving, the declaration and doc comments of a name on hover, go to
  definition, and formatting with `wolf fmt`. The new crate `wolfpack-lsp` holds the protocol
  code. `editors/vscode/` is an extension for VS Code with highlighting of `.wolf` files that
  starts it.
- The playground runs programs in the VM of the compiler, in a worker, instead of transpiling the
  generated TypeScript with a TypeScript compiler loaded from the network: runs start at once,
  the output appears as it is written, a program that runs too long is stopped after 5 seconds
  with its output kept, and the new box *Input* gives the lines that `io.read_line()` reads
  (ADR 0012). `wolf_wasm::run` and `run_live` are the new functions of the compiler for it.
- `==`, `.copy()` and dropping work on values at least 1,000,000 levels deep on every target and in
  the VM (spec chapter 4, section 7.4, rule 3; ADR 0016). Before, a chain of 100,000 links built by
  a loop could not be copied on TypeScript, aborted on Rust, and 3,000,000 links made the VM abort
  and lose its buffered output. The VM and the TypeScript runtime no longer recurse over values;
  the Go and Rust runtimes give the program a stack of 2 GiB. The new test `tests/run/deep_data`
  runs on the three targets and in the VM.
- Fixed in the Go backend: a private type and a private function whose names differ only in the
  case of the first letter (`Chain` and `chain`) were both `chain`; the later one is now `chain_`
  (spec chapter 8, section 3, rule 4).
- Fixed in the Rust backend: `let _ = f()` where `f` returns a function value is `let _ = f();`,
  without rustc's warning about an unused closure; a single tuple, call or struct literal in an
  array is laid out as rustfmt lays it out (`[(` ... `)]`).
- Fixed in the TypeScript backend: `self = ...` in a `mut self` method of an enum stopped `wolf`
  with an internal error. A method of a payload-less enum now takes a reference to the place of its
  receiver, and an object of an enum with payloads takes the new variant in place
  (`$rt.become`). A payload-less enum used as a value of an interface is an object of a small
  class `Color$Animal` that holds the value, instead of an object of closures, so that its
  `mut self` methods and its copies work. The program `plain_enum_interface` moved to
  `tests/run/`, so it runs on every target and in the VM.
- The generated Rust passes `cargo clippy -D warnings` also when the program drops values or
  computes with literals: a dropped value without effects is `let _ = ...` (rustc wants the
  values of `#[must_use]` methods used, clippy wants statements to do something),
  `3.to_float()` is the literal `3.0`, and the crate allows `eq_op`, `zero_divided_by_zero` and
  `match_single_binding`, which judge what the program wrote (`0.0 / 0.0` is how it writes NaN).
  The new test `tests/run/discarded_values` checks it.
- Native modules in TypeScript: a method of a class declared more than once (overloads) is not
  available, as an overloaded function is not; before, its last declaration was taken. A method
  with a `Map` whose keys stay `number` is not available either, as such a function is not
  (`wolf check --native` lists both with the reason).
- Fixed: the bounds that the head of an `impl` block adds (`Stack<T: Equatable> { ... }`, spec
  chapter 3, section 2, rule 6) were not checked when a method of the block was called, so a
  program could call it with a type that does not have them: the VM and TypeScript then compared
  functions, Go stopped with an internal error and Rust rejected the code. Such a call is now
  E0404 (E0406 for `Equatable` and `Copyable`), and the label of these errors no longer says
  "required by `f` needs `Equatable`".
- New diagnostic E0439: a struct literal or a struct pattern whose name is not a struct, an alias
  of one or a variant with named fields (`String { w: 1.0 }`, `Shape { w: 2.0 }` for an enum).
  Before, such a program stopped `wolf` with an internal error. Literals and patterns written with
  an alias of a struct (`type P = Point`, `P { x: 1, y: 2 }`) now build and match the struct, as
  chapter 4, section 7.3 says; before, they stopped `wolf` too.
- Fuzzing of the whole compiler: `crates/wolf_wasm/tests/mutations.rs` sends programs a token or a
  line away from every `.wolf` file of the repository through the check, the three backends and
  the formatter, and checks that formatting is idempotent and keeps the diagnostics
  (`WOLF_MUTATIONS=200` for a long run). The new fuzz target `pipeline` does the same with
  libFuzzer and `fuzz/wolf.dict`; it runs on stable Rust with `cargo fuzz run -s none`. The
  one-file pipeline of the playground moved to `wolf_driver::single`.

## 0.3.0 (2026-10-22)

- `wolf run --vm` and `wolf test --vm` run a program and its tests in a virtual machine inside
  `wolf`, without Node.js, Go or Rust (spec chapter 10). `wolf` compiles the IR into a stack
  bytecode of its own (the new crate `wolfpack-vm`); the output, panic messages and exit codes are
  those of the targets, and unbounded recursion is `panic: stack overflow` with exit code 101.
- E0807: a program whose modules call code of the targets (`extern` or native modules) cannot run
  in the VM; the error names the module and the chain of imports.
- Differential tests: every example and end-to-end program also runs in the VM and must give the
  output it gives on the targets; new programs `places` (every kind of `mut` place), `dispatch`
  (interface values, enum keys, a panic in a callback) and `tests/vm/` (stack overflow, nesting of
  callbacks, standard input, arguments, files, `exit`).
- Fixed in the Rust backend: a method of a negative literal (`(-5).abs()`), a `mut` field of a list
  element (`bump(mut items[0].count)`), `println!` arguments laid out as rustfmt does, and clippy
  on a `match` with one useful arm, on `any` with `==` and on loops that look for an element.

## 0.2.0 (2026-10-21)

- The entry point is a `main` section between `impl` and `functions`: `main { ... }`, or
  `main(args)` and `main -> Result<Void, Error>` when the program needs them (spec chapter 2,
  section 3.7; ADR 0013). `fn main` in `functions` is now an error (E0214); E0213 is retired.
- Interpolated strings `f"{name} has {items.len()} items"`, raw strings `r"\d+"` and multiline
  strings `"""` whose indentation is that of the closing quotes (spec chapter 1, section 5.3a).
- The built-in interface `Display` (`to_string`), implemented by `Int`, `Float`, `Bool`, `String`
  and by your own types; interpolated strings and `assert_eq` use it.
- Destructuring `let Point { x, y } = p`, `if let` and `while let`.
- `loop` with `break value`, and labelled loops: `outer: for ...`, `break outer`, `continue outer`.
  `loop` is now a reserved word.
- The update syntax `User { name: "B", ..old }`.
- `error(message)` makes an `Error` without a type of its own; `assert_eq(a, b)` shows both values
  when it fails.
- New diagnostics E0114 to E0116, E0315, E0316 and E0433 to E0438.
- The example `syntax-tour`; the guide shows the new forms in chapters 2, 3, 5, 7, 9 and 11.
- `wolf fmt` keeps the content of `"""` strings and moves them with the code.
- `wolf self update` installs the latest release (or `--version`), and `wolf` asks once a day,
  in a terminal, whether to update when a newer release exists (spec chapter 8, section 2b;
  E0805). `WOLFPACK_NO_UPDATE_CHECK=1` or `update.check = false` turn the question off.
- Your own target code in `native/ts/`, `native/go/` and `native/rust/`, called through `extern`
  with `native:` paths (spec chapter 7, section 8.2a). `wolf build` copies it into the generated
  project and imports it; new diagnostics E0709 (missing native file), E0710 (native file in a
  generated path) and E0806 (a target tool rejected a project with native code). The example
  `native-slug` and a section of guide chapter 12.
- `Map<K, V>` crosses the extern boundary, with `Int`, `String` or `Bool` keys: it is copied into a
  `Map`, a Go `map[K]V` or a Rust `&HashMap<K, V>`, and a map that comes back is sorted by key on
  every target (spec chapter 7, section 8.4).
- Callbacks: a parameter of an extern function may be a function, called by the extern code
  during the call; a panic in it stays a panic (spec chapter 7, section 8.4, rule 7). Values of
  extern types may not be inside a `Map` at the boundary (E0702).
- Native modules: `imports { native.text.slug }` imports `native/rust/text/slug.rs` without any
  `extern` lines; the compiler reads its declarations and maps the Rust types (spec chapter 6,
  section 1a; chapter 7, section 8.7). The compiler reads Rust files, Go packages
  (`native/go/text/slug/`) and TypeScript files (`native/ts/text/slug.ts`, with `oxc_parser`); a
  TypeScript `number` is `Int` where another target says so, `Float` otherwise. The example
  `native-slug` has no `extern` block any more.
- Methods of native modules: the methods and associated functions of opaque types of native code
  are called as `index.add(word)` and `words.Index.new()` (spec chapter 7, section 8.7, rule 17).
  `wolf check --native [module]` prints what the compiler read from native code; new diagnostic
  E0716. A struct with only public fields becomes a
  record of the module, copied field by field. New diagnostics E0712 to E0715; `native` is a
  reserved root module name.
- `Option` parameters of extern functions arrive as `T | undefined` in TS, `*T` in Go and
  `Option<&T>` in Rust (spec chapter 8, section 8); before, they did not compile on any target.
- Records: a struct of the module crosses the extern boundary by value when a target block binds
  it with `struct Name from "path"` (spec chapter 7, section 8.4a). New diagnostic E0711.
- Fixed: a variant passed for a generic parameter in TypeScript and Go, `Comparable` on a plain
  enum in Rust, `Display` and `Comparable` of plain enums through a type parameter in TypeScript,
  and a Go module named like a package of Go's standard library (such as `cmp`).
- Fixed: in Go, an error returned by extern code is now an `Error` whose `message()` is its text,
  as chapter 7, section 8.4 says; before, calling `message()` on it panicked.

## 0.1.1 (2026-10-17)

- Installation from a terminal on every system: `install.sh` (macOS, Linux) checks the download
  against `SHA256SUMS.txt`, falls back to `wget`, retries, refuses unsupported systems (musl, old
  glibc, 32-bit ARM) with a clear message, updates the profiles of sh, bash, zsh and fish once,
  and takes options (`--version`, `--dir`, `--no-modify-path`, `--no-setup`). The new
  `install.ps1` does the same on Windows (`irm ... | iex`, PowerShell 5.1 and 7, no administrator
  rights). The release workflow tries both scripts on clean machines.
- `wolfpack-lang` is on crates.io: `cargo install --locked wolfpack-lang`. The internal crates say
  that they are not a stable API.
- The playground is published on GitHub Pages, with a smaller compiler (size-optimized, LTO); its
  buttons wait until the compiler is loaded.
- A new `README.md`: every way to install for each system, options, update, removal and
  troubleshooting.
- Plans in `docs/plans/`: calling the project's own TypeScript, Go and Rust code, and package
  managers and signed installers.
- Branches: `master` is the default branch and holds releases; work lands in `night` first.

## 0.1.0 (2026-10-16)

- `wolf setup` installs pinned, checked versions of Node.js, TypeScript, Go and Rust into
  `~/.wolfpack/toolchains`, and `wolf` uses them before the tools on `PATH` (spec chapter 8,
  section 2a; E0804). `wolf setup --check` lists the tools.
- Release installers: a `.pkg` for macOS (universal), a per-user installer for Windows and
  `install.sh` for Linux and macOS; each runs `wolf setup`. The release workflow builds them on a
  tag, tries each one on a clean machine and creates a draft release.
- The language is written `wolfpack` in lower case, the repository is `wolfpack`, and the crates
  are `wolfpack-lang` and `wolfpack-*`, ready for crates.io.
- Fix: on Windows, tools on `PATH` with `.exe` and `.cmd` names are found, and built programs are
  run by their `.exe` name.

## M12: Polish (2026-10-16)

- `wolf fmt` and `wolf fmt --check`: one canonical layout (spec chapter 3, section 8a); the
  examples and run programs are formatted with it.
- `wolf explain <code>`: a text with a wrong and a fixed example for every code, embedded in the
  compiler; the tests check that each wrong example reports its code first and each fixed one
  checks and builds.
- The language guide, `docs/guide/`, in 13 chapters, and a `README.md` with installation and a
  first program. Every program in them is checked, formatted, built for all three targets and run
  on each where its output is shown; the generated code of chapter 13 is compared with the build.
- The playground, `playground/`: a static page with the compiler in WebAssembly (`wolf_wasm`),
  errors while typing, runs of the generated TypeScript in a worker, the generated TypeScript, Go
  and Rust, formatting, `wolf explain` and links that hold the program.
- `wolf_driver`: the in-memory compiler pipeline, used by `wolf_cli` and `wolf_wasm`.
- Spec: rules of `wolf explain`; a method cannot have the name of a field of its struct (E0432);
  `map.get` is a free function, `remove` a method, an assigning `match` arm a block.
- Fix: a string literal passed to a read parameter of a type parameter builds in Rust.
- Decisions: ADR 0012.
- Tests: explain texts, the guide and the README, the playground functions natively and its
  examples, a run program for generic literals, a UI test for E0432; CI builds `wolf_wasm` for
  WebAssembly.

## M11: `extern` (2026-10-16)

- Checks of extern declarations: types at the boundary (E0702), one signature per name across
  target blocks (E0703), path strings in the form of each target (E0705), no generic functions,
  `self` or `mut` parameters (E0706), no operations on opaque types besides extern functions
  (E0707), no duplicates in a block (E0708).
- Target binding: a build rejects every module whose extern names are not declared for the
  target, with the import chain (E0704).
- All three backends: opaque types as type aliases (also Go pointers), method paths called on the
  first parameter, checked integer conversions, `Result` and `Option` results (TS exceptions
  become `Err`, `null` becomes `None`; Rust errors keep their `Display` text), string and list
  results of the target's own types in Rust.
- `[extern.<target>.dependencies]` goes into `package.json`, `go.mod` and `Cargo.toml`;
  `wolf run` and `wolf test` install TS packages with `npm install` (E0803 when it fails).
- New example `fuzzy` with a package of each target language.
- Fix: a package named `deps`, `examples`, `build` or `incremental` gets a Rust binary named
  `<name>-bin`, since Cargo reserves these names.
- Spec: integer conversions at the boundary, pointer types in Go paths, fetching dependencies,
  E0803.
- Decisions: ADR 0011.
- Tests: a UI test for each new code, run programs for the extern boundary and for E0704 and
  E0803, `fuzzy` runs and passes its tests on every target.

## M10: The Rust backend (2026-10-14)

- `wolf_rs`: the Rust backend builds a tree and prints it as rustfmt formats it. Each module is a
  file (`src/users/storage.rs` with `pub mod` declarations in `src/users.rs`); std and the
  runtime are the module `wolf_rt`, which holds only the pieces a program uses. `wolf build`,
  `wolf run` and `wolf test` take `--target rust`, and `rust` is the default when `wolfpack.toml`
  says so. `wolf build --target all` builds every enabled target.
- Borrowing follows the modes: read parameters are `&T`, `&str`, `&[T]`, `&dyn Trait` or
  `&dyn Fn`, `mut` parameters `&mut T`, `own` parameters `T`; locals are `let mut` only where they
  change. Recursive fields are boxed, interface values are `Box<dyn Trait>` with a `copy_dyn`
  method for `.copy()`, `own self` interface methods take `self: Box<Self>`.
- Native `Option`, `Result` and `?`, `match` with `if let` and `while let` where they fit,
  `format!` for string concatenation, derives of `Clone` and `PartialEq` where the fields allow
  them, checked arithmetic through `overflow-checks = true`, indexing through `items.at(i)`, the
  canonical panic messages through a panic hook.
- `wolf test` on Rust runs `cargo test` per Cargo target and prints the same report as on
  TypeScript. E0801 and E0802 name `cargo`.
- Fixes in the earlier backends: Go holds fields recursive through `Option` by pointer, copies
  interface values through a generated `Copy<Name>` method, and puts plain enums with `mut`
  methods behind pointers as interface values; TypeScript narrows `match mut` on list elements;
  `--out` is resolved against the current directory.
- Decisions: ADR 0010.
- Tests: every example and every run program gives on Rust the output it gives on TypeScript,
  with `cargo fmt --check` and `cargo clippy -- -D warnings` clean; the test reports and filters
  match; tests stay out of the normal build; the output is deterministic; E0801 and E0802 for
  `cargo`; new run programs for recursive data, interface values and `own self` interface
  methods; CI builds the generated Rust with Rust 1.85.

## M9: The Go backend (2026-10-13)

- `wolf_go`: the Go backend builds a tree and prints it as `gofmt` formats it. Each module is a
  package; std and the runtime are the package `wolfrt`, which holds only the pieces a program
  uses. `wolf build`, `wolf run` and `wolf test` take `--target go`, and `go` is the default when
  `wolfpack.toml` says so.
- Idiomatic results: `Option<T>` is `(T, bool)` or `*T`, `Result<T, E>` is `(T, error)` or
  `error` when `E` implements `Error`; `?` becomes `if err != nil { return ..., err }`, and a
  `match` on such a result becomes `if v, ok := f(); ok`. Everywhere else the data forms
  `wolfrt.Option[T]` and `wolfrt.Result[T, E]` are used.
- Structs with pointer receivers when a method takes `mut self`, `mut` parameters as pointers,
  enums without payloads as constants, enums with payloads as sealed interfaces with a pointer to
  a struct per variant, `match` as type switches, value switches or `if` chains (a `break` inside
  a `switch` gets a label), generic functions with dictionaries for `Equatable`, `Copyable` and
  `Comparable`, generated `Equal` and `Copy` where they are used, checked integer arithmetic.
- `wolf test` on Go runs `go vet` and `go test -json` and prints the same report as on
  TypeScript. E0801 and E0802 name `go`.
- The IR keeps the bounds of `impl` blocks on the type's parameters.
- Decisions: ADR 0009.
- Tests: every example and every run program gives on Go the output it gives on TypeScript, with
  `gofmt -l` and `go vet` clean; the test reports match; E0801 and E0802 for `go`.

## M8: Ownership (2026-10-10)

- `wolf_own`: the ownership rules of chapter 5, checked on the IR after type checking, as part of
  `wolf check`. A flow-sensitive walk of each body tracks moved variables through branches and
  loops: use after a move (E0501), use after a move on some paths or in a previous iteration
  (E0502), moves of fields and elements (E0503), moves out of borrowed values (E0504), changes of
  immutable bindings (E0505), `mut` arguments that are not places (E0506), call-site markers
  (E0507), overlapping places in one call (E0508), access to places borrowed by `for` and `match`
  (E0509), closures capturing borrowed values (E0510) or changing captured ones (E0511).
- The IR carries source spans and the functions of built-in types.
- Spec: `own` is never written before a temporary; the example of chapter 9 is fixed.
- Tests: a pair for every rule, a project with the error and a correct project
  (`tests/valid/`); all examples and run programs still check and run.

## M7: Tests (2026-10-09)

- `wolf test [text]` builds the project with its tests (`tests` sections, their helper functions
  and the modules under `tests/`), runs them with `node:test`, and prints one report for every
  target through a generated reporter: one line per test, the failures with their panic messages,
  and a summary; exit code 0 or 1 (spec chapter 8, rule 2.7, new). `npm test` works in the
  generated project.
- Tests and helpers are never part of `wolf build`; in a test build `main` is not run.
- A failed `assert` is a panic at the location of the call:
  `panic at src/main.wolf:19:9: assertion failed: halve(4) == 3`.
- Tests: the tests of all examples pass; a project with passing and failing tests (assertions,
  panics, overflow, helpers, a nested module) checks the report, also with a filter.

## M6: IR and the TypeScript backend (2026-10-09)

- `wolf_ir`: the checked program lowered to a target-neutral IR: resolved calls and methods,
  explicit interface coercions and `?` conversions, blocks that discard their final value,
  patterns in declaration order, evaluated constants, `panic` with its source location and
  `assert` with its source text.
- `wolf_ts`: the TypeScript backend builds a tree and prints it. Structs and payload variants are
  classes, payload-less enums are string unions, `Int` is checked `bigint` arithmetic, `Option`
  uses the hybrid form of ADR 0002, `mut` parameters of value types get a `Ref`, `match mut` and
  `for x in mut` write through to the place. The runtime `wolf_rt.ts` contains only the pieces a
  program uses. `extern` functions for TypeScript (ADR 0007).
- `wolf build` writes `out/ts/`; `wolf run` builds it with `tsc --strict` and runs it with Node.js,
  `--example <name>` for examples. Toolchain errors E0801 and E0802 (new in the spec).
- All 10 examples run and print exactly their `expected.txt`; further programs test overflow
  and panics, exit codes, `Err` from `main`, Unicode slicing, the `Float` format, `Map` order and
  the std functions. CI installs Node.js 22 and TypeScript 5.

## M5: Types, part 2 (2026-10-09)

- Bodies of generic functions, types and `impl` blocks are checked: a type parameter is a type
  of its own whose operations come from its bounds (methods of the bound interfaces, `==` with
  `Equatable`, `.copy()` with `Copyable`).
- Bounds of user generics at every use, in calls and in written types (E0404); generic methods
  in inherent `impl` blocks; interfaces with type parameters and coercion to them.
- Interface implementations: missing, extra and mismatched methods (E0413); implementations that
  cannot be written, such as `Rect: Circle`, `Circle: Equatable` or a second implementation
  (E0431, new); object safety of interfaces used as types (E0416); interface members without
  `self` or with type parameters (E0417).
- Closures: parameter types also come from a later use in the same body (`let f = |x| x.len()`).
- `impl` blocks inherit the bounds of their type (spec, new); a `match` whose scrutinee type is
  learned later in the body is still checked for exhaustiveness.
- New language rule: `fn main` is the last function of `functions` (E0213).
- All 10 examples type-check, including `generic_stack`, `shapes` and `textkit`.

## M4: Types, part 1 (2026-10-09)

- `wolf_types`: the type checker of chapter 4. Declarations: aliases expanded (cycles E0419),
  type argument counts (E0403), coherence of `impl` (E0414), one function per name on a type
  (E0415), infinitely sized structs (E0418). Bodies: bidirectional checking with local inference
  (E0401, E0420), operators (E0405), `Equatable`/`Copyable` derived with the offending field named
  (E0406), map keys (E0407), bounds of std generics (E0404), `?` with coercion to interfaces
  (E0408), struct literals and fields (E0422, E0424, E0607), calls and variant forms (E0428),
  types used as values (E0429), loops (E0430, `while true` is Never), missing results (E0421),
  discarded `Result` and useless statements (E0409, E0425).
- Patterns and exhaustiveness with the usefulness algorithm: missing patterns with an example
  (E0410), unreachable arms (E0411), alternatives (E0412).
- `config` constants: allowed types (E0427), constant expressions (E0426), evaluation with the
  integer rules of chapter 4 (E0423), overrides from `wolfpack.toml` (E0613 for alias types).
- Scope per ADR 0006: the seven examples without user generics type-check; generic bodies wait
  for M5.
- Spec: Void blocks discard their final value, `while true` is Never, variant forms, `()` and
  blocks as statements, codes E0426–E0430. std: `Bool.to_string`.
- Tests: a UI project per type rule (E0401 per place where it occurs), std and a large valid
  program type-check cleanly, inference cases, deep nesting through the checker.

## M3: Projects and modules (2026-10-08)

- `wolf_project`: `wolfpack.toml` with exact key positions in errors (E0611), `[config.*]`
  overrides (E0613), the fixed layout (E0612), project discovery from any subdirectory, and
  `wolf new <name> --kind cli|lib`.
- `wolf_std`: the std modules as `.wolf` declarations (`intrinsic` functions and types), with the
  prelude re-exporting the built-in types.
- `wolf_resolve`: two-pass name resolution over the project and std: imports and the import
  graph (E0602–E0605, E0615, E0616, E0620), visibility (E0607, E0608, library API), local scopes
  and shadowing (E0606), unknown names and types with suggestions (E0609, E0402), naming style
  (E0601), module lines (E0610), entry points (E0614), prelude names (E0617), unused variables
  (E0618), sections of test modules (E0619), `intrinsic` outside std (E0701). Failed imports do
  not cause follow-up errors. The resolution table maps names, types, patterns, shorthand fields
  and declarations to what they name, for the type checker.
- New codes in the spec: E0004 (no manifest), E0619, E0620; `intrinsic type` in std.
- `wolf check` checks the project; `wolf check <file.wolf>` stays as a lex-and-parse mode.
- Tests: UI projects for every code of chapter 6, all 10 examples check cleanly (also from a
  subdirectory), `wolf new` projects check cleanly, std resolves cleanly, deep nesting through the
  resolver on the compiler stack.

## M2: Parser and skeleton (2026-10-08)

- `wolf_ast`: the syntax tree for spec chapters 2 and 3, with node ids for later phases and a
  span-free text dump.
- `wolf_parser`: recursive descent with a Pratt parser for expressions, the newline rules of
  chapter 1 (innermost delimiter decides, trailing operators and leading `.` continue a line),
  struct-literal restriction in conditions, `>>` splitting in type arguments.
- Skeleton checks E0201–E0212 (order, duplicates, empty sections, misplaced declarations, unknown
  sections, extern targets, inherent impls, test names, doc comment placement) and parser errors
  E0301–E0314.
- Error recovery: a lexer error silences the rest of its line, an error skips to the end of the
  line or the closing delimiter, a section header inside a block closes the open blocks.
- Nesting limit of 256 levels (E0314, new in the spec); the compiler runs on a 64 MiB thread so
  that the limit always fits on the stack.
- `wolf check` now lexes and parses; `--emit ast` prints the tree.
- Tests: AST snapshots of all 16 example files, grammar unit tests, nesting tests for every kind
  of deep input, a never-panic test on generated token soup, and UI tests for every code.

## M1: Foundation (2026-10-08)

- Cargo workspace with lints (`unsafe_code` forbidden; `unwrap`, `expect`, `panic` denied outside
  tests) and CI on GitHub Actions (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`).
- `wolf_span`: source map, byte spans, line and column lookup counting Unicode scalar values.
- `wolf_diag`: the code registry (E0001–E0003, E0101–E0113), diagnostics with labels, help and
  notes, rustc-style text output and JSON lines output (`--message-format json`).
- `wolf_lexer`: hand-written lexer for spec chapter 1 with all lexical errors, error recovery and
  comment tracking.
- `wolf check [paths]`: lexes `.wolf` files and reports lexical errors. Until M3 it takes files or
  directories directly instead of a project.
- Tests: lexer unit tests, a token snapshot, a check that every example lexes cleanly, a
  never-panic test on generated input, UI tests for every code, and a libFuzzer target in `fuzz/`.

## M0: Specification (2026-10-07)

- Language specification in `docs/spec/`, chapters 0 to 9: lexical structure, file skeleton,
  grammar and precedence, type system, ownership, modules and project layout, standard library and
  `extern`, mapping to TypeScript, Go and Rust, diagnostics.
- Decision records `docs/decisions/0001` to `0005`: 64-bit `Int` with `bigint` in TS, hybrid
  `Option`/`Result` representation, insertion-ordered `Map`, payload-less enums as copy types,
  TS classes and one Go package per module with a generated runtime.
- Ten example projects in `examples/` with expected output, covering every milestone criterion.
- Deferred ideas collected in `docs/ideas.md`.
- No compiler code yet.
