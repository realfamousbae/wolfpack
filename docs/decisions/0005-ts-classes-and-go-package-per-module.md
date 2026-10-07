# 0005: TS classes for structs, one Go package per module, generated runtime

Status: accepted (author, 2026-10-07). Decisions for `docs/spec/08-targets.md`.
**Deviates from `PLAN.md`** section 8 (TS `struct` as "interface and constructor function") and
refines section 6 (Go "packages by folders").

## Decision

1. **TypeScript**: a Wolfpack `struct` becomes a TS `class`; each variant of an enum with payloads
   becomes a class with a `kind` field, and the enum is the union of these classes.
2. **Go**: every Wolfpack module becomes its own Go package in its own directory:
   `src/users/storage.wolf` becomes `out/go/users/storage/storage.go`, package `storage`.
3. **Runtime**: support code (checked arithmetic, ordered map, float formatting, panics, std
   intrinsics) is generated into each output project instead of being published as packages.

## Reasons

1. Values of an interface type (`List<Shape>`) need dynamic method calls. Plain objects with
   module-level functions have no methods, so each coercion would need a wrapper object. Classes
   give methods, `implements` and readable code.
2. Two Wolfpack modules in one directory may have private names that are equal. In one Go package
   they would collide. A package per module also makes Wolfpack `pub` match Go export exactly.
3. A generated runtime keeps every output project self-contained: no network access, no published
   packages to keep in sync, and the runtime always matches the compiler version.

## Consequences

- Go output has one more directory level than `src/`.
- TS output uses `new User({ ... })` instead of object literals.
- Each output project carries its own copy of the runtime (only the parts it uses).
