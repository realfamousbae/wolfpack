# Chapter 4: Type system

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter defines the types of wolfpack, how expressions are typed, generics, interfaces, the
built-in interfaces and the exhaustiveness of `match`. Ownership (moves, parameter modes) is in
chapter 5; the methods of built-in types are in chapter 7.

Decisions already taken by the author for this chapter (2026-10-07):

- Generic methods are allowed only in inherent `impl` blocks, never in interface signatures
  (register item 13a).
- `==` is structural and derived automatically; `<`, `<=`, `>`, `>=` work only on primitives;
  ordering of other types goes through `a.compare(b)` (no operator overloading, PLAN section 3).
- `==` and `.copy()` of user types are derived automatically; an error at the use site names the
  field that prevents it.
- Values of an interface type support `.copy()` but not `==` (register item 13b).

## 1. Principles

1. Typing is static, strict and **nominal**: two struct types with the same fields are different
   types. **[PLAN]**
2. There are **no implicit conversions** between types **[PLAN]**. `1 + 1.0` is an error; write
   `1.to_float() + 1.0` (conversions are std functions, chapter 7).
3. The only implicit change of type is the **interface coercion** of section 8.5: a value of a
   concrete type is accepted where an interface type is expected. **[PROPOSAL]** (see D4.6)
4. Types of parameters, results, fields and constants are always written. Types of local variables
   and closure parameters are inferred **[PLAN]** (section 10).

## 2. Built-in types

| Type | Values | Notes |
| --- | --- | --- |
| `Int` | signed 64-bit integers | ADR 0001 |
| `Float` | IEEE 754 binary64 | **[PLAN]** |
| `Bool` | `true`, `false` | |
| `String` | immutable sequences of Unicode scalar values, stored as UTF-8 | **[PLAN]**, G1 |
| `Void` | exactly one value, `()` | **[PLAN]** |
| `List<T>` | ordered, growable sequence | **[PLAN]** |
| `Map<K, V>` | finite map from keys to values, `K: Hashable` | **[PLAN]**, G3 |
| `Option<T>` | enum `Some(T)`, `None` | **[PLAN]** |
| `Result<T, E>` | enum `Ok(T)`, `Err(E)` | **[PLAN]** |
| `Ordering` | enum `Less`, `Equal`, `Greater` | result of `compare`, **[PROPOSAL]** |
| function types | `fn(A, B) -> R` | chapter 3, section 3 |

1. `Option`, `Result` and `Ordering` behave exactly like user enums (section 7), except that the
   variants `Some`, `None`, `Ok`, `Err` are written without qualification. `Ordering` variants are
   qualified: `Ordering.Less`. **[PROPOSAL]**
2. There is also an internal type **Never**, which has no values and cannot be written in source. It
   is the type of `panic(...)`, `return`, `break` and `continue` (D4.2; they are expressions, chapter 3). An
   expression of type Never is accepted where any type is expected. This is what makes
   `None => return 0` or `_ => panic("unreachable")` type-check in a `match`. **[PROPOSAL]**

## 3. Arithmetic and operators

### 3.1 `Int`

1. `+`, `-`, `*` and unary `-` **panic on overflow** on every target (register item 9).
2. `/` truncates toward zero. `%` has the sign of the dividend: `-7 / 2 == -3`, `-7 % 2 == -1`.
   (Rust, Go and JS `bigint` already agree on this.)
3. Division or remainder by zero panics. `MIN / -1` (where `MIN` is the smallest `Int`, `math.int_min`
   in std) and `MIN % -1` panic (overflow). Go
   wraps silently here, so the Go backend must check.
4. Bitwise `&`, `|`, `^`, `~` work on `Int` only, on the two's complement representation.
5. Shifts `<<` and `>>` **[PROPOSAL]** (see D4.8):
   - the shift amount must be in `0..=63`, otherwise the shift panics;
   - `<<` discards the bits shifted out and never panics for overflow (`1 << 63 == math.int_min`);
   - `>>` is arithmetic: it copies the sign bit.

   The three targets disagree here (Rust discards bits, JS `bigint` grows without limit, Go panics
   only on negative amounts), so every backend implements exactly this rule.
6. Literals: an integer literal has type `Int` and must fit (chapter 1, section 5.1). There are no
   untyped or polymorphic literals: `let x = 5` is `Int`, and `5` is never a `Float`.

### 3.2 `Float`

1. `+`, `-`, `*`, `/` follow IEEE 754 on every target: division by zero gives an infinity or NaN,
   it does not panic. `%` on `Float` is IEEE `fmod` with the sign of the dividend.
2. `==` follows IEEE: `NaN != NaN`. **[PROPOSAL]** (see D4.7)
3. `<`, `<=`, `>`, `>=` work on `Float` with IEEE semantics (every comparison with NaN is `false`).
4. `Float` is **not** `Comparable` and not `Hashable` (it has no total order). **[PROPOSAL]**
5. Bitwise operators and shifts do not apply to `Float`.

### 3.3 `Bool`

