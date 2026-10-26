# Chapter 7: Standard library and `extern`

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter defines the standard library `std` of v1 and the `extern` section through which a module
uses libraries of a target language **[PLAN]** (section 7). How each std function and each `extern`
call is emitted per target is in chapter 8.

Decisions already taken by the author for this chapter (2026-10-07):

- `Error` is a built-in interface; `?` coerces concrete errors to it (chapter 4, section 5).
- `print` takes only a `String`; `Int`, `Float` and `Bool` have `.to_string()`.
- An `extern` declaration names its target item with one string: the full path in the notation of
  the target.
- `extern` may declare **opaque types**.

## 1. How `std` is built

1. `std` is a set of `.wolf` modules shipped with the compiler (crate `wolf_std`) **[PLAN]**. They
   follow every rule of this specification.
2. A function or method marked `intrinsic` has no body; each backend provides its implementation
   **[PLAN]**. `intrinsic` is allowed only in modules under `std` (E0701).
3. Some std functions may be written in wolfpack itself, with a body. That is invisible to users.
4. **Built-in types are declared in std** with an `intrinsic type` declaration in the `types`
   section, which has no body: `pub intrinsic type List<T>` (author, 2026-10-08). The backends know
   their representation (chapter 8). `std.string` declares `String`, `std.list` declares `List`,
   `std.map` declares `Map`, and `std.math` declares `Int` and `Float`; `Bool` and `Void` are
   declared in the prelude module. Each of these modules holds the inherent `impl` block of its
   types, so the coherence rule of chapter 4 (an `impl` lives in the module of its type) holds
   without exceptions. Like `intrinsic` functions, `intrinsic type` is allowed only in std
   (E0701).
5. **Methods of built-in types are available everywhere without an import**: a method is found
   through the type of the receiver (chapter 4, section 8.4). Importing `std.list` is needed only for
   its free functions.

Notation below: signatures are written as in wolfpack. `self` is a read receiver unless marked.

## 2. Errors

```
pub interface Error {
    fn message(self) -> String
}
```

1. `Error` is in the prelude (chapter 6). It is object-safe, so `Error` is usable as a type:
   `Result<Config, Error>`.
2. User error types implement it like any interface:
   ```
   impl {
       StorageError: Error {
           fn message(self) -> String {
               match self {
                   StorageError.Full => "storage is full"
                   StorageError.Io(m) => "io: " + m
               }
           }
       }
   }
   ```
3. std returns concrete error types that implement `Error`: `FsError` (section 7) and `ParseError`
   (section 4). `?` coerces them to `Error` where needed.
4. When `main` returns `Err(e)`, the program prints `error: ` followed by `e.message()` to standard
   error and exits with code 1 (chapter 6, section 5).
5. **`error(message)`** (0.2, author, 2026-10-18) makes an `Error` from a message, without
   declaring an error type: `return Err(error(f"{path} is empty"))`. Its `message()` returns the
   message. The value is of an internal type of std that implements only `Error`; it can be used
   only as `Error`.

## 3. Prelude functions

| Signature | Meaning |
| --- | --- |
| `fn print(s: String)` | writes `s` and a line feed to standard output (author: `String` only) |
| `fn panic(message: String) -> Never` | stops the program (below) |
| `fn assert(condition: Bool)` | panics if `condition` is `false` (below) |
| `fn assert_eq<T: Equatable + Display>(left: T, right: T)` | panics if `left != right`, showing both (below; 0.2) |
| `fn error(message: String) -> Error` | an `Error` with this message (section 2, rule 5; 0.2) |
| `fn replace<T>(mut place: T, own value: T) -> T` | stores `value` in `place` and returns the old value (chapter 5, section 3; author, 2026-10-07) |

1. **Panic output** **[PROPOSAL]**: the program writes
   `panic at <file>:<line>:<column>: <message>` to standard error and exits with code 101. The
   location is the source location in the `.wolf` file, inserted by the compiler. No target stack
   trace is printed.
   Panics raised by the language itself (overflow, index out of range, division by zero, shift out of
   range) print `panic: <message>` **without a location**, with a fixed message per kind, for example
   `panic: integer overflow` and `panic: index 5 out of range for length 3`. The exit code is also
   101. (Author, 2026-10-07: locations on every operation would make the generated code much less
   readable.)
   **Debug builds** (D7.21) **[PROPOSAL]**: `wolf build --debug`, `wolf run --debug` and
   `wolf test --debug` (also with `--vm` and `--example`) build the program so that the panics of
   the language print the location of the operation that failed, in the form of `panic`:
   `panic at src/stats.wolf:12:17: integer overflow`. The location is that of the operator
   (`+ - * / % << >>` and their compound assignments, unary `-`), of the `[` of an index, or of the
   call of a std function or method that panics (`exit` with a code out of range, `list.remove(i)`
   out of range...). Stack overflow (chapter 8, section 4.3) stays without a location. The messages,
   the exit code 101 and everything else stay the same; only the location is added. A program
   built without `--debug` is unchanged, and the generated code of a debug build may be less
   readable: the checks of every operation go through helpers that take the location
   (chapter 8, section 4.3). The flag is not written to `wolfpack.toml`.
