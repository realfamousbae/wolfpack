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
| `Int8`, `Int16`, `Int32` | signed integers of 8, 16 and 32 bits | **[DRAFT]** (2026-10-26, D4.24), section 3.1a |
| `UInt8`, `UInt16`, `UInt32` | unsigned integers of 8, 16 and 32 bits | **[DRAFT]** (2026-10-26, D4.24), section 3.1a |
| `Float` | IEEE 754 binary64 | **[PLAN]** |
| `Bool` | `true`, `false` | |
| `String` | immutable sequences of Unicode scalar values, stored as UTF-8 | **[PLAN]**, G1 |
| `Char` | one Unicode scalar value | **[DRAFT]** (2026-10-26, D4.23), section 3.7 |
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
   **[DRAFT]** (2026-10-26): a literal with a suffix has the type of its suffix (D1.14), and one
   without may take a sized type from its place (section 3.1a, rule 14; D4.25, open).

### 3.1a Sized integer types **[DRAFT]** (2026-10-26, D4.24, D4.25)

Proposed, waiting for the author.

1. Besides `Int` there are six integer types of a fixed width:

   | Type | Values | Suffix (chapter 1, D1.14) |
   | --- | --- | --- |
   | `Int8` | -128 to 127 | `i8` |
   | `Int16` | -32,768 to 32,767 | `i16` |
   | `Int32` | -2,147,483,648 to 2,147,483,647 | `i32` |
   | `UInt8` | 0 to 255 | `u8` |
   | `UInt16` | 0 to 65,535 | `u16` |
   | `UInt32` | 0 to 4,294,967,295 | `u32` |

   `Int` stays **the** integer type: literals without a suffix, lengths, indices, positions,
   ranges, `config` constants and every std function that is not about a width use it. A sized
   type is for the places where a width is part of the data: bytes (`UInt8`, the element type of
   a future `Bytes`, `docs/ideas.md`), the fields of a binary format, colours, values that extern
   code has in that width. Every value of the six types is a value of `Int`, so widening to `Int`
   never fails. There is no `Int64` (that is `Int`), no `UInt64`, no type of the platform's width
   (`isize`) and no `Float32`.
2. They are declared in `std.math`, next to `Int` and `Float`, and are part of the prelude
   (chapter 6, section 3.3; chapter 7, section 4.1a).
3. They are **copy types** (chapter 5, section 2, rule 1).
4. **Arithmetic** is that of `Int` (section 3.1) with the range of the type in place of 64 bits:
   `+`, `-`, `*` and unary `-` panic on overflow (`panic: integer overflow`) and never wrap, so
   `200u8 + 100u8` panics; `/` truncates toward zero and `%` has the sign of the dividend;
   division and remainder by zero panic; on a signed type `MIN / -1` and `MIN % -1` panic.
5. **Unsigned types have no unary minus** (E0405): `-x` with `x: UInt8` and `-1u8` are errors,
   and `0u8 - 1u8` panics.
6. **Bitwise** `&`, `|`, `^`, `~` work on the bits of the width: `~0u8 == 255u8`,
   `~0i8 == -1i8`, `0xF0u8 | 0x0Fu8 == 255u8`.
7. **Shifts.** The amount is always an `Int`, whatever the type on the left (`b << 3`, `b >> n`
   with `n: Int`), and must be in `0` to the width minus one (`0..=7` for `UInt8`), else the shift
   panics (`panic: shift amount out of range`). `<<` discards the bits shifted out of the width
   and never panics for overflow (`1i8 << 7 == -128i8`, `0x81u8 << 1 == 2u8`), as on `Int`. `>>`
   copies the sign bit on a signed type and shifts in zeros on an unsigned one.
8. **No mixing.** Both operands of an arithmetic, bitwise or comparison operator have one type:
   `a + b` with `a: UInt8` and `b: Int` is E0405, with a help that names the conversion
   (`a.to_int() + b`). Widening is explicit too (section 1, rule 2): `b.to_int()`, and back
   `n.to_uint8()` (chapter 7, section 4.1a).
9. **Comparisons** `==`, `!=`, `<`, `<=`, `>`, `>=` compare values, as on `Int`: the six types
   join the list of section 3.5, rule 2.
10. **Indices, slices and ranges stay `Int`**: `l[b]` with `b: UInt8` is E0401 (section 4,
    rule 1), and `for i in 0..n` iterates `Int`s; write `l[b.to_int()]`.
