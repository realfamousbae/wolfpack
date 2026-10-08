# 0006: M4 type-checks every example without user generics

Status: accepted (author, 2026-10-09). **Deviates from `PLAN.md`** section 11, which lists
`impl` and closures under M5.

## Decision

1. M4 checks the seven examples without user-declared generics (`hello`, `fizzbuzz`,
   `calculator`, `word_count`, `todo`, `binary_tree`, `bank`). For that it includes:
   - calls of generic std functions and methods (`List<T>.map`, `list.sort_by`), with type
     arguments inferred;
   - closures;
   - inherent `impl` blocks and their methods;
   - implementations of interfaces such as `CalcError: Error`, interface types as values where
     the examples need them, and the coercion of section 4.8.5 (also for `?`);
   - evaluation of `config` constants, cycles of aliases and constants (E0419) and the types of
     overrides in `wolfpack.toml`.
2. M5 keeps: checking the bodies of user-declared generic functions, types and `impl` blocks,
   bounds on user generics (E0404), full checking of interface implementations (E0413), object
   safety (E0416, E0417) and the remaining examples (`shapes`, `generic_stack`, `textkit`).

## Reasons

The examples without user generics still use built-in generic types, closures, methods and the
`Error` interface. Without them only `hello` and `fizzbuzz` would pass, while the M4 criterion
needs at least four examples.