2. **`assert`** failing panics with the message `assertion failed: <source text of the condition>`.
   The compiler inserts the text. There is no second message parameter (no overloading).
   **`assert_eq(a, b)`** (0.2) failing panics with
   `assertion failed: <text of a> == <text of b> (left: <a>, right: <b>)`, where the texts are the
   source of the two arguments and the values are shown with `Display`:
   `panic at src/main.wolf:7:5: assertion failed: total(cart) == 30 (left: 25, right: 30)`.
3. Output is identical on every target, so it can be compared in tests.

## 4. `Int`, `Float`, `Bool`, `String` (modules `std.math`, `std.string`)

### 4.1 Conversions and formatting

Since 0.2 the `to_string` methods of this table are the implementations of `Display` (chapter 4,
section 9); `String` implements it too and returns a copy.

| Method | Result |
| --- | --- |
| `Int.to_string(self) -> String` | decimal, `-` for negatives |
| `Int.to_float(self) -> Float` | nearest `Float` |
| `Float.to_int(self) -> Int` | truncated toward zero; panics on NaN, infinities and values outside `Int` |
| `Float.to_string(self) -> String` | canonical format, below |
| `Bool.to_string(self) -> String` | `true` or `false` |
| `String.parse_int(self) -> Result<Int, ParseError>` | optional `-`, decimal digits only |
| `String.parse_float(self) -> Result<Float, ParseError>` | the float literal syntax of chapter 1, with optional `-`, plus `NaN`, `inf`, `-inf` |

`ParseError` is a struct implementing `Error`; its message names the input.

**Text of collections** (author, 2026-10-23, chapter 4, section 9, rule 7): the `to_string` of a
list, an option, a map and a result writes it as the literal that would build it, with the
`to_string` of the parts:

| Value | Text |
| --- | --- |
| `[1, 2, 3]`, `[]` | `[1, 2, 3]`, `[]` |
| `[1.0, 2.5]` | `[1.0, 2.5]`: a `Float` part in the canonical format below |
| `["a", "b\n"]` | `["a", "b\n"]`: a part of type `String` is in quotes and escaped (below) |
| `Some(2.5)`, `None` | `Some(2.5)`, `None` |
| `["Ann": 31, "Bob": 25]`, an empty map | `["Ann": 31, "Bob": 25]` in insertion order, `[:]` |
| `Ok(12)`, `Err("bad")` | `Ok(12)`, `Err("bad")`: an error of a `Display` type by its `to_string` |
| `Err(e)` with `e` a `ParseError` | `Err(not a number: x)`: an error that is not `Display` but implements `Error`, by `message()`, without quotes |
| `[Some([1]), None]` | `[Some([1]), None]`: parts that are collections are written the same way |

