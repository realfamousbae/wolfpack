# 11. Testing

## Tests next to the code

Tests are part of the module they test, in its last section, `tests`. A test is
`test "name" { ... }`, and it fails when it panics, usually through a failing `assert`:

```wolf
module main

main {
    for n in 1..=5 {
        print(fizz(n))
    }
}

functions {
    fn fizz(n: Int) -> String {
        if n % 15 == 0 {
            "FizzBuzz"
        } else if n % 3 == 0 {
            "Fizz"
        } else if n % 5 == 0 {
            "Buzz"
        } else {
            n.to_string()
        }
    }
}

tests {
    fn all_of(numbers: List<Int>) -> List<String> {
        numbers.map(|n| fizz(n))
    }

    test "multiples of three are Fizz" {
        assert(fizz(3) == "Fizz")
        assert(fizz(9) == "Fizz")
    }

    test "multiples of fifteen are FizzBuzz" {
        assert(fizz(30) == "FizzBuzz")
    }

    test "other numbers stay numbers" {
        assert(all_of([1, 2, 4]) == ["1", "2", "4"])
    }

    test "this one is wrong" {
        assert(fizz(10) == "Fizz")
    }
}
```

- Tests see every name of their module, private ones included.
- The `tests` section may also hold helper functions, like `all_of`. They are visible only to the
  tests of the file.
- Test names are unique within a file. Tests cannot be called and are not part of the normal
  build: `wolf build` and `wolf run` leave them out.
- `assert` works anywhere, not only in tests. Since `==` compares lists and structs by value,
  `assert(all_of([1, 2, 4]) == ["1", "2", "4"])` checks a whole result at once.

## Running the tests

`wolf test` builds the tests for the default target and runs them:

```text
$ wolf test
test src/main.wolf: multiples of three are Fizz ... ok
test src/main.wolf: multiples of fifteen are FizzBuzz ... ok
test src/main.wolf: other numbers stay numbers ... ok
test src/main.wolf: this one is wrong ... FAILED

failures:

  src/main.wolf: this one is wrong
    panic at src/main.wolf:42:9: assertion failed: fizz(10) == "Fizz"

test result: FAILED. 3 passed; 1 failed
```

A failing `assert` names the condition and its place in the `.wolf` file. The exit code is 1 when a
test fails, and 0 when all pass.

- `wolf test fifteen` runs only the tests whose name contains `fifteen`.
- `wolf test --target go` and `wolf test --target rust` run the same tests on another target. The
  report has the same form on each.

## Integration tests

The files in `tests/` test the project from outside, through its public names. Each is a module
`tests.<name>` with only `imports`, helper `functions` and `tests`:

```wolf
// file: src/lib.wolf
module lib

functions {
    pub fn shout(text: String) -> String {
        text.to_upper_ascii() + "!"
    }
}

// file: tests/shouting.wolf
module tests.shouting

imports {
    lib.{shout}
}

tests {
    test "shouts" {
        assert(shout("hey") == "HEY!")
    }
}

// file: wolfpack.toml
[package]
name = "shouter"
version = "0.1.0"
kind = "lib"
```

`wolf test` runs the tests of `src/` and `tests/` together. A library is tested through its API,
the names of `src/lib.wolf`; for a program, integration tests see the `pub` names of the modules in
`src/`.
