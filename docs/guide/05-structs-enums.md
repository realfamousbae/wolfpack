# 5. Structs and enums

## Structs

A `struct` groups named fields. It is declared in the `types` section and built with a literal
that gives every field:

```wolf
module main

types {
    struct User {
        name: String
        age: Int
    }
}

functions {
    fn main() {
        let name = "Ann"
        let ann = User { name, age: 31 }
        var bob = User { name: "Bob", age: 25 }
        bob.age += 1
        print(ann.name + " " + ann.age.to_string())
        print(bob.name + " " + bob.age.to_string())
        print((ann == bob).to_string())
        let twin = bob.copy()
        print((twin == bob).to_string())
    }
}
```

```output
Ann 31
Bob 26
false
true
```

- `User { name }` is short for `User { name: name }`.
- Fields of a `var` can be changed; nothing of a `let` can.
- `==` compares two values field by field, and `.copy()` makes an independent copy. Both come for
  free with every struct whose fields support them; a field of a function type, for example, has
  no `==`.
- Types are nominal: two structs with the same fields are still different types.

## Methods

Methods go into the `impl` section, in a block named after the type. The first parameter of a
method is `self`, in one of the three modes of chapter 4: `self` reads the value, `mut self` may
change it, and `own self` consumes it. A function in the block without `self` is an **associated
function**, called on the type, often to build a value. Inside the block, `Self` names the type:

```wolf
module main

types {
    struct Counter {
        name: String
        value: Int
    }
}

impl {
    Counter {
        fn new(own name: String) -> Self {
            Self { name, value: 0 }
        }

        fn add(mut self, amount: Int) {
            self.value += amount
        }

        fn label(self) -> String {
            self.name.to_upper_ascii()
        }

        fn finish(own self) -> String {
            self.name + ": " + self.value.to_string()
        }
    }
}

functions {
    fn main() {
        var clicks = Counter.new("clicks")
        clicks.add(2)
        clicks.add(3)
        print(clicks.label())
        print(clicks.finish())
    }
}
```

```output
CLICKS
clicks: 5
```

The receiver is not marked at the call: `clicks.add(2)` needs `clicks` to be a `var` because `add`
takes `mut self`, and after `clicks.finish()` the variable is moved, because `finish` takes
`own self`. `finish` may use `self.name` in its result because it owns `self`; a method with a
plain `self` only borrows the value, so to return a field it would return `self.name.copy()`.

Each type has one such block, in the module that declares the type. Methods cannot be added to
types of other modules or to built-in types; write a function instead.

A method also cannot have the name of a field of its struct: TypeScript and Go keep fields and
methods in one namespace. That is why the field above is `name` and the method `label`.

## Enums

An `enum` is a type whose value is one of several **variants**. A variant can carry no data,
positional data, or named fields. Variants are written with the name of the enum:

```wolf
module main

types {
    enum Shape {
        Circle(Float)
        Rect { w: Float, h: Float }
        Empty
    }
}

functions {
    fn area(s: Shape) -> Float {
        match s {
            Shape.Circle(r) => 3.0 * r * r
            Shape.Rect { w, h } => w * h
            Shape.Empty => 0.0
        }
    }

    fn main() {
        let shapes = [Shape.Circle(1.0), Shape.Rect { w: 2.0, h: 3.0 }, Shape.Empty]
        for s in shapes {
            print(area(s).to_string())
        }
    }
}
```

```output
3.0
6.0
0.0
```

`match` takes a value apart: `Shape.Circle(r)` matches a circle and names its radius `r`,
`Shape.Rect { w, h }` names the fields. A struct pattern may end with `..` to ignore the other
fields: `Shape.Rect { w, .. }`. Patterns nest, so `Some(Shape.Circle(r))` is a pattern too. The
`match` must cover every variant; when a new variant is added, the compiler lists every `match` that
misses it.

An enum whose variants carry no data is a simple set of names. Its values are copied like numbers:

```wolf
module main

types {
    enum Light {
        Red
        Yellow
        Green
    }
}

impl {
    Light {
        fn next(self) -> Light {
            match self {
                Light.Red => Light.Green
                Light.Green => Light.Yellow
                Light.Yellow => Light.Red
            }
        }
    }
}

functions {
    fn main() {
        var light = Light.Red
        for step in 0..4 {
            light = light.next()
            print(step.to_string() + " " + (light == Light.Red).to_string())
        }
    }
}
```

```output
0 false
1 false
2 true
3 false
```

## Recursive types

A type may contain itself through an enum variant, an `Option` or a `List`, which can be empty, but
not directly: a struct that contains itself would never end.

```wolf
module main

types {
    enum Tree {
        Leaf
        Node(Tree, Int, Tree)
    }
}

functions {
    fn insert(own tree: Tree, value: Int) -> Tree {
        match own tree {
            Tree.Leaf => Tree.Node(Tree.Leaf, value, Tree.Leaf)
            Tree.Node(left, v, right) => if value < v {
                Tree.Node(insert(own left, value), v, right)
            } else {
                Tree.Node(left, v, insert(own right, value))
            }
        }
    }

    fn sum(tree: Tree) -> Int {
        match tree {
            Tree.Leaf => 0
            Tree.Node(left, v, right) => sum(left) + v + sum(right)
        }
    }

    fn main() {
        var tree = Tree.Leaf
        for n in [5, 2, 8] {
            tree = insert(own tree, n)
        }
        print(sum(tree).to_string())
    }
}
```

```output
15
```

`match own tree` takes the tree apart and gives the arms ownership of its parts, so they can be
moved into the new tree. Chapter 8 explains these forms of `match`.

## Type aliases

`type UserId = Int` gives another name to an existing type. It is not a new type: a `UserId` and an
`Int` can be used in place of each other. Aliases may have type parameters:
`type Pairs<T> = List<List<T>>`.