1. A part whose type is `String` is written in `"` quotes. Inside them `"` is `\"`, `\` is
   `\\`, line feed, carriage return, tab and U+0000 are `\n`, `\r`, `\t` and `\0`, and the
   other control characters (U+0001 to U+001F and U+007F) are `\u{` with their code in lower-case
   hexadecimal without leading zeros `}` (`\u{1b}`); every other character is written as it is.
   A `String` written by itself, outside a collection, keeps no quotes: `f"{name}"` is `Ann`.
2. A part of any other type is written by its own `to_string`, without quotes: a list of `Point`s
   whose `to_string` gives `(1, 2)` is `[(1, 2), (3, 4)]`, and a list of the interface type
   `Display` writes each value as its `to_string` gives it.
3. Parts are separated by `, `, and a key from its value by `: `; nothing is ever cut.
4. In generic code the type argument decides (author, 2026-10-23): a part of type `T` is written
   as a part of its type argument would be, so `show<T: Display>(items: List<T>)` writes
   `["a"]` for a `List<String>` and `[a]` for a `List<Display>`; an `Err` of type `E` is written
   by its `to_string` when the type argument of `E` is `Display`, and by `message()` otherwise,
   also where the bounds of `E` name only `Error`.

**Canonical `Float` format** (G9) **[PROPOSAL]** (see D7.2):

1. Digits: the shortest decimal that reads back as the same value (round trip).
2. If the decimal exponent of the value is between -6 and 20 inclusive, plain notation is used, and a
   value with no fractional part gets `.0`: `1.0`, `0.25`, `100.0`, `0.000001`.
3. Otherwise scientific notation, with the mantissa having a `.` only if it has more than one digit:
   `1e21`, `1.5e-7`, `-2.5e100`.
4. Special values: `NaN`, `inf`, `-inf`; negative zero is `-0.0`.

### 4.2 Arithmetic helpers (module `std.math`)

There is no overloading, so `Int` and `Float` each get their own methods **[PROPOSAL]** (see D7.5):

| Method | Notes |
| --- | --- |
| `Int.abs(self) -> Int` | panics for the minimum value |
| `Int.min(self, other: Int) -> Int`, `Int.max(...)` | |
| `Int.pow(self, exp: Int) -> Int` | panics on overflow and on negative `exp` |
| `Float.abs`, `Float.min`, `Float.max` | IEEE; `min`/`max` with a NaN return the other operand |
| `Float.pow(self, exp: Float) -> Float`, `Float.sqrt(self) -> Float` | IEEE |
| `Float.floor`, `Float.ceil`, `Float.round` | `round` rounds half away from zero |
| `Float.is_nan(self) -> Bool` | |

`std.math` also has constants: `pi`, `e` (`Float`), `int_min`, `int_max` (`Int`).

### 4.3 Strings (module `std.string`)

All positions and lengths count Unicode scalar values (G1).

| Method | Notes |
| --- | --- |
| `len(self) -> Int`, `is_empty(self) -> Bool` | |
| `contains(self, part: String) -> Bool`, `starts_with`, `ends_with` | |
| `find(self, part: String) -> Option<Int>` | position of the first occurrence |
| `split(self, separator: String) -> List<String>` | an empty separator panics |
| `lines(self) -> List<String>` | splits on `\n` and `\r\n`; no trailing empty line |
| `chars(self) -> List<String>` | one string per scalar value |
| `trim(self) -> String` | removes ASCII whitespace at both ends |
| `replace(self, from: String, to: String) -> String` | all occurrences; empty `from` panics |
| `repeat(self, times: Int) -> String` | negative `times` panics |
| `to_upper_ascii(self) -> String`, `to_lower_ascii(self) -> String` | ASCII letters only, see D7.3 |

Free function: `string.join(parts: List<String>, separator: String) -> String`.

Full Unicode case mapping is not in v1, because Go, Rust and JS disagree on it (for example `ß`).
**[PROPOSAL]** (see D7.3)

### 4.4 Format specifications (author, 2026-10-23, D4.18)

An interpolation with a format specification (chapter 1, section 5.3a), `f"{value:spec}"`, takes
the text of the value (its `Display`, chapter 4, section 9) and changes it as the specification
says, on every target and in the VM:

1. **Precision** `.p`, only for `Float` (else E0441): the value is written in plain notation with
   exactly `p` digits after the point, rounded to the nearest; a value exactly halfway rounds to
   the even digit, on its exact binary value (`{2.5:.0}` is `2`, `{0.125:.2}` is `0.12`, and
   `{2.675:.2}` is `2.67` because the exact value of `2.675` is below it). This is the rule of
   `printf`, Python, Go and Rust; `Float.round` (section 4.2) rounds halves away from zero
   instead. `{x:.0}` has no point: `3`. A negative zero keeps its sign: `{-0.0:.1}` is `-0.0`.
   `NaN`, `inf` and `-inf` are written as without a precision.
2. **Width** `w`: when the text has fewer than `w` Unicode scalar values, it is padded to `w` with
   the fill character (a space by default). A longer text is never cut.
3. **Alignment**: `<` pads on the right, `>` on the left, `^` on both sides with the extra
   character on the right. Without an alignment, `Int` and `Float` align to the right and every
   other type to the left.
4. **`0`**, only for `Int` and `Float` (else E0441) and never with a fill or an alignment (E0117):
   pads with zeros after the sign, as `{n:05}` gives `-0042` for -42. `NaN` and the infinities are
   padded with spaces instead: `{x:05}` of `NaN` is `  NaN`.

| Interpolation | Text |
| --- | --- |
| `f"{3.14159:.2}"` | `3.14` |
| `f"[{42:>5}]"`, `f"[{42:<5}]"`, `f"[{"ab":^6}]"` | `[   42]`, `[42   ]`, `[  ab  ]` |
| `f"{7:03}"`, `f"{-7:03}"` | `007`, `-07` |
| `f"{"x":*>4}"` | `***x` |
| `f"{1.5:8.3}"` | `   1.500` |
| `f"{[1, 2]:>8}"` | `  [1, 2]` |

## 5. Lists (module `std.list`)

Methods of `List<T>`, available without import:

| Method | Notes |
| --- | --- |
| `List.new() -> List<T>` | same as `[]` |
| `len(self) -> Int`, `is_empty(self) -> Bool` | |
| `push(mut self, own item: T)` | |
| `pop(mut self) -> Option<T>` | removes the last element |
| `insert(mut self, index: Int, own item: T)` | index in `0..=len`, else panic |
| `remove(mut self, index: Int) -> T` | index in `0..len`, else panic |
| `swap(mut self, i: Int, j: Int)`, `reverse(mut self)`, `clear(mut self)` | |
| `append(mut self, own other: List<T>)` | moves all elements of `other` to the end |
| `map<U>(self, f: fn(T) -> U) -> List<U>` | `f` reads each element |
| `filter(own self, keep: fn(T) -> Bool) -> List<T>` | **consumes** the list, see D7.4 |
| `any(self, f: fn(T) -> Bool) -> Bool`, `all(...)`, `count(self, f: fn(T) -> Bool) -> Int` | |

Operations that need a bound on `T` are methods with `where` bounds (chapter 4, section 8.6;
D4.16, author, 2026-10-23); until 0.3 they were free functions of `std.list`, which are removed:

| Method | |
| --- | --- |
| `contains(self, item: T) -> Bool where T: Equatable` | |
| `index_of(self, item: T) -> Option<Int> where T: Equatable` | |
| `get(self, index: Int) -> Option<T> where T: Copyable` | a copy, `None` out of range |
| `sort(mut self) where T: Comparable` | **stable** on every target |
| `min(self) -> Option<T> where T: Comparable + Copyable`, `max` | |

Free functions of `std.list`:

| Function | |
| --- | --- |
| `sort_by<T>(mut items: List<T>, compare: fn(T, T) -> Ordering)` | stable |
| `sum(items: List<Int>) -> Int` | panics on overflow |
| `range(from: Int, to: Int) -> List<Int>` | `from..to` as a list |

## 6. Maps (module `std.map`)

Methods of `Map<K, V>` (`K: Hashable`), available without import. Iteration order is insertion order
(ADR 0003).

| Method | Notes |
| --- | --- |
| `Map.new() -> Map<K, V>` | same as `[:]` |
| `len(self) -> Int`, `is_empty(self) -> Bool` | |
| `contains_key(self, key: K) -> Bool` | |
| `insert(mut self, own key: K, own value: V) -> Option<V>` | returns the replaced value |
| `remove(mut self, key: K) -> Option<V>` | |
| `keys(self) -> List<K>` | every `Hashable` type is `Copyable`, so these are copies |
| `entries(own self) -> List<Entry<K, V>>` | consumes the map |
| `clear(mut self)` | |

`Entry<K, V>` is a `pub struct` with `pub` fields `key` and `value`.

Free functions in `std.map`: `get<K, V: Copyable>(m: Map<K, V>, key: K) -> Option<V>` and
`values<K, V: Copyable>(m: Map<K, V>) -> List<V>`. Reading a value in place, without a copy, is
`m[key]` (panics if missing, chapter 4).

## 7. Input, output, files, process

**`std.io`**: `print` (also in the prelude); `write(s: String)` without a line feed;
`eprint(s: String)` to standard error with a line feed; `read_line() -> Option<String>` (`None` at end
of input, the line without its terminator); `args() -> List<String>` (the arguments without the
program name).

**`std.fs`**: `read_text(path: String) -> Result<String, FsError>`,
`write_text(path: String, content: String) -> Result<Void, FsError>`, `exists(path: String) -> Bool`.
Paths are relative to the current directory. Files are UTF-8; invalid UTF-8 is an error.

```
pub enum FsError {
    NotFound(String)          // the path
    PermissionDenied(String)
    Other(String)             // a message
}
```

`FsError` implements `Error` with fixed English messages, the same on every target:
`file not found: <path>`, `permission denied: <path>`, `<message>` for `Other`. The text of `Other`
comes from the target and may differ between targets. **[PROPOSAL]**

**`std.process`**: `exit(code: Int) -> Never` (`code` in `0..=255`, else panic) and
`env(name: String) -> Option<String>`. Inside a test, `exit` fails that test instead of ending the
run (chapter 8, section 2, rule 7; D8.17).

## 8. `extern`

### 8.1 Declarations

```
extern_block = ( "ts" | "go" | "rust" ) "{" extern_decl+ "}"
extern_decl  = visibility "fn" IDENT "(" params? ")" ( "->" type )? "from" STRING
             | visibility "type" IDENT "from" STRING
             | "struct" IDENT "from" STRING                       (0.2, section 8.4a)
