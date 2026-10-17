# 8. Ownership

Chapter 4 introduced the three parameter modes. This chapter explains the model behind them, so that
the errors of the ownership check become predictable. The full rules are in
[chapter 5 of the specification](../spec/05-ownership.md).

## The idea

Every value has exactly **one owner**: a variable, a field of another value, an element of a list,
or a parameter in `own` mode. There are no references that can be stored, returned or kept in a
field. A function may only **borrow** a value for the time of a call, either to read it or to change
it, and a `for` loop or a `match` arm may borrow it for its body.

Because of this, a value that can be changed is never visible from anywhere else at the same time.
That is why the code can be translated as is: into references in Rust, and into ordinary shared
objects in Go and TypeScript, where no one else can observe the sharing. No lifetimes have to be
written, as there are no stored references to track.

## Copies and moves

`Int`, `Float`, `Bool`, `Void`, `Ordering` and enums without data are small: they are **copied**
whenever they are used. All other values are **moved** when they are given to a new owner:

- `let b = a` and `b = a`;
- an argument for an `own` parameter: `save(own a)`;
- `return a`, or `a` as the value at the end of a function;
- a field of a new struct, an element of a new list, the data of a variant: `[a]`, `Some(a)`;
- a closure that uses `a`;
- `match own a` and `for x in own a`.

After a move, the old name holds nothing, and every later use of it is an error that shows where
the value went:

```wolf error E0501
module main

main {
    let names = ["Ann", "Bob"]
    let team = names
    print(team.len().to_string())
    print(names.len().to_string())
}
```

Copy explicitly when two independent values are needed: `let team = names.copy()`. A `var` that
was moved becomes usable again when it gets a new value. The check follows every path through the
code: a value moved in one branch of an `if` may have been moved after it, which is an error too,
and so is moving a variable inside a loop that would use it again in the next round.

Operators only read their operands: `a + b`, `a == b` and `-a` never move anything.

## Only whole variables move

A field or an element cannot be moved out on its own, because the struct or the list would be left
with a hole. There are three ways to take a part:

```wolf
module main

types {
    struct Order {
        id: Int
        items: List<String>
    }
}

main {
    var order = Order { id: 1, items: ["tea", "cake"] }
    let copied = order.items.copy()
    let taken = replace(mut order.items, [])
    print(copied.len().to_string() + " " + taken.len().to_string())
    print(order.items.len().to_string())
    match own order {
        Order { id, items } => print(id.to_string() + " " + items.len().to_string())
    }
}
```

```output
2 2
0
1 0
```

- **Copy it**: `order.items.copy()`.
- **Swap something else in**: `replace(mut place, new)` stores the new value and returns the old
  one. The method `items.remove(i)` takes one element out of a list.
- **Take the whole value apart**: `match own order` moves `order` and gives the arm ownership of
  every part.

## Borrowing: read and mut

A read borrow (a parameter without a mode, a method with plain `self`, `for x in list`,
`match value`) can look at the value but cannot change it or move it. A `mut` borrow
(`mut` parameter, `mut self`, `for x in mut list`, `match mut value`) can change it in place, even
replace it as a whole, but still cannot move it away, since the owner keeps it.

```wolf
module main

types {
    enum Shape {
        Circle(Float)
        Square(Float)
    }
}

main {
    var shapes = [Shape.Circle(1.5), Shape.Square(2.0)]
    grow(mut shapes)
    for s in shapes {
        print(describe(s))
    }
}

functions {
    fn grow(mut shapes: List<Shape>) {
        for s in mut shapes {
            match mut s {
                Shape.Circle(r) => { r *= 2.0 }
                Shape.Square(side) => { side += 1.0 }
            }
        }
    }

    fn describe(s: Shape) -> String {
        match s {
            Shape.Circle(r) => "circle " + r.to_string()
            Shape.Square(side) => "square " + side.to_string()
        }
    }
}
```

```output
circle 3.0
square 3.0
```

`match mut s` gives the arms names for the parts of `s` that change `s` itself. An assignment is a
statement, not an expression, so an arm that changes something is a block: `=> { r *= 2.0 }`.

A `mut` argument must be a place that can change: a `var`, a `mut` or `own` parameter, or a part of
one. A temporary value, such as the result of a call, cannot be passed `mut`, since the change
would be lost.

## Exclusive access

While a place is borrowed for a change, nothing else may use it. The compiler checks this in two
situations.

**Within one call**, a place passed `mut` must not overlap with any other argument. Two places
overlap when one contains the other (`order` and `order.items`), or when both are elements of the
same list (`items[i]` and `items[j]`, since `i` and `j` may be equal). Different fields do not
overlap:

```wolf error E0508
module main

main {
    var numbers = [1, 2]
    move_all(mut numbers, mut numbers)
    print(numbers.len().to_string())
}

functions {
    fn move_all(mut from: List<Int>, mut to: List<Int>) {
        to.append(replace(mut from, []))
    }
}
```

**During a loop or a `match` arm**, the borrowed place cannot be changed through another name. A
`for x in list` body cannot push to `list`, and inside `for x in mut list` the list is reachable
only through `x`. Collect the changes and apply them after the loop, or loop over a copy.

## Closures

A closure owns what it captures: numbers and booleans are copied into it, other values are moved.
Captured values cannot be changed inside the closure, and a borrowed value, such as a read
parameter, cannot be captured at all; copy it into a local variable first:

```wolf
module main

main {
    let shouted = with_suffix(["hey", "you"], "!")
    print(shouted[0] + " " + shouted[1])
}

functions {
    fn with_suffix(words: List<String>, suffix: String) -> List<String> {
        let end = suffix.copy()
        words.map(|w| w + end)
    }
}
```

```output
hey! you!
```

## Reading the errors

| Code | Means |
| --- | --- |
| E0501, E0502 | a value is used after it was moved, or may have been |
| E0503 | a field or an element would be moved out on its own |
| E0504 | a borrowed value would be moved, often a field returned without `.copy()` |
| E0505, E0506 | a `let`, or a temporary, would be changed |
| E0507 | the `mut` or `own` marker at a call is missing or not needed |
| E0508, E0509 | a place is used while it is borrowed for a change |
| E0510, E0511 | a closure would capture a borrowed value, or change a captured one |

`wolf explain` shows each of them with a wrong and a fixed example.