`&&` and `||` short-circuit; `!` negates. Bitwise operators do not apply to `Bool`: Go has no `&` on
`bool`. `==` and `!=` work. `<` and friends do not.

### 3.4 `String`

1. `+` concatenates two `String` values. Nothing else can be added to a `String` (`"n=" + 5` is an
   error; use `5.to_string()`).
2. `==` compares scalar values. `<`, `<=`, `>`, `>=` compare **lexicographically by Unicode scalar
   value** (equivalently, by UTF-8 bytes). JS compares UTF-16 units, which differs for characters
   above U+FFFF, so the TS backend uses a helper. **[PROPOSAL]**
3. Length, positions and slices count scalar values (G1). `s[a..b]` is a new `String`; a bound
   outside `0..=len` or `a > b` panics.
4. There is no `s[i]` on `String` (there is no `Char` type); use `s[i..=i]`. **[PROPOSAL]**

### 3.5 Comparison operators in general

1. `==` and `!=` work on any type that is `Equatable` (section 9), both sides of the same type.
2. `<`, `<=`, `>`, `>=` work only on `Int`, `Float` and `String` (author). Generic code and user
   types use `a.compare(b)`, which returns `Ordering`.

### 3.6 Compound assignment

`x op= y` has exactly the typing and the panics of `x = x op y`; `x` is evaluated once.

## 4. Lists and maps

1. `l[i]` on `List<T>` requires `i: Int`; an index outside `0..len` panics. `l[i] = v` replaces an
   element under the same rule.
2. `l[a..b]` (and the other slice forms) gives a new `List<T>` of copies; it requires
   `T: Copyable`. Bounds as for strings.
3. `Map<K, V>` requires `K: Hashable` (section 9). `m[k]` gives the value and **panics if the key is
   missing**; `map.get(m, k)` from `std.map` returns a copy in an `Option<V>` (chapter 7, section
   6). `m[k] = v` (and `m[k] += 1`) **replaces** the value
   of an existing key and panics if the key is missing; a new key is added only with
   `m.insert(own key, own value)`, so the map never takes a key it does not own. (Author,
   2026-10-07.)
4. **Iteration order of `Map` is insertion order** on every target (author, D4.1): `m.keys()`,
   `m.values()` and the other std functions that walk a map return elements in the order their keys
   were first inserted. Replacing the value of an existing key keeps its position; removing a key
   and inserting it again puts it at the end. TS uses `Map` as is; the Rust backend uses an
   insertion-ordered map (for example the `indexmap` crate); the Go backend uses a generated type
   that keeps a key slice next to the map (chapter 8).
5. A literal's element types must agree: `[1, "a"]` is an error. `[]` and `[:]` take their type
   from context (section 10).
6. Two lists are equal when they have the same length and equal elements in order. Two maps are
   equal when they have the same keys and equal values, independent of order.

## 5. `Option`, `Result` and the `?` operator

1. `?` applied to `Result<T, E>` evaluates to `T` on `Ok`, and on `Err(e)` returns `Err(e)` from the
   enclosing function. The enclosing function must return `Result<U, F>` where either `F` is `E`, or
   `F` is an interface that `E` implements; in the second case `e` is coerced to `F` exactly as in
   section 8.5. The usual case is `F = Error`, the built-in error interface (chapter 7). No other
   conversion of errors exists. (Author, 2026-10-07, register item 15.)
2. `?` applied to `Option<T>` evaluates to `T` on `Some`, and on `None` returns `None`. The
   enclosing function must return `Option<U>`.
3. `?` on `Option` inside a function returning `Result` (or the reverse) is an error.
4. Inside a closure, `?` refers to the closure's own result type.
5. The `PLAN.md` example `fs.read_text(path)?` inside a function returning `Result<Config, Error>`
   works by rule 1: `fs.read_text` returns `Result<String, FsError>`, and `FsError` implements
   `Error` (chapter 7).

## 6. Control flow typing

1. Conditions of `if` and `while` have type `Bool`. In a chain (D3.10, chapter 3,
   section 5, rule 14a), every item that is not a `let` has type `Bool`.
2. An `if` with `else` has the type of its branches, which must agree (Never agrees with
   everything). An `if` without `else` has type `Void`, and its block must have type `Void`.
3. All arms of a `match` have the same type.
4. A block has the type of its final expression, or `Void` if there is none. When a block is
   expected to have type `Void` (the body of a function or closure without a result, an `if`
   without `else`, the body of a loop, the branches of an `if` or a `match` that is itself an
   expression statement), its final expression is treated as an expression statement: its value
   is discarded, and rules 7 and 8 apply to it. So `fn drop_last(mut l: List<Int>) { l.pop() }`
   is valid although `pop` returns `Option<Int>`. (Author, 2026-10-09.)
5. A function's body must have the declared result type. A function whose result is not `Void`
   must not fall off its end without a value.
6. `for x in e` accepts `e: List<T>` (then `x: T`) and integer ranges (then `x: Int`).
   **[PROPOSAL]** Other collections are iterated through std functions that return a `List`
   (`m.keys()`, `s.chars()`, chapter 7), so the language has no iterator protocol in v1.
