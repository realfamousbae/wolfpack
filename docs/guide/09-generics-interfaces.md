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

### Methods that need more

A method of a generic type may need more from `T` than the type itself does. It says so with
`where` after its result type, and exists only for the types that have it:

```wolf
module main

types {
    struct Shelf<T> {
        items: List<T>
    }
}

impl {
    Shelf<T> {
        fn size(self) -> Int {
            self.items.len()
        }

        fn has(self, item: T) -> Bool where T: Equatable {
            for x in self.items {
                if x == item {
                    return true
                }
            }
            false
        }
    }
}

main {
    let books = Shelf { items: ["Dune", "Emma"] }
    print(f"{books.size()} {books.has("Emma")}")
    let tasks = Shelf { items: [|| 1] }
    print(tasks.size().to_string())
}
```

```output
2 true
1
```

`tasks.size()` works for a shelf of functions, but `tasks.has(...)` would be an error at the call:
functions are not `Equatable`. Lists work the same way: `items.contains(x)`, `items.sort()` and
`items.max()` exist for the lists whose elements can do it. `where` names only the type
parameters of the `impl` block; a free function puts its bounds after its name, `<T: Equatable>`.

### Methods for one instantiation

A block may also be written for one instantiation of a generic type: its head gives a type
instead of declaring a parameter. Its methods exist only for that instantiation, next to the
methods of every `Shelf<T>`:

```wolf
module main

types {
    struct Shelf<T> {
        items: List<T>
    }
}

impl {
    Shelf<T> {
        fn size(self) -> Int {
            self.items.len()
        }
    }

    Shelf<Int> {
        fn total(self) -> Int {
            var sum = 0
            for x in self.items {
                sum += x
            }
            sum
        }
    }
}

main {
    let numbers = Shelf { items: [3, 4, 5] }
    print(f"{numbers.size()} {numbers.total()}")
    let books = Shelf { items: ["Dune"] }
    print(books.size().to_string())
}
```

```output
3 12
1
```

`books.total()` would be an error (E0424): `total` is a method of `Shelf<Int>` only. An argument of
the head that names a type (`Int`, a type of your program or an imported one) or that is more than a
single name (`List<String>`, `fn(Int) -> Int`) is a type argument; a single name that names no type
declares a parameter, so a head may mix both: `Pair<String, T> { ... }`. A method name is used once
across all the blocks of a type, and only inherent blocks can be written for one instantiation: an
interface is implemented for every instantiation (`Shelf<Int>: Display` is E0448).

## Interfaces

An **interface** is a set of method signatures. A type implements it in the `impl` section with a
block `Type: Interface { ... }` that defines every method, with exactly the declared signature:

```wolf
module main

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
    rects.sort()
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
  interface. `rects.sort()` needs `Comparable`, so `Rect` implements it.
- **As a type**: `List<Shape>` holds values of different types that all implement `Shape`. A value
  of a concrete type becomes a `Shape` where a `Shape` is expected, as in the list above. Such a
  value supports the methods of the interface and `.copy()`, but not `==`, and it cannot be turned
  back into its concrete type.

The built-in interface `Display` gives the text of a value: `fn to_string(self) -> String`. `Int`,
`Float`, `Bool` and `String` implement it, lists, maps, options and results do when their contents
do (chapter 6), and so can your own types. Interpolated strings (chapter 2) and `assert_eq`
(chapter 11) take any `Display` value:

```wolf
module main

types {
    struct Money {
        cents: Int
    }
}

impl {
    Money: Display {
        fn to_string(self) -> String {
            f"{self.cents / 100}.{self.cents % 100} EUR"
        }
    }
}

main {
    let price = Money { cents: 1250 }
    print(f"price: {price}")
    print(labelled("total", price))
}

functions {
    fn labelled<T: Display>(label: String, value: T) -> String {
        f"{label} = {value}"
    }
}
```

```output
price: 12.50 EUR
total = 12.50 EUR
```

Not every interface can be a type. When a method uses `Self` other than as the receiver, as in
`compare(self, other: Self)`, two values of the same type are needed, and a list of mixed shapes
could not provide them. `Comparable` is therefore only a bound.

### Default methods

A method of an interface may have a body, its **default**. A type that implements the interface
gets the default when its block leaves the method out, and replaces it with `override fn`. The
interface's own `Named.greeting(x)` calls the default, also from a replacement that builds on it:

```wolf
module main

types {
    struct Cat {
        nick: String
    }

    struct Dog {
        title: String
    }
}

interfaces {
    interface Named {
        fn name(self) -> String

        fn greeting(self) -> String {
            "Hello, " + self.name() + "!"
        }
    }
}

impl {
    Cat: Named {
        fn name(self) -> String {
            self.nick.copy()
        }
    }

    Dog: Named {
        fn name(self) -> String {
            self.title.copy()
        }

        override fn greeting(self) -> String {
            Named.greeting(self) + " Woof!"
        }
    }
}

main {
    let pets: List<Named> = [Cat { nick: "Tom" }, Dog { title: "Rex" }]
    for pet in pets {
        print(pet.greeting())
    }
}
```

```output
Hello, Tom!
Hello, Rex! Woof!
```

- A default is checked once, for any type that implements the interface: on `self` it can use the
  methods of the interface and nothing else, so `self.copy()` or `==` is an error there.
- A method that replaces a default must say `override`, and `override` on anything else is an
  error (E0443): a default is never replaced by accident.
- The defaults a type gets are its methods, so they cannot have the name of another method of the
  type or of one of its fields.

A few more rules keep interfaces translatable to Go and Rust:

- Every interface method takes `self`, and has no type parameters of its own. The interface itself
  may have them: `interface Container<T>`.
- A type implements an interface at most once, in the module that declares the type.
- A type cannot have two methods with the same name, even from two different interfaces, and a
  method of a struct cannot have the name of one of its fields.

### Super-interfaces

An interface may name other interfaces after `:`, its **super-interfaces**. A type that
implements it must implement them too, each in its own block, and wherever a type is known to
implement the interface, it has their methods as well: in a default, through a bound, and on a
value of the interface.

```wolf
module main

types {
    struct Dog {
        title: String
    }
}

interfaces {
    interface Named {
        fn name(self) -> String
    }

    interface Greeter: Named + Display {
        fn greeting(self) -> String {
            f"Hello, {self.name()}, {self}!"
        }
    }
}

impl {
    Dog: Named {
        fn name(self) -> String {
            self.title.copy()
        }
    }

    Dog: Display {
        fn to_string(self) -> String {
            "a good dog"
        }
    }

    Dog: Greeter {}
}

main {
    print(welcome(Dog { title: "Rex" }))
}

functions {
    fn welcome<T: Greeter>(guest: T) -> String {
        guest.greeting() + " Come in, " + guest.name() + "."
    }
}
```

```output
Hello, Rex, a good dog! Come in, Rex.
```

- A super-interface is an interface of the program, `Error`, `Comparable` or `Display`.
  `Equatable`, `Copyable` and `Hashable` cannot be one (E0445): no block implements them.
- `Dog: Greeter` without `Dog: Named` is an error (E0446), and so is a method of `Greeter` with
  the name of a method of `Named` (E0447).
- An interface with `Comparable` among its super-interfaces is a bound only, like `Comparable`
  itself. A value of type `Greeter` is not converted to a value of type `Named`.