11. **Built-in interfaces** (section 9): `Equatable` and `Copyable` (derived), `Hashable`, so they
    can be `Map` keys (the closed list of G3 gains the six types), `Comparable` by value, and
    `Display`: the decimal text, as for `Int` (`255`, `-128`, no suffix).
12. **Patterns.** Integer literals are literal patterns of these types (section 11, rule 2). A
    `match` on a sized value needs a `_` or a binding arm, also when its arms happen to list all
    256 values of a `UInt8`: there is no range analysis (section 11, rule 5).
13. **`config` constants** stay `Int`, `Float`, `Bool` or `String` (chapter 2, D2.1): a constant
    with a width is an `Int`, converted where it is used (`limit.to_uint16()`).
14. **Literals of a sized type** (D4.25, an open question; recommendation (b)):
    - (a) **Only with a suffix.** Rule 6 of section 3.1 stays word for word: a literal without a
      suffix is an `Int`, and a sized value is written with its suffix: `let b: UInt8 = 255u8`,
      `b + 1u8`, `match b { 0u8 => ... }`.
    - (b) **The expected type.** A literal without a suffix is an `Int`, except where a sized type
      is **expected**: there it has that type, and its value must fit (E0423:
      `let b: UInt8 = 256`). A sized type is expected
      - in the positions where a value is coerced to an interface type (section 8.5, rule 2): the
        initializer of a `let` or `var` with a written type, an argument, a field of a struct
        literal or a payload of a variant, an element of a list or map literal whose element
        type is known, the result of a function (`return`, the final expression of its body);
      - on the right of an assignment or a compound assignment to a place of a sized type
        (`b = 0`, `b += 1`);
      - as an operand of an arithmetic, bitwise or comparison operator whose other operand has
        a sized type (`b + 1`, `1 + b`, `b == 0`, `mask & 0xFF`), but not as the amount of a
        shift, which is an `Int`;
      - as a literal pattern on a scrutinee of a sized type (`match b { 0 => ..., _ => ... }`);
      - and from these positions on: in the branches of an `if` or a `match`, in the final
        expression of a block, in parentheses, in the operand of a unary minus, and in both
        operands of an arithmetic or bitwise operator (not in a shift amount), so
        `let x: UInt8 = 1 + 2` and `b + 1 * 2` are `UInt8` arithmetic.

      A unary minus and its literal count as one literal (`let x: Int8 = -128`). Where no sized
      type is expected, the literal is an `Int`, also when inference later finds a sized type:
      `let n = 5` followed by `takes_u8(n)` is E0401. A literal never becomes a `Float`.

      **Generic parameters.** A parameter whose type is a type parameter (`T` of
      `assert_eq<T>(left: T, right: T)`) expects a sized type only once `T` is known at that
      argument: from an earlier argument, read from left to right, or from the expected result.
      So `assert_eq(b, 5)` with `b: UInt8` compares two `UInt8`s, while `assert_eq(5, b)` makes
      `T` an `Int` at the `5` and is E0401 at `b`; write `assert_eq(5u8, b)`. The same holds for
      list and map literals: `[b, 1]` is a `List<UInt8>`, `[1, b]` is E0401.

    (b) changes the letter of rule 6 of section 3.1 ("no untyped or polymorphic literals") but not
    its point: a literal gets its type from the place where it is written, never from inference
    later in the body, and nothing is converted at run time. Rust, Swift, Kotlin and Go let
    `let b: u8 = 255` be written without a suffix in the same way.
15. **No new code.** A mismatch is E0401, an operator on mixed types or a unary minus on an
    unsigned type E0405, a literal that does not fit its type E0423 (E0106 in the lexer for a
    suffixed one, chapter 1).

```
fn pack(r: UInt8, g: UInt8, b: UInt8) -> UInt32 {
    ((r.to_int() << 16) | (g.to_int() << 8) | b.to_int()).to_uint32()
}

fn red(color: UInt32) -> UInt8 {
    ((color >> 16) & 0xFF).to_int().to_uint8()     // `0xFFu32` with (a)
}

fn checksum(data: List<UInt8>) -> UInt8 {
    var sum: UInt8 = 0                              // `0u8` with (a)
    for b in data {
        sum = sum ^ b
    }
    sum
}
```

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
   **[DRAFT]** (2026-10-26, D4.23): with `Char` (section 3.7) this stays: `s[i]` does not exist,
   and `s.char_at(i)` gives the `Char` at a position (chapter 7, section 4.3).

### 3.5 Comparison operators in general