7. **Discarding a `Result`.** An expression statement whose value is a `Result` is an error
   (E0409): the error must be handled, propagated with `?` or explicitly ignored with `let _ =`.
   Other values returned by calls may be discarded. **[PROPOSAL]** (see D4.3)
8. **A useless expression statement is an error** (E0425), as in Go (author, 2026-10-08). An
   expression statement that is not the value of its block must be one of: a call (of a function,
   method or closure, possibly followed by `?`), an `if`, a `match`, a block `{ ... }`, the empty
   value `()` (a "do nothing" arm such as `_ => ()`; author, 2026-10-09), or `return`, `break`,
   `continue`. Anything else (`- tax`, `x + 1`, `name`, a literal) computes a value that nobody uses
   and is rejected. This also catches a statement split by a line break:
   ```
   let total = price
       - tax          // error E0425: `- tax` is computed but not used
   ```
9. **`while true` never ends** unless it contains a `break` that belongs to it (not one in a nested
   loop or closure). Such a loop has type Never, so `fn next() -> Int { while true { ... } }`
   needs no value after the loop. Only the literal `true` counts; conditions are not evaluated.
   Every other loop has type `Void`. (Author, 2026-10-09; Go has the same rule for `for {}`.)
   A `loop` (0.2) follows the same rule without a condition: without a `break` of its own it has
   type Never. A `break` of a labelled outer loop belongs to that outer loop.
9a. **The type of a `loop`** (0.2). A `loop` with `break` of its own has the type of its `break`
   values: all of them must have one type, and a `break` without a value counts as `()` (E0401
   when they differ). So `loop { if done { break } }` has type `Void`, and
   `let n = loop { break 5 }` has type `Int`. `break value` in a `while` or a `for` is an error
   (E0435).
9b. **Patterns outside `match`** (0.2). The pattern of a destructuring `let` must be
   **irrefutable**: a name, `_`, or a struct pattern (not a variant) whose sub-patterns are
   irrefutable (E0433 otherwise). `if let` and `while let` accept any pattern; their bindings have
   the types they would have in a `match` arm. Exhaustiveness (E0410) does not apply to them.
   The same holds for every `let` item of a chain (D3.10).
10. `break` and `continue` are allowed only inside a loop body. The body of a closure is not
    inside the loop around the closure (E0430). Labels are resolved as in chapter 3, section 4,
    rule 5 (E0436, E0437, E0438).
11. **Interpolated strings** (0.2). The value of every interpolation in `f"..."` must be of a type
    that implements `Display` (section 9; E0404 otherwise). The result has type `String`.

## 7. User types

### 7.1 Structs

1. A struct literal must give every field exactly once.
2. Field access `x.f` requires the field to be visible (chapter 6).
3. A struct literal `Name { ... }` and a struct pattern `Name { ... }` name a struct, an alias of a
   struct (section 7.3) or a variant with named fields. Any other name, such as a built-in type,
   an enum, an interface or a function, is an error (E0439). **[PROPOSAL]** (2026-10-23)

### 7.2 Enums

1. A variant is constructed with its qualified name: `StorageError.Full`, `StorageError.Io("x")`,
   `Shape.Rect { w: 1.0, h: 2.0 }`. A variant with a payload is always written with it: it is not a
   function value, so `names.map(Some)` is an error; write `names.map(|n| Some(n))`. The form must
   match the declaration: `Ordering.Less()` and `Shape.Rect(1.0, 2.0)` are errors (E0428).
   (Author, 2026-10-09.)
2. Variants are not types of their own: `StorageError.Io("x")` has type `StorageError`.
3. An enum has at least one variant (chapter 3).

### 7.3 Type aliases

`type UserId = Int` introduces another name for the same type: `UserId` and `Int` are fully
interchangeable. Aliases may be generic (`type Pairs<T> = List<List<T>>`). A cycle of aliases is an
error. **[PROPOSAL]**

An alias of a struct also stands for the struct in literals and patterns: with `type P = Point`,
`P { x: 1, y: 2 }` builds a `Point` and `P { x, y }` matches one; type arguments are written after
the alias as after a struct (`Same.<Int> { ... }` for `type Same<T> = Pair<T>`). **[PROPOSAL]**
(2026-10-23)

### 7.4 Recursive types

1. A type may refer to itself through an enum variant, `Option`, `List` or `Map`: `enum Tree { Leaf,
   Node(Tree, Tree) }`, `struct Node { next: Option<Node> }`. The Rust backend adds `Box` where
   needed **[PLAN]** (section 4).
2. A struct that contains itself directly (`struct A { a: A }`), possibly through other structs, has
   no finite values and is an error. **[PROPOSAL]**
