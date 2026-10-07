# 0002: Hybrid representation of `Option` and `Result` in TypeScript and Go

Status: accepted (author, 2026-10-07). Resolves gap G4 of `docs/spec/00-overview.md`.
**Deviates from `PLAN.md`** section 8 (which maps `Option<T>` to `T | undefined` and `*T`, and
`Result<T, E>` to `(T, error)` in Go, in every position) and refines plan default 7 of section 14.

## Decision

- **Function results**, where the idiomatic form is unambiguous, use the idiomatic form:
  `(T, error)` in Go, `T | undefined` in TS.
- **Data positions** (struct fields, collection elements, generic arguments, nesting such as
  `Option<Option<T>>`, `Option<Void>`) use a tagged value in TS and Go.
- Rust always uses the native `Option<T>` and `Result<T, E>`.

The exact boundary between "function result" and "data", and the shape of the tagged values, are
specified in chapter 8.

## Reasons

- `T | undefined` cannot represent `Option<Option<T>>` or `Option<Void>`.
- `(T, error)` is not a value in Go: it cannot be stored in a `List`, a field or passed to a
  generic function.
- Using tagged values everywhere would work, but would make the most common code (functions that
  return `Option` or `Result`) much less idiomatic. Readable output is a project goal.

## Consequences

- The TS and Go backends convert between the two forms at the boundary (for example when a function
  result is stored in a list). This conversion is generated, never written by the user.
- Backends become more complex; this is accepted.