1. `==` and `!=` work on any type that is `Equatable` (section 9), both sides of the same type.
2. `<`, `<=`, `>`, `>=` work only on `Int`, `Float` and `String` (author). Generic code and user
   types use `a.compare(b)`, which returns `Ordering`. **[DRAFT]** (2026-10-26, D4.23): and on
   `Char` (section 3.7). **[DRAFT]** (2026-10-26, D4.24): and on the sized integer types
   (section 3.1a).

### 3.6 Compound assignment

`x op= y` has exactly the typing and the panics of `x = x op y`; `x` is evaluated once.

### 3.7 `Char` **[DRAFT]** (2026-10-26, D4.23)

Proposed, waiting for the author.

1. `Char` is one **Unicode scalar value**: a code in U+0000 to U+D7FF or U+E000 to U+10FFFF,
   never a surrogate. It is the unit that `String` is made of (G1): `s.chars()` gives the `Char`s
   of `s`, and `s.len()` counts them. It is not what a reader sees as one character (a grapheme
   cluster): `é` written as `e` followed by U+0301 is two `Char`s.
2. Its literals are `'a'` (chapter 1, section 5.3b). `Char` is declared in `std.string` and is
   part of the prelude (chapter 6, section 3.3; chapter 7, section 4.3a).
3. `Char` is a **copy type**, like `Int` (chapter 5, section 2, rule 1): `let d = c` copies.
4. `==` and `!=` compare scalar values. `<`, `<=`, `>`, `>=` compare scalar values as well, the
   order in which `String` compares (section 3.4, rule 2): `'Z' < 'a'`, `'z' < 'é'`.
5. There are no arithmetic or bitwise operators on `Char`: `'a' + 1`, `-c` and `c & 0x20` are
   E0405. Arithmetic on codes goes through `c.to_int()` and `Char.from_int(n)` (chapter 7,
   section 4.3a).
6. `+` does not join a `Char` to a `String` (section 3.4, rule 1 stays as it is): `"a" + c` and
   `c + c` are E0405; write `"a" + c.to_string()` or `f"a{c}"`.
7. A `Char` is never a `String` or an `Int`, nor the reverse: `let s: String = 'a'` and
   `let n: Int = 'a'` are E0401 (section 1, rule 2).
8. Built-in interfaces (section 9): `Char` is `Equatable` and `Copyable` (derived), `Hashable`, so
   it can be a `Map` key (the closed list of G3 gains one type), `Comparable` by scalar value, and
   `Display`: its text is the one-character string.
9. **Patterns.** A character literal is a literal pattern (section 11, rule 2):
   `match c { 'a' | 'e' => 1, _ => 0 }`. As for `Int`, a `match` on a `Char` needs a `_` or a
   binding arm to be exhaustive (section 11, rule 5). There are no range patterns (`'a'..='z'`);
   a guard serves: `x if x >= 'a' && x <= 'z' => ...`.
10. `Char` is **not** a type of `config` constants (chapter 2, D2.1 stays): TOML has no character
    value, and a `String` constant serves.
11. **No `s[i]`** (section 3.4, rule 4; D4.10 stays). `s.char_at(i)` gives the `Char` at a
    position and panics out of range like `l[i]` (chapter 7, section 4.3). On every target a
    position counted in scalar values is found by walking the string, so `s[i]` would look cheap
    and is not; a slice `s[i..=i]` stays a `String`.
12. No new code: a mismatch is E0401, an operator that is not defined is E0405, as for the other
    types.

```
fn is_vowel(c: Char) -> Bool {
    match c {
        'a' | 'e' | 'i' | 'o' | 'u' => true
        _ => false
    }
}

fn count_vowels(s: String) -> Int {
    var n = 0
    for c in s.chars() {
        if is_vowel(c) {
            n += 1
        }
    }
    n
}
```

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

**Update syntax for variants** (D4.22) **[DRAFT]**. The update syntax `..base` (chapter 3,
section 4, rule 6) works for an enum variant with named fields too:
`Shape.Rect { width: 10, ..old }`. The base is an expression of the enum type with the same type
arguments (E0401). At run time the base must be the same variant; if it is another one, the
program panics with `panic: update of Shape.Rect from Shape.Circle` (a panic of the language,
without a location; chapter 7, section 3). The listed fields are evaluated first, then the base, as
for structs. E0434 is retired. The reason for a panic rather than a compile-time check: a value of
type `Shape` does not know its variant before it runs, and the language has no type for one
variant.

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
6. (author, 2026-10-25, D4.20) **Super-interfaces.** An interface may name other interfaces
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
7. **Super-interfaces** (D4.20). A block `Type: Greeter { ... }` needs `Type` to
   implement every super-interface of `Greeter` (section 8.2, rule 6), with the same type
   arguments: `Type: Named { ... }` in the same module (rule 2), or a built-in implementation
   (`Display` of `Int`). For a generic type, the bounds of the head of the `Greeter` block must
   give what the head of each super-interface's block needs: with `Bag<T: Named>: Named`,
   `Bag<T>: Greeter` is E0446 and `Bag<T: Named>: Greeter` is fine. A missing implementation is
   E0446 at the block of `Greeter`, naming the super-interface.

