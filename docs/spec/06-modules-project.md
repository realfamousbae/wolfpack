# Chapter 6: Modules, names and the project

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter defines modules and imports, how names are resolved, visibility, the project layout and
the manifest `wolfpack.toml`. The `std` modules and `extern` are in chapter 7; the generated projects
in `out/` are in chapter 8.

Decisions already taken by the author for this chapter (2026-10-07):

- A local variable may shadow another name only in a **nested** block, not in the same block.
- The **prelude** contains the built-in types, `Some`/`None`/`Ok`/`Err`, `Ordering`, the built-in
  interfaces and the functions `print`, `panic`, `assert`.
- The public API of a `lib` project is the `pub` part of `src/lib.wolf` plus what it re-exports with
  `pub` imports.
- `fn main()` or `fn main() -> Result<Void, Error>`.

## 1. Modules

1. One file is one module **[PLAN]**. The module path is the file path below `src/` without the
   extension, with `/` replaced by `.`: `src/users/storage.wolf` is `users.storage` **[PLAN]**.
2. The `module` line must state exactly this path (E0610).
3. The entry modules are `main` (`src/main.wolf`, kind `cli`) and `lib` (`src/lib.wolf`, kind `lib`)
   **[PLAN]**. A project has exactly one of them, matching its kind.
4. Every segment of a module path follows `snake_case` (chapter 1) and is not a reserved keyword.
   File and directory names in `src/` therefore are `snake_case` as well.
5. Index files for directories do not exist **[PLAN]**: `src/users.wolf` and a directory
   `src/users/` cannot both exist (E0612). A directory is only a namespace.
6. The root segment `std` is reserved for the standard library. The root segments `tests` and
   `examples` are reserved for the directories of the same names (section 7).

## 2. Imports

### 2.1 Forms

```
import      = "pub"? module_path ( "as" IDENT )?
            | "pub"? module_path "." "{" sep(import_name, ",") "}"
import_name = IDENT ( "as" IDENT )?
```

| Form | Effect |
| --- | --- |
| `std.fs` | the module is visible under its last segment: `fs.read_text(p)` |
| `std.fs as files` | the module is visible as `files` |
| `users.model.{User, Role}` | the names `User` and `Role` are visible directly |
| `users.model.{User as Person}` | `User` is visible as `Person` |

1. `as` is a contextual keyword, special only inside `imports`. **[PROPOSAL]**
2. An alias follows the naming style of what it names: a module alias and a function alias are
   `snake_case`, a type alias is `PascalCase`.
3. There are no glob imports (`users.model.*`). **[PROPOSAL]**
4. Every module path, including `std` paths, is written in full from the root. There are no
   relative paths (`super`, `self`). **[PROPOSAL]**
5. Names of another module can only be used through an import. Writing `std.fs.read_text(p)` without
   importing is an error. **[PROPOSAL]**
6. A module may not import itself, and may not import an entry module (`main`, `lib`) from `src/`.
   **[PROPOSAL]**
7. **Cyclic imports are forbidden** **[PLAN]**. The error lists the whole cycle (E0604).
8. An **unused import is an error** (E0615), as in Go. **[PROPOSAL]** (see D6.3)

### 2.2 Re-exports

A `pub` import makes the imported names part of the importing module's own public names.
Re-exports are allowed **only in `src/lib.wolf`**, where they build the library API (author;
restriction to `lib.wolf` is **[PROPOSAL]**, see D6.2):

```
module lib

imports {
    pub users.model.{User, Role}
    pub users.storage.{open}
}
```

## 3. Name resolution

### 3.1 Scopes

From the innermost to the outermost:

1. local scopes: blocks, `match` arms, closure bodies, loop bodies;
2. the parameters of the function (the same scope as the top block of its body);
3. the top-level names of the module: everything declared in its sections (chapter 2 rule 2.6:
   order is layout, not visibility);
4. imported names;
5. the **prelude** (section 3.3).

### 3.2 Conflicts and shadowing

1. Two top-level declarations of one module with the same name are an error (E0605). Types and
   values share one namespace: a struct `User` and a function `user` do not clash (different case),
   but two `User`s do.
2. An imported name that equals a top-level name of the module, or another imported name, is an
   error; the hint suggests `as` (E0605).
