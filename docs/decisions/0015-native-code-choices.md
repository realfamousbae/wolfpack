# 0015: Choices of native code and the wider `extern` boundary (0.2)

Status: accepted (author, 2026-10-18 to 2026-10-21). The language rules are in chapter 6,
section 1a, chapter 7, sections 8.2a to 8.7, and chapter 8, sections 8 to 8b (D6.9, D7.12 to
D7.20, D8.8 to D8.13). The items below are the [INTERNAL] choices of the implementation, recorded
so that the author can object, with what each conversion costs at run time.

## Decision

1. **Conversions live in the extern wrappers** (ADR 0011, item 6), never in user code. A
   conversion that the value does not need is not generated: a `List<String>` crosses as it is.
2. **What each boundary type costs on every call:**

   | Value | TS | Go | Rust |
   | --- | --- | --- | --- |
   | `Int` | a check to `number`, or nothing for `bigint` | nothing for `int64`, a check for other integer types | a check (`to_target_int`, `TryFrom`) |
   | `List<T>` | nothing; `.map` when the elements convert; `.slice()` for a `readonly` result | nothing; `ConvertList` when the elements convert | `&[T]` as it is; a copy into `Vec` for `Vec<T>` or converted elements |
   | `Map<K, V>` | a new `Map`, and from extern code a sort by key | a new `map[K]V`, and a sort by key back | a new `HashMap` or `BTreeMap`, and a sort by key back |
   | record | a plain object, field by field | a value of the struct type, field by field | a value of the struct type, field by field |
   | callback | an arrow function that converts | the function value as it is | a closure that converts, by reference |
   | `Option<T>` parameter | `T \| undefined` | a pointer to a copy | `Option<&T>`; a copy first for records and maps |

3. **Rust converts through traits of the runtime**: `ToExternAs<T>` (a wolfpack value as the Rust
   type the call takes, inferred from a typed `let`) and `FromExtern<T>` (back, inferred from the
   wrapper's result type). Nested generic closures broke rustfmt, and the traits let lists, maps,
   `Option` and records nest without any code per shape. Each record gets two generated `impl`s.
4. **Go and TypeScript convert with small generic helpers** of the runtime (`ConvertList`,
   `ToExternMap`, `FromExternMap`, `ConvertPtr`, `OptionToPtr`, `toExternMap`, `fromNullable`),
   and a call whose result is a record is bound to a variable first, so that it runs once.
5. **A native module is a generated wolfpack module.** The loader of `wolf_cli` reads the files
   with the crate `wolf_native` (Rust with `syn`, Go with a small parser of declarations written
   for `wolf`, TypeScript with `oxc_parser`), writes an `extern` block with `native:` paths and a
   `types` section, and parses it like any module. The types the reader found travel beside it
   (`NativeSig`, `NativeTy`, and the field types of records), so that the backends call the target
   with its own integer and float types. Name resolution, types and ownership did not change.
6. **The `number` rule runs before the agreement of targets**: TypeScript `Float`s become `Int`
   where another target has an integer at the same place, then the signatures are compared.
7. **Methods of opaque types are wrappers that disappear.** The generated module declares a private
   extern function per method and an `impl` block whose methods call it; the IR replaces each call
   of such a method by a call of its function, with the receiver first, so the backends never see
   methods of extern types. Written `impl` blocks of extern types are E0707.
8. **`oxc_parser` 0.75**, the last version whose `rust-version` is 1.85, the version that
   `wolfpack-lang` promises. It makes the binary larger; the playground does not use it.
9. **`wolf check --native` prints the generated module**, without the private method functions and
   with comments for methods and for what is not available, so that it can be pasted.

## Reasons

1. Copies at the boundary are the price of values that wolfpack owns: extern code can neither keep
   nor change them, which the ownership rules need.
2. One generated module keeps the readers small and every later check (types, ownership,
   targets) the same for written and read declarations.
3. Disappearing wrappers give opaque types methods without a new kind of method in the IR and the
   three backends.

## Costs

- A `Map` or a list of records crosses in time linear in its size, on every call.
- Three readers of three languages to keep up with what people write; what they cannot read stays
  reachable through written `extern` blocks.
