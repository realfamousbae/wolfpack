# Chapter 2: File skeleton

Status: **DRAFT, awaiting author review.**

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
| 7 | `functions` | free functions | [PLAN] |
| 8 | `tests` | tests of the module | [PLAN] |

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
4. Which types and which initializer expressions are allowed: see D2.1.

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

### 3.7 `functions`

Only `fn` declarations, optionally `pub` **[PLAN]**. In `std` modules a function may be marked
`intrinsic` and then has no body (chapter 7). In `src/main.wolf` of a `cli` project this section
contains `fn main` **[PLAN]** (section 6; signature in chapter 6).

### 3.8 `tests`

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
4. Whether helper functions may be declared inside `tests`: see D2.2.
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

Recovery strategy for the parser **[PROPOSAL]**: on a skeleton error the parser still parses the
contents of the offending section normally, so that errors inside it are also reported. An
out-of-order or duplicate section is treated as if it were in the right place.

## 6. Full example

The example from `PLAN.md` section 5, unchanged, is a valid file under this chapter.

## 7. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D2.1 | `config`: allowed types and initializers | types `Int`, `Float`, `Bool`, `String` only (they map onto `[config]` in the manifest); initializer is a constant expression: literals, other constants, arithmetic, comparison and logical operators, string concatenation, no function calls |
| D2.2 | Helper functions inside `tests` | yes: `fn` declarations are allowed in `tests`, are private and visible only to tests of the same file; they are not compiled into the non-test build |
| D2.3 | `impl` block forms: inherent `Type { }` plus `Type: Interface { }`, at most one inherent block per type in a module | yes |
| D2.4 | A file with only a `module` line is valid | yes |
| D2.5 | Rest of the chapter: order is layout not visibility (2.6), unknown section and stray declaration errors (2.7, 2.8), `extern` target blocks (3.2), `pub` rules for config, methods and tests, doc comment placement (section 4), diagnostics list (section 5) | yes |