```

```
extern {
    go {
        pub fn upper(s: String) -> String from "strings.ToUpper"
    }
    ts {
        pub fn upper(s: String) -> String from "String.prototype.toUpperCase"
    }
    rust {
        pub fn upper(s: String) -> String from "str::to_uppercase"
    }
}
```

1. A declared function is called like any function of the module: `upper(name)`.
2. `pub` makes an extern name visible to other modules, like any declaration.
3. Extern functions are not generic and have no `mut` parameters. **[PROPOSAL]** (see D7.7)
4. A name may appear at most once per target block. The same name in several blocks must have
   **identical signatures** **[PLAN]** (E0703).

### 8.2 The path string

The string after `from` is the full path in the notation of the target (author):

| Target | Form | Examples |
| --- | --- | --- |
| Go | `import/path.Name`; methods as Go method expressions | `"strings.ToUpper"`, `"crypto/sha256.Sum256"`, `"(*bytes.Buffer).WriteString"` |
| TS | `module#export` for a module export (`#default` for the default export); a path without `#` is a global | `"some-lib#parse"`, `"Math.max"`, `"String.prototype.toUpperCase"` |
| Rust | a Rust path; methods in function form | `"std::cmp::max"`, `"str::to_uppercase"`, `"sha2::Sha256::new"` |

For a **method** path (a Go method expression, a TS path through `.prototype.`, a Rust associated
function taking `self`), the first wolfpack parameter becomes the receiver. A path that does not fit
the target's form is an error (E0705).

The path of a Go **opaque type** may start with `*` for a pointer type (author, 2026-10-14):
`type Regex from "*regexp.Regexp"`.

### 8.2a Code of the project: `native:` paths

Added in 0.2 (author, 2026-10-18). A path string that starts with `native:` names code in the
`native/<target>/` directory of the project (chapter 6, section 7, rule 7). The rest of the path
has the form of the target:

| Target | Form | Example | Names |
| --- | --- | --- | --- |
| TS | `native:<file>#<export>` | `"native:text/slug#slugify"` | the export `slugify` of `native/ts/text/slug.ts` |
| Go | `native:<package dir>.<Name>` | `"native:slug.Slugify"` | the function `Slugify` of the package in `native/go/slug/` |
| Rust | `native:<module path>::<name>` | `"native:slug::slugify"` | the function `slugify` of `native/rust/slug.rs` |

```
extern {
    ts {
        pub fn slugify(s: String) -> String from "native:slug#slugify"
    }
    go {
        pub fn slugify(s: String) -> String from "native:slug.Slugify"
    }
    rust {
        pub fn slugify(s: String) -> String from "native:slug::slugify"
    }
}
```

1. All the rules of extern declarations apply unchanged: the boundary types (section 8.4),
   `Result` and panics, the contract of read parameters, opaque types
   (`type Index from "native:search.Index"`) and target binding (section 8.5).
