# 0012: Choices of the playground (M12)

Status: proposed. Item 1 was decided by the author (2026-10-16); the other items are [INTERNAL]
choices recorded so that the author can object.

## Decision

1. **A static site in `playground/`**, published on GitHub Pages once the repository is public.
2. **The compiler runs in WebAssembly.** The crate `wolf_wasm` wraps the in-memory pipeline of
   `wolf_driver` (shared with `wolf_cli`) and exposes `check`, `build`, `format`, `explain` and
   `version`, each taking the source and returning JSON. `playground/build.sh` builds it with
   `wasm-pack` into `playground/pkg/`, which is not committed.
3. **One file**: the playground edits `src/main.wolf` of a `cli` project with the default manifest.
4. **Programs run as the generated TypeScript.** The page loads the TypeScript compiler from a CDN
   (pinned version) on the first run, transpiles the generated files, rewrites their relative
   imports to blob URLs and runs them in a module worker with a small `process` object that
   collects standard output and error. A run is stopped after 5 seconds. `std.fs`, input and the
   environment are not available.
5. **Go and Rust are shown, not run**: their generated files are displayed in their tabs.
6. **The source can be shared** as a link with the program in the URL fragment (base64 of UTF-8),
   and is remembered in local storage.
7. **The WebAssembly stack is 16 MiB** (`.cargo/config.toml`), so that input nested up to the
   parser's limit gives E0314 instead of a crash.

## Reasons

2. One pipeline for the command line and the browser keeps their diagnostics identical; JSON
   strings keep the bindings trivial.
4. Running the generated code, not an interpreter of the IR, shows exactly what the TypeScript
   backend produces, and a worker keeps an endless loop from freezing the page.
5. Go and Rust have no practical in-browser toolchain.
