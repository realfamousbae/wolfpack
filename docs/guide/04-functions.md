# 4. Functions and closures

## Functions

A function lists its parameters with their types and, after `->`, its result type. Without `->`
the function returns `Void`. The value of the body is the result; `return` leaves earlier:

```wolf
module main

functions {
    fn area(width: Int, height: Int) -> Int {
        width * height
    }

    fn clamp(value: Int, low: Int, high: Int) -> Int {
        if value < low {
            return low
        }
        value.min(high)
    }

    fn report(name: String, value: Int) {
        print(name + " = " + value.to_string())
    }

    fn main() {
        report("area", area(3, 4))
        report("clamped", clamp(150, 0, 100))
    }
}
```

```output
area = 12
clamped = 100
```

The types of parameters and results are always written; inside the body, types are inferred. There
are no default parameter values and no overloading: two functions of one module cannot have the
same name.

## Who may change a value: parameter modes

Wolfpack has no pointers and no references that you can store. Instead, each parameter has one of
three **modes**, which say what the function may do with the value it gets:

| Mode | Declared as | Called as | The function may |
| --- | --- | --- | --- |
| read | `names: List<String>` | `count(names)` | only read it; the caller keeps it |
| mut | `mut names: List<String>` | `add(mut names)` | change it in place; the caller keeps it |
| own | `own names: List<String>` | `consume(own names)` | do anything; the caller gives it away |

The mode is repeated at the call site, so every place where a value can change or disappear is
visible when you read the code:

```wolf
module main

functions {
    fn count(names: List<String>) -> Int {
        names.len()
    }

    fn add_guest(mut names: List<String>) {
        names.push("guest")
    }

    fn total_length(own names: List<String>) -> Int {
        var total = 0
        for name in own names {
            total += name.len()
        }
        total
    }

    fn main() {
        var names = ["Ann", "Bob"]
        add_guest(mut names)
        print(count(names).to_string())
        print(total_length(own names).to_string())
    }
}
```

```output
3
11
```

- A `mut` argument must be a variable declared with `var` (or a part of one): a `let` never
  changes.
- After `own names`, the variable `names` is **moved**: it no longer holds a value, and using it
  again is an error. The compiler points at the move.
- `Int`, `Float`, `Bool` and enums without data are small and simply copied, so they never need a
  marker and are never moved.
- When both the caller and the function need the value, pass a copy: `total_length(names.copy())`.
  A fresh value, like a copy, a literal or a call result, needs no `own` marker.

A function that reads its parameter cannot return it, or a part of it, without copying, because the
caller still owns it:

```wolf error E0504
module main

functions {
    fn first(names: List<String>) -> String {
        names[0]
    }

    fn main() {
        print(first(["Ann"]))
    }
}
```

Write `names[0].copy()` instead. These rules are the core of Wolfpack's ownership, which chapter 8
covers in full. They are what lets the compiler emit plain code in all three targets: borrowed
references in Rust, and no defensive copies in Go and TypeScript.

## Functions as values

A function type is written `fn(Int, Int) -> Int`. Named functions and closures are values of such
types:

```wolf
module main

functions {
    fn double(n: Int) -> Int {
        n * 2
    }

    fn apply_twice(f: fn(Int) -> Int, value: Int) -> Int {
        f(f(value))
    }

    fn main() {
        print(apply_twice(double, 5).to_string())
        print(apply_twice(|n| n + 3, 5).to_string())
    }
}
```

```output
20
11
```

## Closures

A closure is written `|parameters| body`. The parameter types are usually inferred from where the
closure is used; they can also be written, and a closure with a written result type has a block
body:

```wolf
module main

functions {
    fn main() {
        let numbers = [1, 2, 3, 4, 5, 6]
        let squares = numbers.map(|n| n * n)
        let even = numbers.count(|n| n % 2 == 0)
        let big = numbers.any(|n: Int| -> Bool {
            let limit = 5
            n > limit
        })
        print(squares[5].to_string() + " " + even.to_string() + " " + big.to_string())
    }
}
```

```output
36 3 true
```

A closure **captures** the variables it uses from around it. Numbers and booleans are copied into
the closure; other values are **moved** into it, so the closure owns them from then on. Copy a value
first if you still need it:

```wolf
module main

functions {
    fn main() {
        let names = ["Ann", "Bob"]
        let title = "Dr. "
        let prefix = title.copy()
        let titled = names.map(|n| prefix + n)
        print(titled[1])
        print(title)
    }
}
```

```output
Dr. Bob
Dr. 
```

Captured values cannot be changed inside the closure. A closure that needs to compute something new
returns it instead. `break` and `continue` inside a closure do not reach a loop around it; `return`
inside a closure returns from the closure.