2. The file, the Go package directory or the Rust module named by the path must exist in
   `native/<target>/` (E0709, at the path string). Whether the function exists and has the declared
   signature is checked by `tsc`, `go build` or `cargo`. An error that their tool reports in a
   native file, or in a generated call that does not match the native function, is not a bug of
   `wolf`: it is E0806 (chapter 8), with the output of the tool.
3. Native code may use the dependencies of `[extern.<target>.dependencies]` and the standard
   library of its language, but not the runtime of the generated code, whose contents are
   internal.
4. A native file or package whose generated path is taken by the compiler (the runtime, or a
   module of `src/`) is an error (E0710, naming both).
5. `wolf test` does not run tests written in the native files; they run with the tools of the
   target in `out/<target>/`.

### 8.3 Opaque types

`type Hasher from "hash.Hash"` (Go) declares an **opaque type** (author):

1. It is a move type: it can be stored in variables, fields and lists, moved and passed to extern
   functions.
2. It is not `Copyable`, `Equatable` or `Hashable`, has no fields and no methods of its own. User
   code uses it only through extern functions (E0707). An opaque type of a native module also has
   the methods and associated functions that the reader finds (section 8.7, rule 17; 0.2).
3. A type declared with the same name in all three blocks is portable, like a function.

### 8.4 The boundary

1. Only these types may appear in extern signatures **[PLAN]**: `Int`, `Float`, `Bool`, `String`,
   `Void`, `List<B>`, `Option<B>`, `Result<B, Error>`, `Map<K, B>` (0.2) and opaque types, where
   `B` is again one of these and the key `K` is `Int`, `String` or `Bool` (E0702). The values of a
   `Map` contain no opaque type, since a map is copied at the boundary and opaque values cannot
   be copied (author, 2026-10-19). Structs, enums and interfaces do not cross the boundary.
2. The error type of a `Result` at the boundary is always `Error` **[PROPOSAL]**: a Go `error`, a
   value thrown in TS, or a Rust `E: Display` becomes an `Error` whose `message()` is the target's
   text.
3. If the declared result is not a `Result`, a Go non-nil error, a TS exception or a Rust panic in the
   extern call becomes a wolfpack panic.
4. Read parameters are passed without copying. The extern code must not change or keep them; this is
   a contract that the compiler cannot check. **[PROPOSAL]** (see D7.8)
5. The exact conversions (`Int` to `bigint`, `Option` to `*T`, and so on) are in chapter 8.
6. A `Map` crosses the boundary as a copy in the map type of the target (chapter 8, section 8),
   built on every call (0.2). A map that extern code returns is in **increasing order of its
   keys** on every target, whatever order the target gave it: numbers by value, strings as `<`
   compares them (chapter 4), `false` before `true`. A key that comes twice keeps its last value.
   (author, 2026-10-19, D7.13)
7. A **parameter** of an extern function may have a function type, a **callback**, such as
   `fn each(items: List<String>, f: fn(String) -> Void) from "..."` (0.2, author, 2026-10-19,
   D7.14):
   - its parameters and result are `Int`, `Float`, `Bool`, `String`, `Void`, `List` of these, or
     opaque types; its parameters are read parameters (no `mut` or `own`);
   - a callback is passed only as a parameter: not as a result, and not inside a `List`, an
     `Option` or a `Map` (E0702);
   - the extern code may call the callback only **during the call** of the extern function, and
     must not keep it. Like rule 4, this is a contract that the compiler cannot check;
   - the values that extern code passes to the callback, and the result the callback gives back,
     are converted by the rules of chapter 8, section 8, in the other direction;
   - a panic in the callback passes through the extern code and is a panic of the caller again,
     also when the extern function returns a `Result`: it never becomes an `Err`.

### 8.4a Records

Added in 0.2 (author, 2026-10-19, D7.15). A struct of the module crosses the boundary **by value**
when a target block binds it to a struct type of the target:

```
types {
    pub struct Point {
        pub x: Float
        pub y: Float
    }
}

extern {
    ts {
        struct Point from "native:geo#Point"
        fn distance(a: Point, b: Point) -> Float from "native:geo#distance"
    }
    go {
        struct Point from "native:geo.Point"
        fn distance(a: Point, b: Point) -> Float from "native:geo.Distance"
    }
    rust {
        struct Point from "native:geo::Point"
        fn distance(a: Point, b: Point) -> Float from "native:geo::distance"
    }
}
```

1. `struct Name from "path"` names a struct declared in the `types` section of the same module,
   not generic (E0711). It declares no new name: the struct stays an ordinary wolfpack struct,
   with its fields, methods, literals and derived interfaces. The path names a type of the target
   in the form of section 8.2 (or 8.2a); in TS it may be an interface or a type alias.
2. A bound struct is a boundary type of that target block (rule 1 of section 8.4): it may be a
   parameter or a result of an extern function, and an element of a `List`, an `Option` or a
   `Map`. In a block that does not bind it, it does not cross (E0702).
3. Every field of a bound struct has a type that may cross the boundary: the types of rule 1 of
   section 8.4 without opaque types, or another struct bound in the same block (E0702, at the
   field). A record is copied field by field on every call, and opaque values cannot be copied.
4. The target type has exactly these fields, named by the naming rules of chapter 8: `max_size`
   is `maxSize` in TS, `MaxSize` in Go (always exported) and `max_size` in Rust. Whether it does
   is checked by the tools of the target (E0806 for native code).