3. A local binding may **shadow** a name from an outer scope (another local, a parameter, a
   top-level name, an import) only if it is in a **nested** block (author). Declaring the same name
   twice in one block (including a parameter and a `let` in the top block of the body) is an error
   (E0606).
4. `match` arms and closure bodies are nested blocks, so `Some(x)` in two arms is fine.
5. Prelude names cannot be declared again at the top level of a module (E0617). **[PROPOSAL]**

### 3.3 The prelude

Available in every module without imports (author):

- the built-in types `Int`, `Float`, `Bool`, `String`, `Void`, `List`, `Map`, `Option`, `Result`,
  `Ordering`;
- the variants `Some`, `None`, `Ok`, `Err`;
- the built-in interfaces `Equatable`, `Copyable`, `Hashable`, `Comparable`, and the error interface
  `Error` (chapter 7);
- the functions `print` (defined in `std.io`, re-exported by the prelude), `panic` and `assert`.

`assert` is usable everywhere, not only in tests **[PROPOSAL]**. Its exact signature and the
behaviour of a failed assertion are in chapter 7.

## 4. Visibility

1. Everything is private to its module; `pub` makes a declaration visible to other modules **[PLAN]**.
2. `pub` applies to: functions, structs, enums, type aliases, interfaces, `config` constants, struct
   fields, inherent methods and associated functions (chapter 2, chapter 3).
3. The variants of a `pub` enum are public. The methods of an interface implementation have the
   visibility of the interface (chapter 2). **[PROPOSAL]**
4. A struct with at least one private field cannot be built with a literal outside its module
   (chapter 4, rule 7.1.1 needs every field). This is how a module keeps invariants: it offers a
   `pub` associated function such as `FileStorage.open(path)` instead.
5. **No private types in public signatures.** A `pub` function, method, field, constant or alias
   may only mention types that are visible wherever the declaration is visible (E0608).
   **[PROPOSAL]**
6. **Library API.** For kind `lib`, code outside the project sees only the `pub` names of
   `src/lib.wolf` and its `pub` imports (author). Every type that appears in a signature of that
   API must itself be part of the API (E0608). Inside the project, `pub` means "visible to the other
   modules of this project".

## 5. The entry point

1. Kind `cli`: `src/main.wolf` must declare `fn main()` or `fn main() -> Result<Void, Error>` in
   `functions` (author). Any other signature is an error (E0614). `main` cannot be `pub`.
2. When `main` returns `Err(e)`, the program writes the error to standard error and exits with code
   1. When it returns normally, the exit code is 0. A panic exits with code 101 **[PROPOSAL]** (the
   Rust convention; chapter 8 makes every target do the same).
3. Command-line arguments and an explicit exit code come from `std.io` and `std.process`
   (chapter 7).
4. Kind `lib`: `src/lib.wolf` has no `main`.

## 6. The manifest `wolfpack.toml`

```toml
[package]
name = "my-app"
version = "0.1.0"
kind = "cli"              # cli | lib

[targets]
default = "ts"
enabled = ["ts", "go", "rust"]

[config.users.storage]    # overrides for constants of module users.storage
max_users = 200

[extern.go.dependencies]
"github.com/some/lib" = "v1.2.0"

[extern.ts.dependencies]
"some-lib" = "^2.0.0"
```

1. `[package]` is required **[PLAN]**:
   - `name`: `kebab-case`, pattern `[a-z][a-z0-9]*(-[a-z0-9]+)*`. Backends derive target names from
     it (chapter 8). **[PROPOSAL]**
   - `version`: a semantic version `MAJOR.MINOR.PATCH`. **[PROPOSAL]**
   - `kind`: `cli` or `lib` **[PLAN]**.
2. `[targets]` is optional **[PROPOSAL]**: `enabled` defaults to all three targets, `default` to
   `"ts"`. `enabled` must not be empty, and `default` must be one of the enabled targets.
3. `[config.<module path>]` overrides `config` constants (decisions register, item 4)
   **[PROPOSAL]** for the format:
   - the table path is the module path, the key is the constant name;
   - the constant must exist (E0613), and the TOML value must have exactly the constant's type:
     integer for `Int`, float for `Float` (an integer is not accepted), boolean, string;
   - an `Int` override must fit into 64 bits;
   - an override replaces the initializer before constant evaluation, so other constants that use it
     see the new value;
   - both `pub` and private constants can be overridden.