3. **Depth of values** (author, 2026-10-23). `==`, `!=`, `.copy()` and dropping a value
   (chapter 5, section 4, rule 5) work on values nested at least 1,000,000 levels deep, on every
   target and in the VM: a chain of 1,000,000 variants built by a loop
   (`c = Chain.Link(i, c)`) is compared, copied and dropped like a short one, and never overflows
   the stack. The rule is about the data only; how deep the program's own functions may recurse is
   not specified (chapter 10, section 2, rule 7). How each target meets the rule is in chapter 8,
   section 5.5.

## 8. Generics and interfaces

### 8.1 Generic declarations

1. Functions, structs, enums, aliases, interfaces and inherent `impl` blocks may have type
   parameters with bounds `<T: A + B>` **[PLAN]**.
2. Inside a generic body only the operations allowed by the bounds may be used on `T`: methods of
   the bound interfaces, `==` with `T: Equatable`, `.copy()` with `T: Copyable`. **[PROPOSAL]**
3. Generic code is checked once, at the declaration, not at each use (as in Rust and Go, unlike C++
   templates).
4. A use of a generic declaration must satisfy the bounds; the error points at the use and names
   the missing interface.

### 8.2 Interfaces

1. An interface is a set of method signatures **[PLAN]**. Every member takes `self` (in any mode).
   Functions without `self` are not allowed in interfaces in v1. **[PROPOSAL]** (see D4.4)
2. An interface may have type parameters (`interface Container<T>`), but its methods may not (author,
   13a). The reason: neither Go nor a Rust trait object can express a generic interface method.
3. A signature may use `Self`, meaning "the implementing type": `fn compare(self, other: Self) ->
   Ordering`.
4. **Default methods** (author, 2026-10-23, D4.19). A method of an interface may have a body,
   its default:
   ```
   interface Named {
       fn name(self) -> String
       fn greeting(self) -> String {
           "Hello, " + self.name() + "!"
       }
   }
   ```
   The body is checked once, as a generic function over `Self: Named`: on `self` it can use the
   methods of the interface, defaults included, and nothing else (`Self` is not known to be
   `Copyable` or `Equatable`). A type that implements the interface gets the default when its
   block leaves the method out, and replaces it with `override fn` (section 8.3, rule 1). A default
   is part of the interface everywhere the interface is: on interface values, through bounds, and
   in the implementing type's own calls `x.greeting()`.
5. **Calling a default by the interface's name** (D4.19). `Named.greeting(x)` calls the body of
   the interface, also when the type of `x` replaces it: an override can build on the default.
   ```
   Dog: Named {
       fn name(self) -> String { self.title.copy() }
       override fn greeting(self) -> String {
           Named.greeting(self) + " Woof!"
       }
   }
   ```
   The receiver is the first argument, passed in the mode of the method's `self` (`mut x` for
   `mut self`), and its type must implement the interface (E0404). Only a method with a default
   can be called this way (E0444).
6. **[DRAFT]** (2026-10-25, D4.20) **Super-interfaces.** An interface may name other interfaces
   after `:`, its super-interfaces, written as bounds are (chapter 3, section 2):
   ```
   interface Named {
       fn name(self) -> String
   }

   interface Greeter: Named + Display {
       fn greeting(self) -> String {
           "Hello, " + self.name() + "!"
       }
   }
   ```
   - A type that implements `Greeter` must implement `Named` and `Display` too, each in its own
     block (section 8.3, rule 7). The block of `Greeter` defines only the methods of `Greeter`.
   - Wherever a type is known to implement `Greeter`, it is known to implement the
     super-interfaces, and theirs in turn: generic code with `T: Greeter` may call `name` and
     `to_string` on a `T`, and a default of `Greeter` may call them on `self`.
   - A super-interface is an interface of the program, `Error` (chapter 7, section 2), or one of
     the built-in interfaces that a user implements by hand, `Comparable` and `Display`
     (section 9). `Equatable`, `Copyable` and `Hashable` cannot be super-interfaces (E0445): they
     are derived or closed, so no block implements them.
   - A super-interface may have type arguments, from the parameters of the interface:
     `interface Sorted<T>: Container<T>`.
   - An interface that is its own super-interface, directly or through others
     (`interface A: B` and `interface B: A`), and a super-interface named twice in one list are
     errors (E0445). Two super-interfaces may share one of their own (`interface C: A + B` with
     `A: Base` and `B: Base`): a type implements `Base` once.
   - A method of an interface cannot have the name of a method of one of its super-interfaces,
     since a type that implements both would have two methods with one name (section 8.3,
     rule 4): E0447, reported at the interface.
   - A default of a super-interface is replaced in the block of that super-interface, never in the
     block of the interface that names it. `Named.m(x)` (rule 5) calls a default of `Named` only.

### 8.3 Implementations

1. `Type: Interface { ... }` must define every method of the interface, with exactly the same
   parameter modes, parameter types and result type (after substituting `Self`). Extra methods are
   not allowed in such a block. A method with a default (section 8.2, rule 4) may be left out; a
   block that leaves out a method without a default is the error it is today (E0413). A method
   that replaces a default is written `override fn`; without `override` it is an error, and so is
   `override` on a method without a default or outside an interface implementation block (E0443).
   (D4.19, author, 2026-10-23.)