5. Methods of the target type are not reachable through the record; a target type with methods
   that wolfpack code needs is declared as an opaque type instead.

### 8.5 Target binding of modules

**[PLAN]**:

1. A module whose extern names are declared for only some targets is **bound** to those targets.
   A module with no `extern` section, or whose every extern name is declared in all three blocks, is
   portable.
2. Binding is transitive: a module that imports a bound module is bound to the same targets (the
   intersection, if it imports several).
3. Building a project for a target checks every module reachable from the entry module. A module not
   available for that target is an error that names the module, its `extern` section and the import
   chain from the entry (E0704).

### 8.6 Dependencies

`[extern.<target>.dependencies]` in the manifest **[PLAN]** lists target packages and versions. They
are copied verbatim into `package.json`, `go.mod` or `Cargo.toml` (chapter 8); `wolf` does not check
them. Packages of a target's own standard library need no entry.

### 8.7 Native modules: declarations read from the target code

Added in 0.2 (author, 2026-10-20, D7.16). A native module (chapter 6, section 1a) has no wolfpack
declarations: the compiler reads the **declarations** of its files, never their bodies, and needs
no tool of the target for it, so `wolf check` works without Node.js, Go or Cargo. Each item it can
map becomes a name of the module; every other item is not available, and using it is E0713 with
the reason (`its parameter is a &mut reference`), so nothing is left out silently.

1. **Readers.** In 0.2 the compiler reads Rust (step N3a), Go (step N3b, rules 8 to 11;
   author, 2026-10-20, D7.17) and TypeScript (step N3c, rules 12 to 16; D7.19).
2. **Rust items.** At the top level of the module file, outside `mod` blocks and items with a
   `#[cfg(...)]` attribute:
   - a `pub fn`, not generic except for callback parameters (rule 4), not `async` and not
     `unsafe`, becomes a function of the same name;
   - a `pub struct` with named fields that are all `pub` and of the field types of rule 3 becomes a
     **record**: a wolfpack struct with the same fields, built, read and compared like any struct,
     and copied field by field at the boundary (as in section 8.4a);
   - any other `pub struct`, a `pub enum` or a `pub type` becomes an **opaque type** (section 8.3).
   Methods in `impl` blocks follow rule 17.
3. **Types.** Each Rust type maps to a wolfpack type; a parameter is always a read parameter:

   | Wolfpack | Rust |
   | --- | --- |
   | `Int` | `i8` to `i64`, `u8` to `u64`, `isize`, `usize` (converted with a check, chapter 8, section 8) |
   | `Float` | `f64`; `f32` (converted with `as`) |
   | `Bool` | `bool` |
   | `String` | `&str`, `String`, `&String` |
   | `Void` | `()`, or no result |
   | `List<T>` | `&[T]`, `Vec<T>`, `&Vec<T>` |
   | `Map<K, V>` | `&HashMap<K, V>`, `HashMap<K, V>`; as a result also `BTreeMap<K, V>`; keys `Int`, `String` or `Bool` |
   | `Option<T>` | `Option<T>` |
   | `Result<T, Error>` | `Result<T, E>` as a result; `E` must implement `Display`, whose text is the message |
   | a record or opaque type of the module | the struct, enum or type itself as a result; `&` of it as a parameter, since values of opaque types cannot be copied (author, 2026-10-20) |

   A `char`, a tuple, a `&mut` reference, a lifetime parameter other than elided ones, a type from
   another module or crate, and every other type make the item not available.
4. **Callbacks.** A parameter of type `impl Fn(A, ..) -> R`, `&dyn Fn(A, ..) -> R`, or of a type
   parameter `F` bounded by `Fn(A, ..) -> R` is a callback `fn(A, ..) -> R` (section 8.4, rule 7).
5. **Names.** The Rust names are kept: they already follow the wolfpack style. A name that is a
   reserved keyword of wolfpack makes the item not available.
6. **Calls.** A function of a native module is called as if it were declared in a `rust` block
   with the path `native:<module path>::<name>`, in a `go` block with the path
   `native:<package dir>.<Name>`, or in a `ts` block with the path `native:<file>#<name>`
   (section 8.2a): the conversions of chapter 8, section 8 apply, with the integer and float types
   the reader found, and `cargo`, `go` or `tsc` checks the rest (E0806).
7. A Rust file that cannot be parsed is E0714, with the message of the parser and the position in
   the file.
8. **Go items.** The `.go` files directly in `native/go/<path>/` are read, except `_test.go` files
   and files with a `//go:build` line. At their top level:
   - an exported `func` without a receiver and without type parameters becomes a function; its
     name becomes `snake_case` (`Slugify` is `slugify`, `CountWords` is `count_words`, `ParseURL`
     is `parse_url`);
   - an exported `type X struct { ... }` whose fields are all exported and of the field types of
     rule 9 becomes a record (as in rule 2); its fields become `snake_case` like function names
     (`MaxSize` is `max_size`, `ID` is `id`), and the generated code uses the Go names. A struct
     whose field names give the same `snake_case` name twice (`URL` and `Url`) is not available.
     Any other exported struct type becomes an opaque type used as `*X`; any other exported named
     type an opaque type used as `X`.
   Methods follow rule 17.
