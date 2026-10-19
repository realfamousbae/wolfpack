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
   readable. A debug build flag with locations is in `docs/ideas.md`.)
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

Operations that need a bound on `T` are **free functions** in `std.list`, because methods cannot add
bounds to the type parameters of their `impl` block (chapter 3, D3.2) **[PROPOSAL]** (see D7.4):

| Function | |
| --- | --- |
| `contains<T: Equatable>(items: List<T>, item: T) -> Bool` | |
| `index_of<T: Equatable>(items: List<T>, item: T) -> Option<Int>` | |
| `get<T: Copyable>(items: List<T>, index: Int) -> Option<T>` | a copy, `None` out of range |
| `sort<T: Comparable>(mut items: List<T>)` | **stable** on every target |
| `sort_by<T>(mut items: List<T>, compare: fn(T, T) -> Ordering)` | stable |
| `min<T: Comparable + Copyable>(items: List<T>) -> Option<T>`, `max` | |
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
`env(name: String) -> Option<String>`.

## 8. `extern`

### 8.1 Declarations

```
extern_block = ( "ts" | "go" | "rust" ) "{" extern_decl+ "}"
extern_decl  = visibility "fn" IDENT "(" params? ")" ( "->" type )? "from" STRING
             | visibility "type" IDENT "from" STRING
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
   code uses it only through extern functions (E0707).
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