2. **Coherence** **[PROPOSAL]** (see D4.5): every `impl` block for a type, inherent or for an
   interface, must be in the module that declares the type. Consequently user interfaces cannot be
   implemented for `Int`, `String`, `List` or extern types. Reason: Go allows methods only on types
   of the same package; this rule makes every `impl` map onto Go methods.
3. A type may implement an interface at most once.
4. **No two methods with the same name** on one type, across its inherent block and all its
   interface blocks. Go cannot express that. **[PROPOSAL]** The defaults that a type gets from its
   interfaces (section 8.2, rule 4) count as its methods: for them too, this rule is E0415 and
   rule 6 is E0432, reported at the implementation block. (D4.19.)
5. Generic inherent methods (`fn map<U>(self, f: fn(T) -> U) -> Stack<U>`) are allowed (author). The
   Go backend may emit them as free generic functions until it can rely on generic methods in Go;
   this is a backend matter (chapter 8).
6. **No method with the name of a field.** A method of a struct (a function with `self`, in its
   inherent block or in an interface implementation) cannot have the name of one of the struct's
   fields (E0432). TypeScript classes and Go structs keep fields and methods in one namespace.
   Associated functions without `self` and the fields of enum variants are not affected. (Author,
   2026-10-16.)
7. **[DRAFT]** (D4.20) **Super-interfaces.** A block `Type: Greeter { ... }` needs `Type` to
   implement every super-interface of `Greeter` (section 8.2, rule 6), with the same type
   arguments: `Type: Named { ... }` in the same module (rule 2), or a built-in implementation
   (`Display` of `Int`). For a generic type, the bounds of the head of the `Greeter` block must
   give what the head of each super-interface's block needs: with `Bag<T: Named>: Named`,
   `Bag<T>: Greeter` is E0446 and `Bag<T: Named>: Greeter` is fine. A missing implementation is
   E0446 at the block of `Greeter`, naming the super-interface.

### 8.4 Method calls

`x.m(args)` looks up `m` among the methods of the type of `x` (inherent or from an implemented
interface). By rule 8.3.4 there is at most one. For a type parameter `T`, the methods come from its
bounds. Associated functions are called as `Type.f(args)`.

### 8.5 Interfaces as value types

1. An interface `I` can be used as a type (`List<Shape>`, a field of type `Shape`) **only if it is
   object-safe**: no method mentions `Self` except as the receiver, and (by 8.2) no method is
   generic and every member takes `self`. Using a non-object-safe interface as a type is an error
   that names the offending method. Such interfaces (for example `Comparable`) are still usable as
   bounds. **[PROPOSAL]** **[DRAFT]** (D4.20): an interface with super-interfaces is object-safe
   only if each of them is, so `interface Ordered: Comparable` is a bound only. A value of type
   `Greeter` has the methods of `Greeter` and of its super-interfaces (`g.name()`), but it is not
   converted to a value of a super-interface: `let n: Named = g` is E0401, as there is no
   coercion between interface types. The reason: a value of a Rust trait object becomes one of
   its super-trait only from Rust 1.86, and the generated Rust builds with 1.85 (chapter 8).
2. **Coercion.** Where an expression of interface type `I` is expected (an annotated `let`, an
   argument, a field, a list element with a known element type, a result), a value of a concrete
   type that implements `I` is accepted and becomes a value of type `I`. **[PROPOSAL]** (see D4.6)
   There is no coercion between `List<Circle>` and `List<Shape>`: only single values are coerced.
3. Operations on a value of type `I`: calling the methods of `I`, `.copy()` (author, 13b), moving.
   `==` is not available (author, 13b). There is no downcast back to the concrete type in v1.
4. In Rust the type is `Box<dyn I>`; `own self` methods take `self: Box<Self>`. Chapter 8 has
   the mapping.

### 8.6 Methods with bounds of their own (author, 2026-10-23, D4.16)

1. A method of a generic inherent `impl` block may add bounds to the type parameters of the block
   with `where`, written after its result type:
   ```
   impl {
       Stack<T> {
           fn contains(self, item: T) -> Bool where T: Equatable {
               for x in self.items {
                   if x == item {
                       return true
                   }
               }
               false
           }
       }
   }
   ```
   Inside the method the added bounds hold; the other methods of the block see only the bounds
   of the block. Several parameters are separated by commas: `where T: Comparable, U: Display`.
2. A call `s.contains(x)` needs the added bounds for the type arguments of the receiver. When one
   is missing, it is the error of a bound that is not satisfied, which names the method: E0404, or
   E0406 for `Equatable` and `Copyable` ("`fn() -> Int` is not `Equatable`", with the label
   "`contains` needs `Equatable`"). The same holds for the bounds that the head of the block adds
   (chapter 3, section 2, rule 6): they too are checked at every call of a method of the block.
3. `where` is a contextual word (chapter 1, section 4): special only between the signature of a
   method and its body. It may name only type parameters of the block: a `where` on a free
   function, in an interface, in an interface implementation block, or naming the method's own
   type parameters or an unknown name, is an error (E0442).