8. **Blocks for one instantiation** (D4.21) **[DRAFT]**. The head of an inherent block may give
   a type argument instead of declaring a parameter: `Stack<Int> { fn sum(self) -> Int { ... } }`.
   An argument of the head that names a type in scope (`Int`, `User`, an imported type), or that
   is a type with arguments of its own or a function type (`List<String>`, `fn(Int) -> Int`), is a
   type argument; a single name that names no type declares a parameter, as before. Arguments and
   parameters may be mixed: `Pair<String, T> { ... }`.
   - The methods of such a block exist only for that instantiation: `s.sum()` on a `Stack<Int>`,
     and E0424 (unknown method) on a `Stack<String>`, with a note naming the block.
   - Rule 4 holds across all the blocks of the type: a method name appears once among the generic
     block and every block of an instantiation (E0415). So `Stack<Int>` and `Stack<String>` cannot
     both have `describe`.
   - Only inherent blocks: an interface implementation for one instantiation
     (`Stack<Int>: Display`) is **E0448**, because Go cannot give one instantiation of a generic
     type a method that the others lack, so the type could not satisfy a Go interface.
   - Such a block is the same as a generic block whose methods all have `where T == Int`, which the
     language does not have; the backends treat its methods like the methods of section 8.6
     **[INTERNAL]**: Rust `impl Stack<i64> { ... }`; Go package functions
     (`StackIntSum(s Stack[int64]) int64`), as for methods with `where` bounds; TypeScript methods on
     the class, typed with `this: Stack<bigint>`.

### 8.4 Method calls

`x.m(args)` looks up `m` among the methods of the type of `x` (inherent or from an implemented
interface). By rule 8.3.4 there is at most one. For a type parameter `T`, the methods come from its
bounds. Associated functions are called as `Type.f(args)`.

### 8.5 Interfaces as value types

1. An interface `I` can be used as a type (`List<Shape>`, a field of type `Shape`) **only if it is
   object-safe**: no method mentions `Self` except as the receiver, and (by 8.2) no method is
   generic and every member takes `self`. Using a non-object-safe interface as a type is an error
   that names the offending method. Such interfaces (for example `Comparable`) are still usable as
   bounds. **[PROPOSAL]** An interface with super-interfaces (D4.20, author, 2026-10-25) is
   object-safe only if each of them is, so `interface Ordered: Comparable` is a bound only. A
   value of type `Greeter` has the methods of `Greeter` and of its super-interfaces
   (`g.name()`), but it is not converted to a value of a super-interface: `let n: Named = g`
   is E0401, as there is no coercion between interface types. The reason: a value of a Rust
   trait object becomes one of its super-trait only from Rust 1.86, and the generated Rust
   builds with 1.85 (chapter 8).
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
8. **[DRAFT]** (2026-10-26, D4.23) **`Char`** (section 3.7) is `Equatable` and `Copyable`,
   `Hashable` (added to the closed list), `Comparable` (built in, by scalar value) and `Display`
   (built in): each row of the table above gains `Char`. A list of `Char`s is `Display` and writes
   its parts in single quotes, `['a', 'b']` (chapter 7, section 4.1).
9. **[DRAFT]** (2026-10-26, D4.24) **The sized integer types** (section 3.1a) are `Equatable` and
   `Copyable`, `Hashable` (added to the closed list), `Comparable` (built in, by value) and
   `Display` (built in, decimal): each row of the table above gains them.

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
   **[DRAFT]** (2026-10-26, D4.23): and for `Char`. **[DRAFT]** (2026-10-26, D4.24): and for
   the sized integer types (section 3.1a, rule 12).
