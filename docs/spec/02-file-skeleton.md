# Chapter 2: File skeleton

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter defines the fixed structure of a `.wolf` file: the `module` line, the sections, their
order and what each section may contain. The exact grammar of each declaration is in chapter 3;
the meaning of imports, `extern` and `config` overrides is in chapters 6 and 7.

## 1. Overall shape

**[PLAN]** (section 5): a file starts with a `module` line and then consists only of sections, in a
fixed order. Outside sections only comments are allowed.

```
file    = inner_doc* "module" module_path NEWLINE section*
section = section_name "{" entry+ "}"
```

1. Before `module` only ordinary comments and inner doc comments (`//!`, `/*! */`) may appear.
   Inner doc comments document the module. **[PROPOSAL]**
2. `module` is followed by the module path (`users.storage`). The path must match the file path
   (`src/users/storage.wolf`) **[PLAN]**. That check needs the project layout and is specified in
   chapter 6; this chapter only requires the line to be present and to be first.
3. After the `module` line come zero or more sections. A file with only a `module` line is valid.
   **[PROPOSAL]** (see D2.4)
4. A section header is a section name followed by `{` on the same line. The closing `}` stands on
   its own line. **[PROPOSAL]** The section names are contextual keywords (chapter 1, section 4).

## 2. Sections and their order

| # | Section | Contents | Source |
| --- | --- | --- | --- |
| 0 | `module` | one line with the module path, required | [PLAN] |
| 1 | `imports` | imports of project modules and `std` | [PLAN] |
| 2 | `extern` | declarations from target-language libraries, one block per target | [PLAN] |
| 3 | `config` | module constants | [PLAN] |
| 4 | `types` | `struct`, `enum`, `type` aliases | [PLAN] |
| 5 | `interfaces` | `interface` declarations | [PLAN] |
| 6 | `impl` | methods of types and implementations of interfaces | [PLAN] |
| 7 | `main` | the entry point of the program, at most one block | author, 2026-10-17 |
| 8 | `functions` | free functions | [PLAN] |
| 9 | `tests` | tests of the module | [PLAN] |

Rules **[PLAN]**:

1. The order is fixed. A section that comes too late or too early is an error with a hint that names
   the section it must be moved before or after (E0202).
2. Each section appears at most once (E0203; the hint points to the first occurrence).
3. An empty section is not written: `types { }` is an error (E0204).
4. A declaration in the wrong section is an error with a hint naming the right section (E0205):
   `struct` inside `functions` does not compile.
5. Declarations inside a section may be in any order. The formatter does not reorder them.

Additional rules **[PROPOSAL]**:

6. **Order is layout, not visibility.** Every top-level name of a module is visible in the whole
   module, regardless of section and position. A struct field may have an interface type even
   though `interfaces` comes after `types`, and a method in `impl` may call a function from
   `functions`.
7. An unknown word in section position is an error (E0206), with a suggestion if it is close to a
   section name (`function {` gives "did you mean `functions`?").
8. A declaration written outside any section (for example a `fn` at the top level) is an error
   (E0207) with a hint naming the section it belongs in.

## 3. Contents of each section

The declaration syntax shown here is the starting point from `PLAN.md`; chapter 3 gives the grammar.

### 3.1 `imports`

One import per line **[PLAN]**:

```
imports {
    std.fs
    users.model.{User, Role}
}
```

The forms of an import (whole module, a group of names, aliases) and the rules for resolving them
are in chapter 6.

### 3.2 `extern`

Inside `extern` there are only target blocks: `ts { }`, `go { }`, `rust { }` **[PLAN]**.

```
extern {
    go {
        fn sha256(input: String) -> String from "crypto/sha256"
    }
}
```

1. Each target block appears at most once, in any order. **[PROPOSAL]**
2. A target block may not be empty (same rule as sections). **[PROPOSAL]**
3. Any other word as a block name is an error (E0209). **[PROPOSAL]**
4. What may be declared inside a block (functions, types), the meaning of `from`, and target binding
   of the module are in chapter 7.