4. std uses it (chapter 7, section 5): `contains`, `index_of`, `get`, `sort`, `min` and `max` are
   methods of `List<T>` with `where` bounds, and no longer free functions of `std.list`.

## 9. Built-in interfaces

| Interface | Meaning | Who implements it |
| --- | --- | --- |
| `Equatable` | `==` and `!=` are defined | **derived automatically**: `Int`, `Float`, `Bool`, `String`, `Void`, `Ordering`; `List<T>`, `Option<T>` if `T: Equatable`; `Map<K, V>` if `V: Equatable`; `Result<T, E>` if both are; a struct or enum if all its fields and payloads are. Not: function types, interface values |
| `Copyable` | `.copy()` is defined | **derived automatically**: every type except function types and types containing them. Interface values are `Copyable` (author, 13b) |
| `Hashable` | can be a `Map` key | **closed**: `Int`, `String`, `Bool` and enums without payloads (G3). Cannot be implemented by the user |
| `Comparable` | `fn compare(self, other: Self) -> Ordering` | built in for `Int` and `String`; implemented by the user for own types like any interface |
| `Display` | `fn to_string(self) -> String` (0.2) | built in for `Int`, `Float`, `Bool`, `String`; implemented by the user for own types like any interface |

1. `Equatable` and `Copyable` cannot be implemented by hand: there is no custom equality or copy
   in v1 (no operator overloading, PLAN section 3). **[PROPOSAL]**
2. When `==` or `.copy()` is used on a type that does not have it, the error names the field or
   payload that prevents it: "`Task` is not `Equatable` because field `on_done` has a function
   type". (author)
3. `Comparable` is not object-safe (it mentions `Self`), so it is a bound only.
4. A type parameter has no built-in interfaces unless they are in its bounds:
   `fn dedup<T: Equatable + Copyable>(items: List<T>) -> List<T>`.
5. Name note: G7 called the copy interface `Copy`. It is named `Copyable` to avoid confusion with
   Rust's `Copy` (implicit copies), which in wolfpack applies only to `Int`, `Float`, `Bool`
   (chapter 5).
6. **`Display`** (0.2, author, 2026-10-18) gives the text of a value for interpolated strings
   (chapter 1, section 5.3a) and `assert_eq` (chapter 7). It is object-safe, so `Display` is
   also usable as a type. Its built-in implementations are the conversions of chapter 7,
   section 4.1; for `String` it returns a copy of the string. By the coherence rule (section 8.3)
   a user cannot implement it for `List`, `Option` or other built-in types; `f"{items}"` with a
   list is an error, and the text is built with std functions (`join`).
7. **`Display` of collections** (author, 2026-10-23, D4.17). `List<T>` and `Option<T>` are
   `Display` when `T` is, and `Map<K, V>` when `K` and `V` are. `Result<T, E>` is `Display` when
   `T` is and `E` is `Display` or implements `Error` (whose `message()` then gives the text of an
   `Err`). These implementations are built in, like those of `Equatable`, and write the value as
   chapter 7, section 4.1 shows (`[1, 2, 3]`, `Some("a")`, `["Ann": 31]`, `Ok(12)`), so
   `f"{items}"` and `assert_eq` of two lists or two results work. The last sentence of rule 6 no
   longer holds.

## 10. Type inference

1. Inference is **local to one function body** (or one `config` initializer, or one test) and
   never crosses a signature **[PLAN]**.
2. Inside a body it works in both directions, as in Rust: `var names = []` followed by
   `names.push("a")` makes `names: List<String>`. A type that is still unknown at the end of the
   body is an error asking for an annotation (`let names: List<String> = []`).
3. Closure parameter types come from the expected function type (`users.filter(|u| u.active)`), or
   from annotations, or from later use in the same body.
4. Type arguments of generic calls are inferred from arguments and the expected type; when that is
   not enough, they are written `parse.<Int>(s)` (chapter 3).
5. `None`, `[]` and `[:]` need their type from context.

## 11. Patterns and exhaustiveness

1. A pattern must have the type of the scrutinee: `Some(x)` against `Option<T>` binds `x: T`.
2. Literal patterns are allowed for `Int`, `String` and `Bool` (chapter 3 forbids `Float`).
3. Both sides of `A | B` must bind the same names with the same types.
4. A guard (`if cond`) must be `Bool`. **A guarded arm never counts toward exhaustiveness.**
5. **Exhaustiveness** **[PLAN]**: the arms must cover every value of the scrutinee type. For
   `Int` and `String` this needs a `_` or a binding arm. For enums, every variant (and every
   combination for nested patterns) must be covered. The error lists an example of a missing
   pattern: "missing `Shape.Rect { .. }`".
6. **Unreachable arms** (covered entirely by earlier arms) are an error. **[PROPOSAL]** (see D4.9)
7. How a binding in a pattern relates to ownership (does `Some(x)` move or borrow) is in chapter 5.

## 12. Constant expressions

