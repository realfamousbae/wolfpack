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
- a `main` section (chapter 2, section 3.7).

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
   `examples` are reserved for the directories of the same names (section 7), and `native` for
   native modules (section 1a; 0.2): `src/native.wolf` and `src/native/` are E0612.

## 1a. Native modules

Added in 0.2 (author, 2026-10-20, D6.9).

1. A module path `native.<path>` names a **native module**: code of the project in a target
   language under `native/<target>/` (section 7, rule 7). `native.text.slug` is the TS file
   `native/ts/text/slug.ts`, the Go package `native/go/text/slug/` and the Rust module
   `native/rust/text/slug.rs` (or `native/rust/text/slug/mod.rs`), whichever exist.
2. A native module is imported like any module, in both forms of section 2.1:
   `imports { native.slug }` and then `slug.slugify(s)`, or `imports { native.slug.{slugify} }`.
   An import of a native module that exists for no target is E0602.
3. Its names are not declared in wolfpack: the compiler reads them from the native files
   (chapter 7, section 8.7). A name of a native module used in a module binds that module to the
   targets whose files declare the name, as an extern declaration of only these targets does
   (chapter 7, section 8.5, E0704).
4. A native module cannot be re-exported with a `pub` import (E0712).

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
8. An **unused import is an error** (E0615), as in Go. **[PROPOSAL]** (see D6.3) A `pub` import in
   `src/lib.wolf` is always used: it exports the names.

### 2.2 Re-exports

A `pub` import makes the imported names part of the importing module's own public names.
Re-exports are allowed **only in `src/lib.wolf`**, where they build the library API (author;
restriction to `lib.wolf` is **[PROPOSAL]**, see D6.2); a `pub` import anywhere else is an error
(E0620):

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
5. **Names introduced by a construct share the scope of its top block** (author, 2026-10-08): the
   variable of `for x in xs { ... }`, the bindings of a `match` arm pattern and the parameters of a
   closure are in the same scope as the first level of the body. So `for x in xs { let x = 1 }`,
   `Some(x) => { let x = 1 }` and `|x| { let x = 1 }` are errors (E0606), exactly like a parameter
   and a `let` in the top block of a function.
6. **Unused local variables** (E0618, chapter 9) (author, 2026-10-08): the rule applies to `let`
   and `var` bindings and to bindings in patterns, where `_` is available. The variable of a `for`
   loop is exempt, because the grammar requires a name there. Any read or write of a variable
   counts as a use: `left += days` uses `left`.
5. Prelude names cannot be declared again at the top level of a module (E0617). **[PROPOSAL]**

### 3.3 The prelude

Available in every module without imports (author):

- the built-in types `Int`, `Float`, `Bool`, `String`, `Void`, `List`, `Map`, `Option`, `Result`,
  `Ordering`;
- the variants `Some`, `None`, `Ok`, `Err`;
- the built-in interfaces `Equatable`, `Copyable`, `Hashable`, `Comparable`, `Display` (0.2), and
  the error interface `Error` (chapter 7);
- the functions `print` (defined in `std.io`, re-exported by the prelude), `panic`, `assert`,
  `assert_eq` (0.2), `error` (0.2) and `replace` (chapter 7).

The names added in 0.2 cannot be declared again at the top level (E0617), so a 0.1 program with a
top-level `fn error` or `type Display` must rename it. Local variables named `error` are fine: they
shadow the prelude function (section 3.2).

**[DRAFT]** (2026-10-26, chapter 4, D4.23): the built-in type `Char` joins the prelude. Like the
names of 0.2 it cannot be declared again at the top level (E0617), so a program with a top-level
type `Char` must rename it.

**[DRAFT]** (2026-10-26, chapter 4, D4.24): so do the sized integer types `Int8`, `Int16`, `Int32`,
`UInt8`, `UInt16` and `UInt32`, under the same rule.

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
   **[PROPOSAL]** The same holds for a `pub` interface: its method signatures and its
   super-interfaces (chapter 4, section 8.2, rule 6) may only name visible types and interfaces, so a
   `pub` interface cannot extend a private one (author, 2026-10-25). In a library the
   super-interfaces of an exported interface are part of the API (rule 6).
