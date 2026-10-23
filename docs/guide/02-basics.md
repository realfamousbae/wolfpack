# 2. Basics

## The shape of a file

A file starts with its `module` line, and everything after it is in sections. The sections always
come in this order, and each one is optional:

| Section | Holds |
| --- | --- |
| `imports` | the modules and names this module uses |
| `extern` | functions and types of the target languages (chapter 12) |
| `config` | constants |
| `types` | `struct`, `enum` and `type` declarations |
| `interfaces` | `interface` declarations |
| `impl` | methods, and implementations of interfaces |
| `functions` | functions |
| `tests` | tests of the module |

The fixed order means that every file looks alike: the types of a module are always in the same
place, and the reader knows where to look. A section appears at most once, and an empty section is
not written. The order is only about layout, not about visibility: every name of a module can be
used anywhere in the module, before or after its declaration.

```wolf
//! Shows the sections of a file. A `//!` comment before the `module` line documents the module.
module main

imports {
    std.math
}

config {
    greeting: String = "Hello"
}

types {
    /// A point on a grid.
    struct Point {
        x: Int
        y: Int
    }
}

main {
    // An ordinary comment.
    let p = Point { x: 3, y: 4 }
    print(greeting + ", " + (p.x + p.y).to_string())
    print(math.pi.to_string())
}
```

```output
Hello, 7
3.141592653589793
```

Comments are written as in Rust: `//` to the end of the line, `/* ... */` around a block (block
comments nest), `///` before a declaration to document it, and `//!` at the top of the file to
document the module. Doc comments are carried into the generated code.

## Statements and line breaks

Wolfpack has no `;`. A line break ends a statement, so each statement stands on its own line. A
line break does not end anything inside `( )` and `[ ]`, after a token that cannot end an
expression (a binary operator, `=`, `,`, `.`, `->`, `=>`), and before a line that starts with `.`.
So a long statement can continue after `=` or after an operator, and a chain of method calls can
put each call on its own line. `wolf fmt` breaks long lines exactly this way:

```wolf
module main

main {
    let base_price_in_cents = 12000
    let delivery_fee_in_cents = 1500
    let total_price_in_cents =
        base_price_in_cents + delivery_fee_in_cents + base_price_in_cents / 10
    let words = "  the quick brown fox jumps over the lazy dog and runs far away from the old farm  "
        .trim()
        .split(" ")
        .filter(|w| w.len() > 3)
    print(total_price_in_cents.to_string() + " " + words.len().to_string())
}
```

```output
14700 9
```

Writing the operator at the start of the next line is an error, because the statement ended with
the line before it (`wolf explain E0303`). For the same reason `else` stands on the line of the
closing `}`: `} else {`.

## Variables

`let` declares a variable that never changes, `var` one that does:

```wolf
module main

main {
    let name = "wolfpack"
    var count = 1
    count += 1
    count = count * 10
    let label: String = name + " " + count.to_string()
    print(label)
}
```

```output
wolfpack 20
```

The type of a variable is inferred from its value; it can also be written after the name, as for
`label`. Types of parameters, results, fields and constants are always written: inference never
crosses the signature of a function.

Two rules keep code tidy, and the compiler enforces both:

- A variable that is never used is an error. Write `let _ = value` to compute something and drop
  the result on purpose.
- A name cannot be declared twice in the same block. A nested block may reuse a name of an outer
  one.

## Constants

The `config` section holds constants. Their type is always written, and is `Int`, `Float`, `Bool`
or `String`. The value is computed by the compiler, so it may use literals, other constants and
operators, but no function calls:

```wolf
module main

config {
    max_users: Int = 100
    max_bytes: Int = max_users * 1024
    name: String = "store"
}

main {
    print(name + ": " + max_bytes.to_string())
}
```

```output
store: 102400
```

A constant can be overridden at build time from `wolfpack.toml` (chapter 10).

## Numbers

`Int` is a signed 64-bit integer on every target, and `Float` a 64-bit IEEE floating-point number.

| Literal | Value |
| --- | --- |
| `42`, `1_000_000` | decimal integers; `_` separates digits |
| `0xff`, `0o17`, `0b1010` | hexadecimal, octal and binary integers |
| `1.5`, `2.0e-3`, `1e9` | floats; digits are required on both sides of `.` |

There are no implicit conversions, not even between `Int` and `Float`. `1 + 1.5` is an error;
convert one side with `to_float()` or `to_int()`, which truncates toward zero:

```wolf
module main

main {
    let items = 7
    let price = 2.5
    let total = items.to_float() * price
    print(total.to_string())
    print(total.to_int().to_string())
    print((items / 2).to_string() + " " + (items % 2).to_string())
    print((-7 / 2).to_string() + " " + (-7 % 2).to_string())
}
```

```output
17.5
17
3 1
-3 -1
```