The initializer of a `config` constant (chapter 2, section 3.3) is evaluated by the compiler using
exactly the rules of this chapter. A panic during that evaluation (overflow, division by zero,
shift out of range) is a compile error.

## 13. Diagnostics

Proposed range E04xx (confirmed in chapter 9). Main codes:

| Code | Error |
| --- | --- |
| E0401 | Type mismatch (expected X, found Y) |
| E0402 | Unknown type name |
| E0403 | Wrong number of type arguments |
| E0404 | Bound not satisfied (`T` does not implement `I`) |
| E0405 | Operator not defined for these types (`"a" + 1`, `true & false`, `<` on a struct) |
| E0406 | Type not `Equatable` / not `Copyable` (names the offending field) |
| E0407 | Map key type not `Hashable` |
| E0408 | `?` used in a function with an incompatible result type |
| E0409 | Unhandled `Result` value discarded |
| E0410 | Non-exhaustive `match` (lists a missing pattern) |
| E0411 | Unreachable `match` arm |
| E0412 | Alternatives in a pattern bind different names or types |
| E0413 | Missing method / wrong signature in an interface implementation |
| E0414 | `impl` outside the module of the type |
| E0415 | Two methods with the same name on one type |
| E0416 | Interface is not object-safe but used as a type (names the method) |
| E0417 | Generic method in an interface, or interface member without `self` |
| E0418 | Infinitely sized struct |
| E0419 | Cycle in type aliases or in `config` constants |
| E0420 | Type cannot be inferred (asks for an annotation) |
| E0421 | Missing result: function may end without a value |
| E0422 | Struct literal: missing, unknown or duplicate field |
| E0423 | Integer literal out of range, constant evaluation panic |
| E0424 | Unknown field or method |
| E0425 | Expression statement whose value is not used and that is not a call, `if`, `match`, a block, `()`, `return`, `break` or `continue` |
| E0426 | `config` initializer that is not a constant expression (chapter 2, section 3.3, rule 5) |
| E0427 | `config` constant whose type is not `Int`, `Float`, `Bool` or `String` (chapter 2, section 3.3, rule 4) |
| E0428 | Wrong number of arguments in a call or a pattern, or a variant written in the wrong form |
| E0429 | A type or a module used as a value (`let x = Int`) |
| E0430 | `break` or `continue` outside a loop |
| E0431 | Implementation that cannot be written: not an interface, a derived built-in interface (`Equatable`, `Copyable`, `Hashable`), or an interface implemented twice (added in M5) |
| E0432 | A method of a struct with the name of one of its fields (section 8.3, rule 6; added in M12) |
| E0433 | A pattern that may fail in `let` or `var` (hint: `if let` or `match`; added in 0.2) |
| E0434 | Update syntax `..base` in a literal of an enum variant (added in 0.2) |
| E0435 | `break` with a value in a `while` or a `for` (added in 0.2) |
| E0436 | Unknown loop label (added in 0.2) |
| E0437 | A name after `break` that is both an enclosing label and a variable (added in 0.2) |
| E0438 | A label that repeats the label of an enclosing loop (added in 0.2) |
| E0439 | A struct literal or a struct pattern whose name is not a struct, an alias of one or a variant with named fields (section 7.1, rule 3; added after 0.3) |
| E0441 | A format specification that the type of the interpolation cannot take: a precision for a type other than `Float`, `0` for a type other than `Int` and `Float` (chapter 7, section 4.4; D4.18) |
| E0442 | A `where` that is not allowed there: on a free function, in an interface or an interface implementation, or naming a type parameter that is not one of the block (section 8.6, rule 3) |
| E0443 | A default method replaced without `override`, or `override` on a method that replaces no default (section 8.3, rule 1; D4.19) |
| E0444 | `Interface.method(x)` for a method without a default (section 8.2, rule 5; D4.19) |
| E0445 | **[DRAFT]** A super-interface that is not allowed: not an interface, `Equatable`, `Copyable` or `Hashable`, the interface itself directly or through others, or one named twice (section 8.2, rule 6; D4.20) |
| E0446 | **[DRAFT]** An interface implementation for a type that does not implement a super-interface of the interface (section 8.3, rule 7; D4.20) |
| E0447 | **[DRAFT]** A method of an interface with the name of a method of one of its super-interfaces (section 8.2, rule 6; D4.20) |

Codes E0426 to E0430 were added in M4 (author, 2026-10-09).

## 14. Register items opened by this chapter

| # | Item | Chapter |
| --- | --- | --- |
| 14 | Go: a single constraint cannot be satisfied both by `int64` (no methods) and by a struct with a `Compare` method. `fn max<T: Comparable>` used with `Int` and with a user type needs monomorphization or dictionary passing in the Go backend | 8 |
| 15 | `std` must provide a shared `Error` type so that `?` composes across std calls. **Closed**: `Error` is a built-in interface, `?` coerces to it (section 5, rule 1) | 7 |