9. **Go types.** A parameter is always a read parameter:

   | Wolfpack | Go |
   | --- | --- |
   | `Int` | `int`, `int8` to `int64`, `uint`, `uint8` to `uint64` (converted with a check) |
   | `Float` | `float64`; `float32` (converted) |
   | `Bool`, `String` | `bool`, `string` |
   | `Void` | no result |
   | `List<T>` | `[]T` |
   | `Map<K, V>` | `map[K]V`; keys `Int`, `String` or `Bool` |
   | `Option<T>` | the results `(T, bool)` |
   | `Result<T, Error>` | the results `(T, error)`; `Result<Void, Error>` for the result `error` |
   | a callback | `func(A, ..) R`, with the forms of rule 4 in Go types (`int64`, `float64`, `bool`, `string`, `[]T`, `*X`) |
   | an opaque type `X` of the module | `*X` for a struct type, `X` for another named type, as declared by rule 8 |

   An `Option` parameter, a pointer to anything but a struct of the module, an interface (other
   than `error`), a channel, an array, a variadic parameter, a type from another package, and other
   result lists make the function not available.
10. **Agreement.** A name declared in several targets must map to the same wolfpack signature in
    each, and a record to the same fields in the same order with the same types (D7.18); otherwise
    the name is not available, and importing the module reports E0715 with both signatures. A
    TypeScript `number` is first settled by rule 14. A name declared in only some targets binds
    the importing module to them (E0704).
11. A Go file that cannot be parsed is E0714.
12. **TypeScript items.** The file `native/ts/<path>.ts` is read (author, 2026-10-21, D7.19). At
    its top level, a declaration with the `export` keyword:
    - `export function` (or `export declare function`) with a type on every parameter and a
      written result type, not generic, not `async`, not a generator and without overloads,
      becomes a function; its name becomes `snake_case` (`slugify` is `slugify`, `countWords` is
      `count_words`, `parseURL` is `parse_url`);
    - `export interface X { ... }` or `export type X = { ... }` whose members are all fields (no
      methods, no index signatures, no optional fields) of the field types of rule 13 becomes a
      record (as in rule 2); its fields become `snake_case`, and the generated code uses the TS
      names, as for Go (D7.18). Two fields with the same `snake_case` name make it not available;
    - `export class X`, `export enum X`, and any other exported `interface` or `type` become an
      opaque type `X`, passed and returned as it is.
    Other exports (`export default`, `export { a as b }`, `export const`, re-exports from other
    files) are not read; methods of classes follow rule 17. Two exports whose names give the same
    `snake_case` name are not available.
13. **TypeScript types.** A parameter is always a read parameter:

    | Wolfpack | TypeScript |
    | --- | --- |
    | `Int` | `bigint` (passed as it is); `number` when rule 14 says so (converted with a check) |
    | `Float` | `number` |
    | `Bool`, `String` | `boolean`, `string` |
    | `Void` | the result `void` |
    | `List<T>` | `T[]`, `readonly T[]`, `Array<T>`, `ReadonlyArray<T>` |
    | `Map<K, V>` | `Map<K, V>`, `ReadonlyMap<K, V>`; keys `Int`, `String` or `Bool`; as a result also `Record<string, V>` |
    | `Option<T>` | `T \| undefined`, and a parameter `x?: T`; as a result also `T \| null` and `T \| null \| undefined` |
    | a callback | `(a: A, ..) => R` with the types of section 8.4, rule 7 |
    | a record or opaque type of the module | its name |

    A TS function cannot declare that it throws, so its result is never a `Result`: an exception
    is a panic (section 8.4, rule 3). `any`, `unknown`, `object`, other unions, tuples, literal
    types, intersections, `Promise`, a type imported from another file, and every other type make
    the item not available.
14. **The `number` rule.** A `number` is `Float`, unless another target of the same native module
    gives `Int` at the same place (a parameter, the result, a field, or inside one of them): then it
    is `Int`, passed as a `number` and checked to be a safe integer (chapter 8, section 8). TS code
    of a module without another target writes `bigint` for an `Int`.
15. **Calls.** An `Int` passed to a `bigint` comes as it is; a `number` that comes back as an
    `Int` must be an integer, as for a written extern function (chapter 8, section 8).
16. A TypeScript file that cannot be parsed is E0714. Only `.ts` files are read: `.tsx`, `.js` and
    `.d.ts` files of `native/ts/` are copied, not read.
17. **Methods** (author, 2026-10-21, D7.20). The methods of an opaque type of the module become
    methods of the opaque type in wolfpack, called as `index.add(word)`, and its functions without
    a receiver become associated functions, called as `slug.Index.new()` (chapter 3, section 1):

    | | Rust | Go | TypeScript |
    | --- | --- | --- | --- |
    | method | `pub fn name(&self, ..)` in an inherent `impl X` block | `func (x *X) Name(..)` for a struct type, `func (x X) Name(..)` | a public method of `export class X`, not `static` |
    | associated function | `pub fn name(..)` without `self` in an inherent `impl X` block | none | a public `static` method; the `constructor` is the associated function `new` |

    Names become `snake_case` as for functions. The parameters and the result follow rules 3, 9
    and 13; the receiver is a read receiver `self`. A receiver `&mut self` or `self` (Rust), a Go
    value receiver on a struct type used as `*X`, a TypeScript getter, setter, `async` or private
    method, a generic method, and a method of a record or of a trait `impl` block are not
    available. Methods take part in the agreement of rule 10, type by type: a method declared in
    several targets maps to the same signature in each (E0715). A method or associated function
    used in a module binds it to the targets that declare it (E0704). Written `extern` blocks have
    no syntax for methods in 0.2: the method form of a target function there stays a function
    whose first parameter is the receiver (section 8.2).