6. **Library API.** For kind `lib`, code outside the project sees only the `pub` names of
   `src/lib.wolf` and its `pub` imports (author). Every type that appears in a signature of that
   API must itself be part of the API (E0608). Inside the project, `pub` means "visible to the other
   modules of this project".

## 5. The entry point

1. Kind `cli`: `src/main.wolf` must have a `main` section (author, 2026-10-17; it replaces
   `fn main` in `functions`). Its header may carry the parameter `args: List<String>` and the
   result type `Result<Void, Error>`, each optional (chapter 2, section 3.7). Any other parameter
   or result type is an error (E0614). A missing `main` is E0614 too.
2. When `main` returns `Err(e)`, the program writes the error to standard error and exits with code
   1. When it returns normally, the exit code is 0. A panic exits with code 101 **[PROPOSAL]** (the
   Rust convention; chapter 8 makes every target do the same).
3. Command-line arguments come to `main` as its optional parameter (the same list as
   `std.io.args()`); an explicit exit code comes from `std.process` (chapter 7).
4. Kind `lib`: `src/lib.wolf` has no `main` section.

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
  native/                   your own target code (0.2, rule 7)
    ts/  go/  rust/
  out/                      generated, not in git
```

1. The layout is the same for every project; the compiler refuses a different structure **[PLAN]**.
2. `src/` contains only `.wolf` files and directories. Any other file there is an error (E0612),
   except hidden files (names starting with `.`). **[PROPOSAL]** (see D6.4)
3. **`tests/`** holds integration tests **[PLAN]**. Each file is a module `tests.<path>`. It may
   contain only the sections `imports`, `functions` (helpers) and `tests`; any other section is an
   error (E0619). It sees only `pub` names:
   for a `lib`, only the library API (it imports `lib`); for a `cli`, the `pub` names of all modules
   in `src/`. **[PROPOSAL]**
4. **`examples/`** holds usage examples **[PLAN]**. Each file is a module `examples.<path>` and a
   small program with its own `main` section, seeing the same names as integration tests.
   `wolf run --example <name>` runs one (a new flag of `wolf run`, PLAN section 10).
   **[PROPOSAL]**
5. `out/` is generated and is not part of the source; `wolf new` adds it to `.gitignore`.
6. `wolf new <name> --kind cli|lib` creates `wolfpack.toml`, the entry file with a minimal valid
   program, empty `tests/` and `examples/`, and `.gitignore` **[PLAN]**.
7. **`native/`** (0.2, author, 2026-10-18) holds code of the project written in a target language,
   imported as a native module (section 1a; 0.2) or called through `extern` with a `native:` path
   (chapter 7, section 8.2a): `native/ts/` holds `.ts` files, `native/go/` Go packages (one
   directory per package), `native/rust/` `.rs` files. Each subdirectory is optional. `native/`
   may contain no other directories (E0612). The compiler reads only the declarations of the
   files of imported native modules (chapter 7, section 8.7), and copies every file into the
   generated project of its target (chapter 8, section 8a).

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
| E0612 | Invalid layout: missing or extra entry file, index file, unexpected file, bad file name. When the problem has no place in a source file (an unexpected file such as `src/notes.txt`), the diagnostic has no label and names the path in its message (author, 2026-10-08) |
| E0613 | Config override for an unknown constant or with a wrong type |
| E0614 | Missing `main` section or a wrong parameter or result type in its header |
| E0615 | Unused import |
| E0616 | Import of an entry module, or of the module itself |
| E0617 | Redeclaration of a prelude name |
| E0618 | Unused local variable (added by chapter 9; discard with `let _ = expr`) |
| E0619 | Section not allowed in a module of `tests/` (added in M3) |
| E0620 | `pub` import outside `src/lib.wolf` (added in M3) |
| E0712 | `pub` import of a native module (0.2) |

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
| D6.8 | 0.2: the directory `native/` with `ts/`, `go/` and `rust/` (section 7, rule 7) | **answered** (author, 2026-10-18): yes |
| D6.9 | 0.2: native modules `native.<path>`, imported like modules; the root segment `native` reserved in `src/`; no re-export (section 1a, E0712) | **answered** (author, 2026-10-20): yes |