### 3.3 `config`

One constant per line: `name: Type = value` **[PLAN]**.

```
config {
    max_users: Int = 100
    pub data_file: String = "users.db"
}
```

1. The type is always written. **[PLAN]** (types are inferred only for local variables)
2. Constants are immutable **[PLAN]** and can be overridden from `[config]` in `wolfpack.toml`
   (decisions register, item 4; rules in chapter 6).
3. A constant may be `pub`. **[PROPOSAL]**
4. The type of a constant is `Int`, `Float`, `Bool` or `String` (author, D2.1). These are the
   types that map directly onto TOML values in `[config]`.
5. The initializer is a **constant expression**, evaluated by the compiler (author, D2.1): literals,
   other `config` constants of any module, unary and binary arithmetic, bitwise, comparison and
   logical operators, and string concatenation. Function calls are not allowed. Example:
   `max_bytes: Int = max_users * 1024`.
6. Evaluation follows the normal rules of the language: an overflow or a division by zero in a
   constant expression is a compile error, not a runtime panic. A cycle between constants
   (`a = b`, `b = a`) is a compile error. Codes for these are in the type-checking range.

### 3.4 `types`

`struct`, `enum` and type aliases **[PLAN]**. A type alias is written `type Name = Type`
**[PROPOSAL]** (`type` is a reserved keyword, chapter 1).

```
types {
    pub struct FileStorage {
        path: String
        count: Int
    }

    pub enum StorageError {
        Full
        Io(String)
    }

    pub type UserId = Int
}
```

### 3.5 `interfaces`

Only `interface` declarations **[PLAN]**.

### 3.6 `impl`

Two forms of block **[PLAN]** for the second form; the first is **[PROPOSAL]**:

```
impl {
    FileStorage {                  // methods of the type itself (inherent methods)
        fn is_full(self) -> Bool { self.count >= max_users }
    }

    FileStorage: Storage {         // implementation of an interface
        fn save(mut self, u: User) -> Result<Void, StorageError> { ... }
    }
}
```

1. Only methods may appear inside a block. **[PROPOSAL]**
2. The same type may have one inherent block and one block per implemented interface; two inherent
   blocks for one type in the same module are an error (E0210). **[PROPOSAL]** (see D2.3)
3. `pub` is written on methods, not on blocks. Methods of an interface implementation take the
   visibility of the interface and cannot be marked `pub` themselves. **[PROPOSAL]**
4. Which types may be given an `impl` in which module (coherence, "orphan" rules) and the syntax for
   generic types are in chapter 4.

### 3.7 `main`

The entry point of a program is not a function but a section of its own. It holds a body and is
written above `functions` (author, 2026-10-17; it replaces `fn main` as the last declaration of
`functions`).

```
main {
    print("hello")
}

main(args: List<String>) -> Result<Void, Error> {
    let n = parse_count(args)?
    print("count: " + n.to_string())
}
```

The header is `main`, then an optional parameter list, then an optional `-> type`, then `{`, all on
one line. Both optional parts are independent, so all four forms are valid: `main { }`,
`main(args) { }`, `main -> Result<Void, Error> { }` and `main(args) -> Result<Void, Error> { }`.

1. The section holds a statement body, like a function body, not a list of entries. An empty
   `main { }` is an error, as for any section (E0204).
2. The parameter list has at most one parameter, whose type is `List<String>`: the command-line
   arguments without the program name, the same value as `std.io.args()`. Its name is free
   (`args`); the type may be left out (`main(args) { }`) and, when written, must be
   `List<String>`. Other parameters are E0614 (chapter 6).
3. The result type is omitted (then `Void`) or `Result<Void, Error>`. Anything else is E0614.
4. The section appears at most once (E0203), may not be `pub`, and cannot be called: it has no
   name in the module scope. Everything else in the module is visible inside it (section 2 rule 6).
5. `fn main` in `functions` is an error with a hint to use the `main` section (E0214).
6. The section may appear in any file, as `fn main` could (examples and tests use it); chapter 6
   says where it is required.