## 9. Diagnostics

Proposed range E07xx (confirmed in chapter 9).

| Code | Error |
| --- | --- |
| E0701 | `intrinsic` outside `std` |
| E0702 | Type not allowed at the extern boundary |
| E0703 | Different signatures for one extern name in different target blocks |
| E0704 | Module not available for the target (shows the import chain) |
| E0705 | Malformed path string for the target |
| E0706 | Generic extern function or `mut` parameter in an extern function |
| E0707 | Operation not available on an opaque type |
| E0708 | Extern name declared twice in one target block |
| E0709 | A `native:` path names a file, a package or a module that `native/<target>/` does not have (0.2) |
| E0710 | A native file or package takes a path of the generated project (0.2) |
| E0712 | `pub` import of a native module (0.2; chapter 6, section 1a) |
| E0713 | A name of a native module that the compiler could not map; the reason is shown (0.2) |
| E0714 | A native file that the compiler cannot read: a syntax error in the target language (0.2) |
| E0715 | A name of a native module that maps to different wolfpack signatures in different targets (0.2) |
| E0711 | `struct Name from "path"` in an extern block does not name a non-generic struct of the module (0.2) |
| E0716 | `wolf check --native <module>` names no native module of the project (0.2; chapter 8, section 8b) |

## 10. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D7.1 | Panic and assertion output format with source location; exit code 101; no target stack traces | **answered**: yes; later narrowed: built-in panics have no location |
| D7.2 | Canonical `Float` format (section 4.1) | **answered**: yes |
| D7.3 | Only ASCII case mapping in v1 (`to_upper_ascii`, `to_lower_ascii`) | **answered**: yes |
| D7.4 | Operations needing a bound are free functions in `std.list` / `std.map`; `filter` consumes the list (per-method bounds go to `ideas.md`) | **answered**: yes |
| D7.5 | Math as methods of `Int` and `Float`, constants in `std.math` | **answered**: yes |
| D7.6 | std error types `FsError` and `ParseError` with fixed messages | **answered**: yes |
| D7.7 | Extern functions: no generics, no `mut` parameters; result error type is `Error`; target errors and exceptions become `Err` or a panic | **answered**: yes |
| D7.8 | Read parameters are passed to extern code without copies (unchecked contract) | **answered**: yes |
| D7.9 | Rest: method lists of `String`, `List`, `Map`; `std.io`, `std.fs`, `std.process`; target binding; diagnostics | **answered**: yes |
| D7.10 | 0.2: `error(message) -> Error` and `assert_eq` with `Display`, in the prelude | **answered** (author, 2026-10-18): yes |
| D7.11 | 0.2: the panic text of `assert_eq` (section 3, rule 2) | **answered** (author, 2026-10-18): yes |
| D7.12 | 0.2: code of the project in `native/<target>/`, called with `native:` paths (section 8.2a; the recommendations Q1 to Q3 of `docs/plans/native-code.md`) | **answered** (author, 2026-10-18): yes |
| D7.13 | 0.2: `Map<K, B>` at the extern boundary, keys `Int`, `String`, `Bool`, copied on every call; maps returned by extern code in key order on every target (section 8.4, rules 1 and 6) | **answered** (author, 2026-10-19): yes |
| D7.14 | 0.2: callbacks, parameters of a function type of extern functions, called only during the call (section 8.4, rule 7) | **answered** (author, 2026-10-19): yes |
| D7.15 | 0.2: records: a struct of the module bound to a struct type of each target with `struct Name from "path"`, copied field by field (section 8.4a), code E0711 | **answered** (author, 2026-10-19): yes |
| D7.16 | 0.2: native modules: declarations read from Rust code (section 8.7), the type mapping of rule 3, codes E0713 and E0714 | **answered** (author, 2026-10-20): yes |
| D7.17 | 0.2: native modules read from Go code (section 8.7, rules 8 to 11), the agreement of targets, code E0715 | **answered** (author, 2026-10-20): yes |
| D7.18 | 0.2: records of native modules: fields of the types the reader found (any integer type, converted with a check; `float32`, `f32`), Go fields with their Go names, and agreement of the fields between targets (section 8.7, rules 8 and 10; chapter 8, section 8a) | **answered** (author, 2026-10-21): yes |
| D7.19 | 0.2: native modules read from TypeScript (section 8.7, rules 12 to 16): exported functions, records from interfaces and object types, opaque classes; `number` is `Float` unless another target says `Int`; no `Result` results | **answered** (author, 2026-10-21): yes |
| D7.20 | 0.2: methods of opaque types of native modules (section 8.7, rule 17): read-receiver methods become methods called `x.name()`, functions without a receiver and TS constructors become associated functions `Type.name()`; the alternative is plain functions of the module (`index_add(index, word)`), which needs no change to opaque types | **answered** (author, 2026-10-21): methods as methods |
| D7.21 | Debug builds (section 3, rule 1): `--debug` for `wolf build`, `wolf run` and `wolf test` (and with `--vm`) adds the location of the failing operation to the panics of the language, `panic at <file>:<line>:<column>: <message>`; stack overflow stays without one; builds without the flag do not change. Alternatives: a setting in `wolfpack.toml` (`[build] debug = true`); locations always in the VM only | **proposed** (2026-10-26), waiting for the author |
