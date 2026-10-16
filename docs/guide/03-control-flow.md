# 3. Control flow

## Blocks have values

A block `{ ... }` runs its statements in order. If it ends with an expression, that expression is
the value of the block. This is how a function returns its result without `return`, and why `if`
and `match` can be used as values:

```wolf
module main

functions {
    fn sign(n: Int) -> String {
        if n < 0 {
            "negative"
        } else if n == 0 {
            "zero"
        } else {
            "positive"
        }
    }

    fn main() {
        let limit = 10
        let size = if limit > 5 { "big" } else { "small" }
        print(size)
        print(sign(-3))
        print(sign(0))
    }
}
```

```output
big
negative
zero
```

The condition of an `if` is a `Bool`; there is no "truthiness" of numbers or strings. When an `if`
is used as a value, both branches have the same type. An `if` without `else` has no value: its
type is `Void`.

`return` leaves a function early, with a value if the function has a result. An expression whose
value is never used, such as a lone `x + 1` on its own line, is an error: it is almost always a
mistake, often a statement broken by a line break.

## Loops

`while` repeats while its condition is true. `for` walks a list, or a range of integers:
`0..n` counts from 0 up to `n - 1`, `1..=n` from 1 up to `n`.

```wolf
module main

functions {
    fn main() {
        var n = 1
        while n < 100 {
            n = n * 3
        }
        print(n.to_string())

        for i in 1..=3 {
            print("step " + i.to_string())
        }

        let names = ["Ann", "Bob", "Cid"]
        for name in names {
            if name == "Bob" {
                continue
            }
            print(name)
        }
    }
}
```

```output
243
step 1
step 2
step 3
Ann
Cid
```

`break` leaves the innermost loop and `continue` starts its next round; neither has a label or a
value. There is no `loop` keyword: write `while true`. Such a loop is known to never end unless a
`break` leaves it, so a function may end with it without a value after it:

```wolf
module main

functions {
    fn first_square_above(limit: Int) -> Int {
        var i = 1
        while true {
            if i * i > limit {
                return i * i
            }
            i += 1
        }
    }

    fn main() {
        print(first_square_above(50).to_string())
    }
}
```

```output
64
```

Ranges are not values: they are written only in a `for` and inside the brackets of a slice
(`text[1..3]`, chapter 2). To get a list of numbers, use `list.range(from, to)` (chapter 6).

## A first look at `match`

`match` compares a value with a list of **patterns**, top to bottom, and runs the arm of the first
one that fits:

```wolf
module main

functions {
    fn describe(n: Int) -> String {
        match n {
            0 => "zero"
            1 | 2 | 3 => "a few"
            x if x < 0 => "negative"
            _ => "many"
        }
    }

    fn greet(lang: String) -> String {
        match lang {
            "en" => "hello"
            "de" => "hallo"
            other => "no greeting for " + other
        }
    }

    fn main() {
        print(describe(2))
        print(describe(-5))
        print(describe(40))
        print(greet("de"))
        print(greet("fr"))
    }
}
```

```output
a few
negative
many
hallo
no greeting for fr
```

- A literal pattern (`0`, `"en"`, `true`) matches that value. Float literals are not allowed, since
  comparing floats exactly is rarely what is meant.
- `A | B` matches either alternative.
- A name (`x`, `other`) matches anything and gives the value that name inside the arm; `_` matches
  anything without a name.
- `if` after a pattern is a **guard**: the arm is taken only when the condition is true.
- An arm is an expression or a block, and all arms have the same type.

A `match` must cover every possible value, and the compiler checks it. For `Int` or `String` that
needs a final `_` or name arm. A guarded arm does not count, because the compiler cannot know when
the condition is true. An arm that can never be reached, because an earlier arm already covers it,
is an error too:

```wolf error E0410
module main

functions {
    fn describe(n: Int) -> String {
        match n {
            0 => "zero"
            x if x > 0 => "positive"
        }
    }

    fn main() {
        print(describe(1))
    }
}
```

`match` shows its real strength with enums, which chapter 5 introduces. First, functions.
