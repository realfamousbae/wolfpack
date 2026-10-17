# 9. Generics and interfaces

## Generic functions and types

A **type parameter** in angle brackets makes a function or a type work for many types at once:

```wolf
module main

types {
    /// A last-in, first-out stack of any element type.
    struct Stack<T> {
        items: List<T>
    }
}

impl {
    Stack<T> {
        fn new() -> Stack<T> {
            Stack { items: [] }
        }

        fn push(mut self, own item: T) {
            self.items.push(own item)
        }

        fn pop(mut self) -> Option<T> {
            self.items.pop()
        }

        fn map<U>(self, f: fn(T) -> U) -> Stack<U> {
            Stack { items: self.items.map(f) }
        }
    }
}

main {
    var numbers = Stack.new()
    numbers.push(1)
    numbers.push(2)
    var labels = numbers.map(|n| "#" + n.to_string())
    match labels.pop() {
        Some(label) => print(label)
        None => print("empty")
    }
    print(last_or(["a", "b"], "none"))
    let empty = List.<String>.new()
    print(last_or(own empty, "none"))
}

functions {
    fn last_or<T>(own items: List<T>, own fallback: T) -> T {
        var rest = items
        match rest.pop() {
            Some(item) => item
            None => fallback
        }
    }
}
```

```output
#2
b
none
```

- The type arguments are inferred from the use: `Stack.new()` becomes a `Stack<Int>` once `1` is
  pushed. When nothing tells the compiler the type, write it after a dot: `List.<String>.new()`,
  `parse.<Int>(text)`.
- Inside generic code, a `T` might be any type, so it is treated as a type that moves: `push` needs
  `own item`, even when `T` turns out to be `Int`.
- A method of a generic type may have type parameters of its own, like `map<U>`.
- Generic code is checked once, where it is declared, not separately for every use.

## Bounds

Inside a generic function, a value of type `T` supports only what every type supports: it can be
moved and passed on. Anything more is asked for with a **bound**, `T: Interface`, and several are
joined with `+`:

```wolf
module main

main {
    print(count_equal(["a", "b", "a"], "a").to_string())
    match largest([3, 9, 4]) {
        Some(n) => print(n.to_string())
        None => print("empty")
    }
}

functions {
    fn count_equal<T: Equatable>(items: List<T>, target: T) -> Int {
        var count = 0
        for item in items {
            if item == target {
                count += 1
            }
        }
        count
    }

    fn largest<T: Comparable + Copyable>(items: List<T>) -> Option<T> {
        if items.is_empty() {
            return None
        }
        var best = items[0].copy()
        for item in items {
            if item.compare(best) == Ordering.Greater {
                best = item.copy()
            }
        }
        Some(best)
    }
}
```

```output
2
9
```

A call must use types that satisfy the bounds; otherwise the error points at the call and names
the missing interface. Four interfaces are built in:

| Interface | Gives | Who has it |
| --- | --- | --- |
| `Equatable` | `==` and `!=` | numbers, `Bool`, `String`, and lists, maps, options, structs and enums whose contents have it; not functions |
| `Copyable` | `.copy()` | every type except functions and types that contain them |
| `Comparable` | `compare(self, other: Self) -> Ordering` | `Int`, `String`, and your types that implement it |
| `Hashable` | can be a map key | `Int`, `String`, `Bool`, enums without data; no others |

`Equatable` and `Copyable` are derived from the fields of a type and cannot be written by hand.
`Comparable` is implemented like any other interface.

## Interfaces

An **interface** is a set of method signatures. A type implements it in the `impl` section with a
block `Type: Interface { ... }` that defines every method, with exactly the declared signature:

```wolf
module main

imports {
    std.list
}

types {
    struct Rect {
        w: Float
        h: Float
    }

    struct Square {
        side: Float
    }
}

interfaces {
    interface Shape {
        fn area(self) -> Float
        fn name(self) -> String
    }
}

impl {
    Rect: Shape {
        fn area(self) -> Float {
            self.w * self.h
        }

        fn name(self) -> String {
            "rect"
        }
    }

    Rect: Comparable {
        fn compare(self, other: Self) -> Ordering {
            if self.area() < other.area() {
                Ordering.Less
            } else if self.area() > other.area() {
                Ordering.Greater
            } else {
                Ordering.Equal
            }
        }
    }

    Square: Shape {
        fn area(self) -> Float {
            self.side * self.side
        }

        fn name(self) -> String {
            "square"
        }
    }
}

main {
    let shapes: List<Shape> = [Rect { w: 2.0, h: 3.0 }, Square { side: 2.0 }]
    print(total_area(shapes.copy()).to_string())
    for shape in shapes {
        print(shape.name() + " " + shape.area().to_string())
    }
    var rects = [Rect { w: 5.0, h: 1.0 }, Rect { w: 1.0, h: 1.0 }]
    list.sort(mut rects)
    print(rects[0].area().to_string())
}

functions {
    fn total_area(shapes: List<Shape>) -> Float {
        var sum = 0.0
        for shape in shapes {
            sum += shape.area()
        }
        sum
    }
}
```

```output
10.0
rect 6.0
square 4.0
1.0
```

An interface is used in two ways:

- **As a bound**: `fn largest<T: Comparable + Copyable>(...)` works with any one type that has the
  interface. `list.sort` needs `Comparable`, so `Rect` implements it.
- **As a type**: `List<Shape>` holds values of different types that all implement `Shape`. A value
  of a concrete type becomes a `Shape` where a `Shape` is expected, as in the list above. Such a
  value supports the methods of the interface and `.copy()`, but not `==`, and it cannot be turned
  back into its concrete type.

Not every interface can be a type. When a method uses `Self` other than as the receiver, as in
`compare(self, other: Self)`, two values of the same type are needed, and a list of mixed shapes
could not provide them. `Comparable` is therefore only a bound.

A few more rules keep interfaces translatable to Go and Rust:

- Every interface method takes `self`, and has no type parameters of its own. The interface itself
  may have them: `interface Container<T>`.
- A type implements an interface at most once, in the module that declares the type.
- A type cannot have two methods with the same name, even from two different interfaces, and a
  method of a struct cannot have the name of one of its fields.
- Interfaces have no default method bodies and do not extend each other.