## 15. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D4.1 | Iteration order of `Map` (`m.keys()` and so on) on every target | **answered**: insertion order |
| D4.2 | `return`, `break`, `continue` become expressions of type Never, so that `None => return 0` parses as a `match` arm (changes chapter 3 grammar) | **answered**: yes |
| D4.3 | Discarding a `Result` in an expression statement is an error | **answered**: yes |
| D4.4 | Interface members must take `self`; no associated functions in interfaces in v1 | **answered**: yes |
| D4.5 | Coherence: every `impl` is in the module of the type | **answered**: yes |
| D4.6 | Implicit coercion of a concrete value to an interface type where the interface is expected | **answered**: yes |
| D4.7 | `Float`: IEEE semantics for `==` and arithmetic; not `Comparable`, not `Hashable` | **answered**: yes |
| D4.8 | `Int` semantics: truncating `/`, sign of `%`, panics for overflow and division by zero, shift amount `0..=63`, `<<` discards bits | **answered**: yes |
| D4.9 | Unreachable `match` arm is an error | **answered**: yes |
| D4.10 | No `s[i]` on strings; `for` over `List` and ranges only (other collections through std functions returning `List`) | **answered**: yes |
| D4.12 | 0.2: the built-in interface `Display`, implemented for `Int`, `Float`, `Bool`, `String` and by the user | **answered** (author, 2026-10-18): yes |
| D4.13 | 0.2: the typing of `loop` (rules 9 and 9a), irrefutable patterns in `let` (rule 9b), `f"..."` needs `Display` (rule 11), codes E0433 to E0438 | **answered** (author, 2026-10-18): yes |
| D4.14 | `==`, `.copy()` and dropping work on values at least 1,000,000 levels deep on every target and in the VM (section 7.4, rule 3) | **answered** (author, 2026-10-23): yes, at least a million levels |
| D4.16 | Methods with bounds of their own (section 8.6): `where T: Equatable` after the result type; the error at the call is E0404 (E0406 for `Equatable` and `Copyable`), a misplaced `where` E0442; `contains`, `index_of`, `get`, `sort`, `min` and `max` become methods of `List<T>` and the free functions of `std.list` are removed | **answered** (author, 2026-10-23): `where` at the method; remove the free functions |
| D4.17 | `Display` of `List`, `Option`, `Map` and `Result` (section 9, rule 7; chapter 7, section 4.1): written as the literal would be, strings in quotes with the escapes of chapter 7, a map as a map literal in insertion order; `Result` also when its error implements `Error`, through `message()` | **answered** (author, 2026-10-23): strings in quotes, `Result` with `Error` too, maps as `["Ann": 31]`; in generic code the type argument decides (chapter 7, section 4.1, rule 4) |
| D4.18 | Format specifications in interpolations (chapter 1, section 5.3a; chapter 7, section 4.4): `[[fill] align] ["0"] [width] ["." precision]`; numbers align to the right by default, other values to the left; width counts Unicode scalar values; precision only for `Float`, rounded to the nearest with ties to even; `0` only for numbers and never with a fill or an alignment; width at most 1000, precision at most 100; codes E0117 and E0441 | **answered** (author, 2026-10-23): ties to even; `0` with an alignment forbidden; limits 1000 and 100 |
| D4.19 | Default methods in interfaces (section 8.2, rules 4 and 5; section 8.3, rules 1 and 4): a body after the signature is the default, checked once over `Self: I`; a replacement is written `override fn` (E0443); `I.m(x)` calls the default also when it is replaced (E0444 for a method without one); inherited defaults count for E0415 and E0432. Targets: the default is a function of its own that the implementing types and `I.m(x)` call (in Rust too, which cannot call a replaced trait default) | **answered** (author, 2026-10-23): replacements marked with `override`; `Named.greeting(self)` calls the default |
| D4.20 | **[DRAFT]** Super-interfaces (section 8.2, rule 6; section 8.3, rule 7; section 8.5, rule 1; chapter 8, section 5.3): `interface Greeter: Named + Display`; a type implements each super-interface in its own block (E0446 otherwise); a bound by `Greeter` gives the methods of the super-interfaces, also to its defaults; super-interfaces are interfaces of the program, `Error`, `Comparable` and `Display`, not `Equatable`, `Copyable` or `Hashable`; cycles and repeats are E0445, a method name taken from a super-interface E0447; no coercion of a `Greeter` value to a `Named` value (Rust 1.85). Reverses "no super-interfaces" of D3.3. Targets: Rust super-traits, TS `extends`, Go embedded interfaces; the dictionaries of TS and Go bounds carry those of the super-interfaces | **proposed** (2026-10-25), waiting for the author |
| D4.15 | E0439 for a struct literal or pattern of a name that is not a struct (section 7.1, rule 3), and literals and patterns through an alias of a struct (section 7.3). Before, both stopped `wolf` with an internal error | **proposed** (2026-10-23), waiting for the author |
| D4.11 | Rest: `Ordering`, Never type, string ordering by scalar value, `?` with identical `E`, recursive types rule, object safety rule, no two methods with one name, built-in interfaces table, inference rules, diagnostics | **answered**: yes |