### 3.8 `functions`

Only `fn` declarations, optionally `pub` **[PLAN]**. In `std` modules a function may be marked
`intrinsic` and then has no body (chapter 7). The entry point is not declared here but in the
`main` section (section 3.7).

### 3.9 `tests`

```
tests {
    test "new storage is empty" {
        let s = open("tmp.db")
        assert(s.count == 0)
    }
}
```

1. A test is `test "name" { body }` **[PLAN]**.
2. Test names are unique within a file (E0211). **[PROPOSAL]**
3. Tests cannot be `pub` and cannot be called. **[PROPOSAL]**
4. Helper functions may be declared inside `tests` (author, D2.2). They are written like functions in
   `functions` but cannot be `pub`, are visible only to tests and helpers of the same file, and are
   not part of the non-test build.
5. Tests see every name of the module, private ones included. **[PROPOSAL]**

## 4. Doc comments in the skeleton

1. An outer doc comment (`///`, `/** */`) may stand before a declaration inside a section, before an
   enum variant, a struct field, a method and a test. Anywhere else it is an error (E0212).
   **[PROPOSAL]**
2. An outer doc comment before a section header is an error, because sections are not declarations
   (E0212, hint: use `//!` at the top of the file to document the module). **[PROPOSAL]**
3. Inner doc comments are allowed only before the `module` line (E0212 elsewhere). **[PROPOSAL]**

## 5. Diagnostics

Proposed range E02xx for skeleton errors (confirmed in chapter 9). All of them are reported by the
parser, which recovers and continues so that every skeleton error of a file is shown at once.

| Code | Error | Hint |
| --- | --- | --- |
| E0201 | File does not start with `module` (or the line appears twice) | add `module <path>` as the first line |
| E0202 | Section out of order | move `types` before `impl` |
| E0203 | Duplicate section | merge into the first `types` at line N |
| E0204 | Empty section or empty `extern` target block | remove the section |
| E0205 | Declaration in the wrong section | move this `struct` into `types` |
| E0206 | Unknown section name | did you mean `functions`? |
| E0207 | Declaration outside any section | put this `fn` into `functions` |
| E0208 | Content after `module` on the same line, or a missing module path | |
| E0209 | Unknown or duplicate `extern` target block | targets are `ts`, `go`, `rust` |
| E0210 | Second inherent `impl` block for the same type | merge with the block at line N |
| E0211 | Duplicate test name | |
| E0212 | Doc comment in a position where it documents nothing | |
| E0213 | Retired: `fn main` is no longer a declaration of `functions` (was: not last, added in M5). The code is never reused | |
| E0214 | `fn main` in `functions` | declare the entry point as a `main { }` section above `functions` |

Recovery strategy for the parser **[PROPOSAL]**: on a skeleton error the parser still parses the
contents of the offending section normally, so that errors inside it are also reported. An
out-of-order or duplicate section is treated as if it were in the right place.

## 6. Full example

The example from `PLAN.md` section 5, unchanged, is a valid file under this chapter.

## 7. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D2.1 | `config`: allowed types and initializers | **answered**: types `Int`, `Float`, `Bool`, `String` only (they map onto `[config]` in the manifest); initializer is a constant expression: literals, other constants, arithmetic, comparison and logical operators, string concatenation, no function calls |
| D2.2 | Helper functions inside `tests` | **answered**: yes: `fn` declarations are allowed in `tests`, are private and visible only to tests of the same file; they are not compiled into the non-test build |
| D2.3 | `impl` block forms: inherent `Type { }` plus `Type: Interface { }`, at most one inherent block per type in a module | **answered**: yes |
| D2.4 | A file with only a `module` line is valid | **answered**: yes |
| D2.5 | Rest of the chapter: order is layout not visibility (2.6), unknown section and stray declaration errors (2.7, 2.8), `extern` target blocks (3.2), `pub` rules for config, methods and tests, doc comment placement (section 4), diagnostics list (section 5) | **answered**: yes |