Integer division truncates toward zero, and `%` has the sign of the left side. Arithmetic on `Int`
**never wraps around**: an overflow stops the program with a panic on every target, and so does a
division by zero. `Float` follows IEEE rules: dividing by zero gives an infinity, not a panic.

`Float.to_string()` writes the shortest decimal that reads back as the same number, with `.0` for
whole numbers (`17.0`), and switches to scientific notation for very large and very small values
(`1e21`). The output is the same on every target.

## Booleans and comparisons

`Bool` has the values `true` and `false`, combined with `&&`, `||` and `!`. `==` and `!=` compare
values of the same type, including whole structs and lists, field by field. `<`, `<=`, `>` and `>=`
work on `Int`, `Float` and `String`; other types are compared with a `compare` method (chapter 9).
Comparisons cannot be chained: write `0 < x && x < 10`, not `0 < x < 10`.

The bitwise operators `&`, `|`, `^`, `~`, `<<` and `>>` work on `Int` only.

## Strings

A `String` is an immutable sequence of Unicode characters. `+` joins two strings; anything else is
converted with `to_string()` first. Lengths and positions count characters (Unicode scalar values),
not bytes, on every target:

```wolf
module main

main {
    let word = "naïve"
    print(word.len().to_string())
    print(word[0..2])
    print(word.to_upper_ascii())
    print("a,b,c".split(",").len().to_string())
    print("tab\tquote\" backslash\\")
}
```

```output
5
na
NAïVE
3
tab	quote" backslash\
```

- `s[a..b]` is a new string with the characters from `a` up to, not including, `b`; `s[a..=b]`
  includes `b`. There is no `s[i]`, since there is no character type: write `s[i..=i]`.
- Other methods include `contains`, `starts_with`, `find`, `replace`, `trim`, `lines`, `chars` and
  `repeat`. `to_upper_ascii` changes only ASCII letters, because the targets disagree on full
  Unicode case mapping.
- Strings use double quotes. The escapes are `\n`, `\r`, `\t`, `\0`, `\\`, `\"`, `\xHH` (ASCII) and
  `\u{...}`. A `"..."` string ends on its line; strings over several lines are written with `"""`
  (below).
- `"text".parse_int()` and `parse_float()` read numbers; they return a `Result` (chapter 7).

### Interpolated, raw and multiline strings

An **interpolated string** `f"..."` puts the values of expressions into the text. Anything that
implements `Display` can stand in the braces: numbers, `Bool`, `String`, lists, maps, options and
results of them (chapter 6), and your own types (chapter 9). `{{` and `}}` write a brace:

```wolf
module main

main {
    let name = "Ann"
    let items = ["tea", "jam"]
    print(f"{name} has {items.len()} items, the first is {items[0]}")
    print(f"half of 3 is {3.0 / 2.0}, braces: {{}}, all: {items}")
}
```

```output
Ann has 2 items, the first is tea
half of 3 is 1.5, braces: {}, all: ["tea", "jam"]
```

An interpolation may end with a **format specification** after a `:`. It sets a width, an
alignment (`<`, `^`, `>`) with an optional fill character before it, `0` to pad a number with
zeros, and the number of digits after the point of a `Float`:

```wolf
module main

main {
    let names = ["tea", "honey"]
    let prices = [3.5, 12.25]
    for i in 0..names.len() {
        print(f"|{names[i]:<8}|{prices[i]:>8.2}|")
    }
    print(f"{7:03} {"mid":*^9} {2.5:.0} {3.5:.0}")
}
```

```output
|tea     |    3.50|
|honey   |   12.25|
007 ***mid*** 2 4
```

- Numbers align to the right and other values to the left, unless an alignment is written.
- The width counts characters (Unicode scalar values) and never cuts a longer text.
- A precision rounds to the nearest, and a value exactly halfway to the even digit, as `{2.5:.0}`
  and `{3.5:.0}` show; it is only for `Float`, and `0` only for numbers (E0441).
- The specification starts at the first `:` outside brackets and strings, so `{m["a"]:>3}` works.

A **raw string** `r"..."` has no escapes: `r"\d+"` is the four characters `\d+`. A **multiline
string** starts with `"""` at the end of a line and ends with `"""` on a line of its own; the
indentation of the closing quotes is removed from every line. `f"""` and `r"""` combine both:

```wolf
module main

main {
    let name = "tour"
    print(r"C:\files\new")
    print(f"""
        usage: {name} <command>

        commands:
            run    show everything
        """)
}
```

```output
C:\files\new
usage: tour <command>

commands:
    run    show everything
```

## Void

A function without a result type returns `Void`, whose only value is written `()`. It appears
mostly in types such as `Result<Void, Error>`, a result that is either nothing or an error.

The next chapter covers blocks, conditions and loops.
