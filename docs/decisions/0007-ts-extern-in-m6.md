# 0007: `extern` for TypeScript in M6

Status: accepted (author, 2026-10-09). **Deviates from `PLAN.md`** section 11, which puts `extern`
into M11.

## Decision

M6 implements calls of `extern` functions for the TypeScript target only: the import or the
prototype method named by the `from` string, and the boundary conversions of chapter 8, section 8.
Dependencies from `[extern.ts.dependencies]` and the Go and Rust targets stay in M11.

## Reasons

The M6 criterion is that every example runs in Node. `textkit` declares an `extern` function for
each target, so without TypeScript `extern` it could not run.