3. Both sides of `A | B` must bind the same names with the same types.
4. A guard (`if cond`) must be `Bool`. **A guarded arm never counts toward exhaustiveness.**
5. **Exhaustiveness** **[PLAN]**: the arms must cover every value of the scrutinee type. For
   `Int` and `String` this needs a `_` or a binding arm. For enums, every variant (and every
   combination for nested patterns) must be covered. The error lists an example of a missing
   pattern: "missing `Shape.Rect { .. }`". **[DRAFT]** (2026-10-26, D4.23): a `Char` needs a `_`
   or a binding arm too, and so does a sized integer (D4.24).
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
| E0434 | Update syntax `..base` in a literal of an enum variant (added in 0.2; retired by D4.22 if it is approved) |
| E0435 | `break` with a value in a `while` or a `for` (added in 0.2) |
| E0436 | Unknown loop label (added in 0.2) |
| E0437 | A name after `break` that is both an enclosing label and a variable (added in 0.2) |
| E0438 | A label that repeats the label of an enclosing loop (added in 0.2) |
| E0439 | A struct literal or a struct pattern whose name is not a struct, an alias of one or a variant with named fields (section 7.1, rule 3; added after 0.3) |
| E0441 | A format specification that the type of the interpolation cannot take: a precision for a type other than `Float`, `0` for a type other than `Int` and `Float` (chapter 7, section 4.4; D4.18) |
| E0442 | A `where` that is not allowed there: on a free function, in an interface or an interface implementation, or naming a type parameter that is not one of the block (section 8.6, rule 3) |
| E0443 | A default method replaced without `override`, or `override` on a method that replaces no default (section 8.3, rule 1; D4.19) |
| E0444 | `Interface.method(x)` for a method without a default (section 8.2, rule 5; D4.19) |
| E0445 | A super-interface that is not allowed: not an interface, `Equatable`, `Copyable` or `Hashable`, the interface itself directly or through others, or one named twice (section 8.2, rule 6; D4.20) |
| E0446 | An interface implementation for a type that does not implement a super-interface of the interface (section 8.3, rule 7; D4.20) |
| E0447 | A method of an interface with the name of a method of one of its super-interfaces (section 8.2, rule 6; D4.20) |
| E0448 | An interface implementation block for one instantiation of a generic type, `Stack<Int>: Display` (section 8.3, rule 8) **[DRAFT]** |

Codes E0426 to E0430 were added in M4 (author, 2026-10-09).

