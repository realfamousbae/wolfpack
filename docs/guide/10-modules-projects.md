# 10. Modules and projects

## One file, one module

Each `.wolf` file under `src/` is a module, named after its path: `src/geometry.wolf` is
`geometry`, `src/users/model.wolf` is `users.model`. The `module` line repeats that name. A
directory is only a namespace; there are no index files.

The examples of this chapter have several files. A comment line `// file: <path>` starts each file
other than `src/main.wolf`; it only separates the files and is not part of the code.

```wolf
module main

imports {
    geometry.{Point, distance}
    text.format
}

functions {
    fn main() {
        let a = Point { x: 0, y: 0 }
        let b = Point.new(3, 4)
        print(format.labeled("distance", distance(a, b)))
    }
}

// file: src/geometry.wolf
module geometry

types {
    pub struct Point {
        pub x: Int
        pub y: Int
    }
}

impl {
    Point {
        pub fn new(x: Int, y: Int) -> Point {
            Point { x, y }
        }
    }
}

functions {
    /// The distance in steps along the grid.
    pub fn distance(a: Point, b: Point) -> Int {
        (a.x - b.x).abs() + (a.y - b.y).abs()
    }
}

// file: src/text/format.wolf
module text.format

functions {
    pub fn labeled(label: String, value: Int) -> String {
        label + ": " + value.to_string()
    }
}
```

```output
distance: 7
```

## Imports

Another module's names are used only through an import in the `imports` section:

| Import | Then write |
| --- | --- |
| `text.format` | `format.labeled(...)`: the module under its last name |
| `text.format as fmt` | `fmt.labeled(...)` |
| `geometry.{Point, distance}` | `Point`, `distance(...)` directly |
| `geometry.{distance as steps}` | `steps(...)` |

Paths are always written in full from the root; the standard library starts with `std.`
(`std.list`, `std.fs`). The **prelude** needs no import: the built-in types, `Some`, `None`, `Ok`,
`Err`, the built-in interfaces and `Error`, and the functions `print`, `panic`, `assert` and
`replace`. The methods of built-in types, such as `"a".len()` or `items.push(x)`, also need no
import.

Three rules keep the imports honest:

- An unused import is an error, so the list shows what a module really depends on.
- Imports must not form a cycle. If two modules need each other, move what they share into a third.
- A name may not be declared twice in one module, imports included; `as` resolves a clash.

## Visibility

Everything is private to its module unless it is marked `pub`: functions, types, constants,
methods and struct fields each have their own `pub`. The variants of a `pub` enum are public with
it.

A struct with a private field cannot be built outside its module, since a literal must give every
field. This is how a module protects the invariants of its types: it offers a `pub` function that
builds the value, like `Point.new` above. A `pub` declaration also cannot mention a private type,
because the users of the declaration could not name it.

## The manifest

`wolfpack.toml` describes the project:

```toml
[package]
name = "my-app"           # kebab-case
version = "0.1.0"
kind = "cli"              # cli or lib

[targets]
default = "ts"
enabled = ["ts", "go", "rust"]

[config.main]             # overrides of constants of the module main
max_users = 500

[extern.ts.dependencies]  # packages of the target languages (chapter 12)
"left-pad" = "^1.3.0"
```

Only `[package]` is required. An unknown key is an error, so a typo does not go unnoticed.

### Overriding constants

`[config.<module>]` replaces the values of `config` constants of that module when the project is
built. The value must have the type of the constant, and constants computed from it see the new
value:

```wolf
module main

config {
    max_users: Int = 100
    max_sessions: Int = max_users * 2
    greeting: String = "hello"
}

functions {
    fn main() {
        print(greeting + " " + max_sessions.to_string())
    }
}

// file: wolfpack.toml
[package]
name = "server"
version = "0.1.0"
kind = "cli"

[config.main]
max_users = 500
greeting = "hi"
```

```output
hi 1000
```

## Libraries

A project with `kind = "lib"` is a library. Its code starts in `src/lib.wolf`, which has no `main`
and decides what the library offers: its own `pub` declarations, plus the names it re-exports with
`pub` imports. Only `src/lib.wolf` may re-export.

```wolf
// file: src/lib.wolf
module lib

imports {
    pub text.words.{word_count}
}

// file: src/text/words.wolf
module text.words

functions {
    pub fn word_count(text: String) -> Int {
        text.split(" ").count(|w| !w.is_empty())
    }
}

// file: wolfpack.toml
[package]
name = "textkit"
version = "0.1.0"
kind = "lib"
```

Every type that appears in the public API must itself be part of it, so that the users of the
library can name it.

## Tests and examples

Next to `src/`, a project has two more directories:

- `tests/` holds **integration tests**. Each file is a module `tests.<name>` with only `imports`,
  helper `functions` and `tests`. It sees the project from outside: a library through its API
  (`imports { lib.{word_count} }`), a program through the `pub` names of its modules. Chapter 11
  covers tests.
- `examples/` holds **usage examples**: small programs `examples.<name>`, each with its own
  `fn main`, which see the project the same way. `wolf run --example <name>` runs one.

The generated code goes to `out/`, which is not part of the source and is ignored by git.
