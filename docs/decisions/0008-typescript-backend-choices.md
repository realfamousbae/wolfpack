# 0008: Choices of the TypeScript backend

Status: accepted (author, 2026-10-09). **Deviates from `PLAN.md`** defaults recorded in chapter 8
(`match` as `switch`, generated `equals` and `copy` functions, `node dist/main.js`) and closes a gap
of chapter 8, section 4.1.

## Decision

1. `match` becomes an `if`/`else if` chain on `kind`; an arm with a guard turns the `match` into a
   labeled block. When every arm tests only the top-level variant, the last branch is a plain
   `else`, which TypeScript narrows by elimination.
2. The output project compiles with `rootDir` `.`, so `src/` and `examples/` keep their paths in
   `dist/`; a program runs with `node dist/src/main.js`.
3. `==` and `.copy()` of composite types use the structural `eq` and `copy` of the runtime instead
   of a generated function per type.
4. Function values (closures, functions stored or passed) return `Option` in the data form; a
   declared function used as a value is wrapped.
5. (2026-10-25, [INTERNAL]) **A chain of conditions** (`if let ... && ...`) is a labelled block:
   an `if` per item around the rest, the first block followed by `break $chain`, and the `else`
   branch after the `if`s. Without an `else`, or when the first block always leaves, there is no
   label. **Super-interfaces** are `extends`; the class that holds a payload-less enum as a value
   of an interface also has the methods of its super-interfaces.

## Reasons

1. A `switch` catches the `break` of a loop around it (`while true { match x { None => break } }`)
   and cannot express nested patterns or guards; an `if` chain can, and reads as well.
2. With `rootDir` `src`, examples outside `src/` could not be compiled in the same project.
3. One runtime function is shorter and as correct for every type, including generic ones; it keeps
   classes and compares maps independently of order.
4. What a function value returns is usually stored (`list.map(|x| halve(x))`); the data form is
   needed there anyway, and a single form for function types keeps them consistent.
5. The `else` branch is written once, also in a chain of `else if`s; a labelled block does not
   catch the `break` and `continue` of a loop around it.
