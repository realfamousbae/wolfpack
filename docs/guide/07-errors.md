# 7. Errors

Wolfpack has no `null` and no exceptions. A value that may be missing is an `Option`, an operation
that may fail returns a `Result`, and the types make sure that both cases are handled. For bugs,
the situations that should never happen, there are panics.

## `Option`: a value or nothing

`Option<T>` is either `Some(value)` or `None`:

```wolf
module main

main {
    let names = ["Ann", "Bob"]
    match find_index(names.copy(), "Bob") {
        Some(i) => print("found at " + i.to_string())
        None => print("not found")
    }
    match find_index(names, "Cid") {
        Some(i) => print("found at " + i.to_string())
        None => print("not found")
    }
}

functions {
    fn find_index(names: List<String>, wanted: String) -> Option<Int> {
        for i in 0..names.len() {
            if names[i] == wanted {
                return Some(i)
            }
        }
        None
    }
}
```

```output
found at 1
not found
```

A value of type `Option<Int>` is not an `Int`: to use the number inside, the code has to say what
happens when there is none, usually with a `match`.

## `Result`: a value or an error

`Result<T, E>` is either `Ok(value)` or `Err(error)`. `"42".parse_int()`, for example, returns a
`Result<Int, ParseError>`:

```wolf
module main

main {
    for text in ["42", "forty-two"] {
        match text.parse_int() {
            Ok(n) => print("number " + (n + 1).to_string())
            Err(e) => print("error: " + e.message())
        }
    }
}
```

```output
number 43
error: cannot parse forty-two
```

A `Result` cannot be silently ignored. A call whose `Result` is dropped is an error, so a failure
never goes unnoticed; when the result really does not matter, say so with `let _ = ...`:

```wolf error E0409
module main

imports {
    std.fs
}

main {
    fs.write_text("notes.txt", "remember the milk")
}
```

## Passing errors on with `?`

Most code does not handle an error where it happens but passes it to its caller. `?` after a
`Result` does exactly that: on `Ok(v)` it gives `v`, on `Err(e)` it returns `Err(e)` from the
function at once. On an `Option`, `?` gives the value or returns `None`.

## Your own errors

Every error type implements the built-in interface `Error`, which has one method:
`fn message(self) -> String`. An enum is the usual way to list what can go wrong. Interfaces are
covered in chapter 9; for now, an implementation is a block `Type: Error { ... }` in `impl`:

```wolf
module main

types {
    enum PortError {
        OutOfRange(Int)
    }
}

impl {
    PortError: Error {
        fn message(self) -> String {
            match self {
                PortError.OutOfRange(port) => "port " + port.to_string() + " is out of range"
            }
        }
    }
}

main {
    for text in ["8080", "http", "70000"] {
        match parse_port(text) {
            Ok(port) => print("listening on " + port.to_string())
            Err(e) => print(e.message())
        }
    }
}

functions {
    fn parse_port(text: String) -> Result<Int, Error> {
        let port = text.parse_int()?
        if port < 1 || port > 65535 {
            return Err(PortError.OutOfRange(port))
        }
        Ok(port)
    }
}
```

```output
listening on 8080
cannot parse http
port 70000 is out of range
```

`parse_port` returns `Result<Int, Error>`: its error is any value that implements `Error`. So `?`
can pass on the `ParseError` of `parse_int`, and `Err(PortError.OutOfRange(port))` is accepted
too. Where a function promises a specific error type instead, such as `Result<Int, PortError>`,
`?` passes on only errors of exactly that type.

## When `main` fails

The `main` section may be written `main -> Result<Void, Error> { ... }`. Then `?` works in `main`
too, and when `main` returns an error, the program writes `error: ` and the message to standard error and exits with code 1:

```wolf
module main

main -> Result<Void, Error> {
    let n = "12".parse_int()?
    print((n * 2).to_string())
    Ok(())
}
```

```output
24
```

With `"twelve"` instead of `"12"` the program would print `error: cannot parse twelve` and
exit with code 1.

## Panics

A panic stops the program at once. It is for bugs, not for errors that a correct program has to
expect. The language panics on integer overflow, division by zero and an index outside a list.
Your code panics with `panic("message")`, or with `assert(condition)` when the condition is false:

```wolf fragment
fn average(values: List<Int>) -> Int {
    assert(!values.is_empty())
    list.sum(values) / values.len()
}
```

A panic writes a message to standard error and exits with code 101. `panic` and `assert` name the
place in the `.wolf` file; the panics of the language itself do not, to keep the generated code
readable:

```text
panic at src/main.wolf:5:9: assertion failed: !values.is_empty()
panic: integer overflow
```

The output of a panic is the same on every target. There is no way to catch a panic: a part of a
program that can fail in an expected way returns a `Result`.