**[DRAFT]** (2026-10-26, D4.24, D4.25): the sized integer types need no new code. E0405 also covers
an operator on two different integer types and a unary minus on an unsigned type, E0423 a literal
that does not fit its sized type, and E0441 a precision as for `Int` (a `0` is allowed for every
integer type).

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
| D4.20 | Super-interfaces (section 8.2, rule 6; section 8.3, rule 7; section 8.5, rule 1; chapter 8, section 5.3): `interface Greeter: Named + Display`; a type implements each super-interface in its own block (E0446 otherwise); a bound by `Greeter` gives the methods of the super-interfaces, also to its defaults; super-interfaces are interfaces of the program, `Error`, `Comparable` and `Display`, not `Equatable`, `Copyable` or `Hashable`; cycles and repeats are E0445, a method name taken from a super-interface E0447; no coercion of a `Greeter` value to a `Named` value (Rust 1.85). Reverses "no super-interfaces" of D3.3. Targets: Rust super-traits, TS `extends`, Go embedded interfaces; the dictionaries of TS and Go bounds carry those of the super-interfaces | **answered** (author, 2026-10-25): yes |
| D4.21 | **[DRAFT]** Inherent blocks for one instantiation, `Stack<Int> { ... }` (section 8.3, rule 8): a head argument that names a type is a type argument, a name that names no type is a parameter; method names stay unique across all blocks of the type (E0415); interface implementations for one instantiation are E0448. Alternatives: an explicit marker for parameters (`Stack<Int>` vs `<T> Stack<T>`, as in Rust), at the cost of changing every existing head; or `where T == Int` on single methods | **proposed** (2026-10-26), waiting for the author |
| D4.22 | **[DRAFT]** `..base` for enum variants with named fields (section 7.2): the base has the enum type; a base of another variant panics at run time (`panic: update of Shape.Rect from Shape.Circle`); E0434 is retired. Alternative: keep E0434 (no compile-time check is possible without a type for one variant), which is the drafter's second choice | **proposed** (2026-10-26), waiting for the author |
| D4.15 | E0439 for a struct literal or pattern of a name that is not a struct (section 7.1, rule 3), and literals and patterns through an alias of a struct (section 7.3). Before, both stopped `wolf` with an internal error | **proposed** (2026-10-23), waiting for the author |
| D4.23 | The type `Char` (section 3.7): one Unicode scalar value (not a grapheme cluster); a copy type (chapter 5, section 2); `==` and `<` by scalar value, which adds `Char` to the author's list of section 3.5, rule 2; no arithmetic or bitwise operators, and `"a" + c` stays E0405 (section 3.4, rule 1 unchanged); `Equatable`, `Copyable`, `Hashable` (a `Map` key, one more type in the closed list of G3), `Comparable`, `Display`; literal patterns, exhaustive only with `_`; not a `config` type; declared in `std.string` and part of the prelude, so a 0.5 program with a top-level type `Char` must rename it (E0617, chapter 6, section 3.3). `s[i]` stays absent (D4.10); `s.char_at(i)` is proposed instead (chapter 7, D7.22). No new E04xx code. Alternatives: `s[i] -> Char`, reversing D4.10 (it would look O(1) but walks the string on every target); `+` of a `String` and a `Char`; `Char` as a `config` type, from a one-character TOML string; a grapheme cluster, as Swift's `Character` (the targets disagree on clusters as they do on case mapping, D7.3) | **proposed** (2026-10-26), waiting for the author |
| D4.11 | Rest: `Ordering`, Never type, string ordering by scalar value, `?` with identical `E`, recursive types rule, object safety rule, no two methods with one name, built-in interfaces table, inference rules, diagnostics | **answered**: yes |
| D4.24 | Sized integer types (section 3.1a): `Int8`, `Int16`, `Int32`, `UInt8`, `UInt16`, `UInt32`, named in the style of `Int` (as Swift names them); `Int` stays the default and the only 64-bit type; copy types; the arithmetic of `Int` with the range of the type, panicking on overflow and never wrapping; no unary minus on the unsigned types; bitwise operators on the bits of the width; the shift amount is always an `Int` in `0` to the width minus one, `>>` arithmetic on signed and logical on unsigned types; no mixing in an operator (E0405) and no implicit widening; indices, slices and ranges stay `Int`; `Equatable`, `Copyable`, `Hashable` (`Map` keys), `Comparable`, `Display` (decimal); literal patterns, a `match` needing `_`; not `config` types; prelude names, so a program with a top-level type of one of these names must rename it (E0617). No new E04xx code. **Open**: (1) the set: recommended these six, which all fit in `Int`, so `to_int()` never fails; alternatives a smaller set (`UInt8` for bytes, `Int32` and `UInt32` for interop) or a larger one with `UInt64` (64-bit hashes; then `to_int()` can fail) and `Int64` as a second name of `Int`; (2) the names: alternatives `I8`/`U8` (they match the suffixes, not `Int` and `Float`), or `Byte`/`Short` (signed in Java and Kotlin, unsigned in Go and C#); (3) `Byte`: recommended no second name, and the `Bytes` draft uses `UInt8` for its elements; the alternative `type Byte = UInt8` in the prelude is best decided with `Bytes`; (4) wrapping arithmetic (`wrapping_add`, `wrapping_mul`) is not proposed: 32-bit hashes such as FNV-1a work in `Int` and are masked (`((h ^ b) * 16777619) & 0xFFFF_FFFF`), 64-bit ones would need it | **proposed** (2026-10-26), waiting for the author |
| D4.25 | Literals of the sized types (section 3.1a, rule 14). (a) only with a suffix, as section 3.1, rule 6 says today: `let b: UInt8 = 255u8`, `b + 1u8`, `match b { 0u8 => ... }`; (b) **recommended**: a literal without a suffix takes the sized type expected at its place (the positions of interface coercion, section 8.5, rule 2; the right side of an assignment; the other operand of an operator, not a shift amount; a literal pattern; the branches of an `if` or a `match`; parentheses and both operands of an arithmetic or bitwise operator, so `let x: UInt8 = 1 + 2` works) and is an `Int` everywhere else, E0423 when it does not fit; inference never changes it later (`let n = 5` stays `Int`); for a parameter of a type parameter the type must be known from an earlier argument, so `assert_eq(b, 5)` works and `assert_eq(5, b)` is E0401 (write `5u8`). (b) changes the letter of section 3.1, rule 6, not its point: no literal becomes a `Float`, nothing is converted at run time. Alternative (c): Rust's inference of integer literals with `Int` as the default (`let n = 5` becomes a `UInt8` when it is passed to a `UInt8` parameter), more convenient but a literal's type then depends on code far from it | **proposed** (2026-10-26), waiting for the author |
