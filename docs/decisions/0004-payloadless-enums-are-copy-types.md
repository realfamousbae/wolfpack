# 0004: `Void`, `Ordering` and enums without payloads are copy types

Status: accepted (author, 2026-10-07). Decision D5.1 of `docs/spec/05-ownership.md`.
**Deviates from `PLAN.md`** section 4, rule 3, where only `Int`, `Float` and `Bool` are copied
implicitly.

## Decision

Besides `Int`, `Float` and `Bool`, these types are copied implicitly on every use:

- `Void`;
- `Ordering`;
- every non-generic enum whose variants all have no payload (`enum Color { Red, Green }`).

## Reasons

- Such values are as cheap as an integer on every target (a tag, a string literal or a small
  number).
- Moving them would be surprising: after `let c = color`, `color` would be unusable.
- In Rust these enums get `#[derive(Clone, Copy)]`, which is idiomatic.

## Consequences

- Adding a payload to one variant of an enum turns it into a move type. Code that relied on
  implicit copies then gets ownership errors. This is accepted: the error messages point to the
  exact uses.
- `Option<Int>` and other generic enums stay move types.
