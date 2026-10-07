# 0003: `Map` iterates in insertion order on every target

Status: accepted (author, 2026-10-07). Decision D4.1 of `docs/spec/04-types.md`.
**Deviates from `PLAN.md`** section 8, which maps `Map<K, V>` to `HashMap<K, V>` in Rust and
`map[K]V` in Go.

## Decision

Walking a `Map` (`m.keys()`, `m.values()` and similar std functions) yields entries in the order in
which their keys were first inserted, on every target.

| Target | Representation |
| --- | --- |
| TypeScript | `Map<K, V>` (already insertion-ordered) |
| Rust | an insertion-ordered map, for example `indexmap::IndexMap<K, V>` |
| Go | a generated generic type holding a `map[K]V` and a slice of keys |

## Reasons

- Go and Rust randomize hash map iteration. Without a fixed order, the same program prints
  different output on different targets and runs, and differential tests cannot work.
- Insertion order is what TS gives natively and what users of Python and JS expect.
- Sorted order (like `BTreeMap`) was the alternative; it was rejected because it costs a sort on
  every walk in Go and TS.

## Consequences

- The generated Rust project depends on an extra crate (or a small generated type).
- Generated Go code for maps is less idiomatic than a plain `map[K]V`.
- Removing a key in the Go representation must also update the key slice (O(n), or a tombstone
  scheme); this is a backend detail.