4. `[extern.<target>.dependencies]` lists packages of the target language **[PLAN]**. Their meaning
   is in chapter 7.
5. Unknown tables or keys are an error (E0611), so typos are not silently ignored. **[PROPOSAL]**
6. `wolf` finds the project by looking for `wolfpack.toml` in the current directory and then in its
   parents. **[PROPOSAL]**

## 7. Project layout

```
my-app/
  wolfpack.toml
  src/
    main.wolf or lib.wolf
    users/
      model.wolf
  tests/
    storage_flow.wolf       module tests.storage_flow
  examples/
    quick_start.wolf        module examples.quick_start
  out/                      generated, not in git
```

1. The layout is the same for every project; the compiler refuses a different structure **[PLAN]**.
2. `src/` contains only `.wolf` files and directories. Any other file there is an error (E0612),
   except hidden files (names starting with `.`). **[PROPOSAL]** (see D6.4)
3. **`tests/`** holds integration tests **[PLAN]**. Each file is a module `tests.<path>`. It may
   contain only the sections `imports`, `functions` (helpers) and `tests`. It sees only `pub` names:
   for a `lib`, only the library API (it imports `lib`); for a `cli`, the `pub` names of all modules
   in `src/`. **[PROPOSAL]**
4. **`examples/`** holds usage examples **[PLAN]**. Each file is a module `examples.<path>` and a
   small program with its own `fn main`, seeing the same names as integration tests.
   `wolf run --example <name>` runs one (a new flag of `wolf run`, PLAN section 10).
   **[PROPOSAL]**
5. `out/` is generated and is not part of the source; `wolf new` adds it to `.gitignore`.
6. `wolf new <name> --kind cli|lib` creates `wolfpack.toml`, the entry file with a minimal valid
   program, empty `tests/` and `examples/`, and `.gitignore` **[PLAN]**.

## 8. Explicitly discarding a value

Chapter 4 (D4.3) makes discarding a `Result` an error unless it is "explicitly ignored", but no
syntax for that exists yet. Proposal: `let _ = expr` evaluates `expr` and drops the value; it is the
only use of `_` in `let`. **[PROPOSAL]** (see D6.5; changes the chapter 3 grammar).

## 9. Diagnostics

Proposed range E06xx (confirmed in chapter 9).

| Code | Error |
| --- | --- |
| E0601 | Name does not follow the naming style (chapter 1, section 3) |
| E0602 | Unknown module in an import |
| E0603 | Imported name does not exist or is not `pub` |
| E0604 | Import cycle (lists the cycle) |
| E0605 | Name declared or imported twice in a module |
| E0606 | Name declared twice in the same block |
| E0607 | Private item used outside its module |
| E0608 | Type that is less visible than the declaration using it |
| E0609 | Unknown name (with a "did you mean" suggestion) |
| E0610 | `module` line does not match the file path |
| E0611 | Invalid manifest: syntax, unknown key, wrong value |
| E0612 | Invalid layout: missing or extra entry file, index file, unexpected file, bad file name |
| E0613 | Config override for an unknown constant or with a wrong type |
| E0614 | Missing `main` or wrong signature |
| E0615 | Unused import |
| E0616 | Import of an entry module, or of the module itself |
| E0617 | Redeclaration of a prelude name |
| E0618 | Unused local variable (added by chapter 9; discard with `let _ = expr`) |

## 10. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D6.1 | Import forms: whole module, group of names, `as` aliases; no globs, no relative paths, no use without import | **answered**: yes |
| D6.2 | `pub` imports (re-exports) only in `src/lib.wolf` | **answered**: yes: one meaning for re-exports |
| D6.3 | An unused import is an error | **answered**: yes, as in Go: keeps dependencies honest |
| D6.4 | Strict layout: only `.wolf` files in `src/`; `tests/` and `examples/` as described | **answered**: yes |
| D6.5 | `let _ = expr` to discard a value explicitly (grammar change in chapter 3) | **answered**: yes |
| D6.6 | Manifest details: package name and version format, optional `[targets]` with defaults, `[config.<module>]` override format with exact types, unknown keys are errors | **answered**: yes |
| D6.7 | Rest: scopes and conflicts, no private types in public signatures, panic exit code 101, `assert` usable outside tests, prelude names cannot be redeclared, diagnostics | **answered**: yes |
