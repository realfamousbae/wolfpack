# 0001: `Int` is 64-bit everywhere; `bigint` in TypeScript

Status: accepted (author, 2026-10-07). Resolves open question 2 of `PLAN.md` section 14 and
**deviates from the plan's default** (which was `number` with an overflow check above 2^53 in debug).

## Decision

`Int` is a signed 64-bit integer on every target.

Overflow behaviour (open question 9) is **not decided by this record**. It is tracked in the
decisions register (`docs/spec/00-overview.md`), where the author accepted the plan default
"panic on every target".

| Target | Representation |
| --- | --- |
| Rust | `i64` |
| Go | `int64` |
| TypeScript | `bigint` |

## Reasons

- Identical semantics on all targets, including for large values. Differential tests (the same
  example gives the same output everywhere) stay meaningful.
- The default (`number` + debug check) would make behaviour above 2^53 depend on the target and on
  the build mode.

## Consequences

- The TS backend emits `bigint` arithmetic and must check the 64-bit range after operations to
  panic on overflow. Generated TS is less idiomatic and slower than with `number`; this is accepted.
- The TS backend needs an explicit rule for indexing and `.length`-like values (which are `number`
  in JS) when they cross into `Int`. To be fixed in the TS mapping chapter of the spec.
- `console.log(5n)` prints `5n`, so `std.io.print` for TS must stringify `bigint` itself.
- `Float` stays `number` in TS.
