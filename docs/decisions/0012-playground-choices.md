# 0012: Choices of the playground (M12)

Status: accepted (author, 2026-10-16). Item 1 was decided by the author; the other items are
[INTERNAL] choices recorded so that the author can object.

## Decision

1. **A static site in `playground/`**, published on GitHub Pages once the repository is public.
2. **The compiler runs in WebAssembly.** The crate `wolf_wasm` wraps the in-memory pipeline of
   `wolf_driver` (shared with `wolf_cli`) and exposes `check`, `build`, `format`, `explain` and
   `version`, each taking the source and returning JSON. `playground/build.sh` builds it with
   `wasm-pack` into `playground/pkg/`, which is not committed.
3. **One file**: the playground edits `src/main.wolf` of a `cli` project with the default manifest.
4. **Programs run in the VM of the compiler** (amended 2026-10-23, at the author's request; until
   then they ran as the generated TypeScript, transpiled by a TypeScript compiler loaded from a
   CDN). `wolf_wasm::run_live` checks the program and runs it in `wolf_vm` (spec chapter 10) in a
   module worker (`playground/worker.js`), which posts the output as the program writes it; the
   page stops a run after 5 seconds and shows at most 1 MiB of output. The box *Input* gives the
   lines that `io.read_line()` reads; files, arguments and the environment are not available, and
   code of the targets cannot run (E0807).
5. **Go and Rust are shown, not run**: their generated files are displayed in their tabs.
6. **The source can be shared** as a link with the program in the URL fragment (base64 of UTF-8),
   and is remembered in local storage.
7. **The WebAssembly stack is 16 MiB** (`.cargo/config.toml`), so that input nested up to the
   parser's limit gives E0314 instead of a crash.

## Reasons

2. One pipeline for the command line and the browser keeps their diagnostics identical; JSON
   strings keep the bindings trivial.
4. The VM gives the output, panics and exit codes of every target (chapter 10) without loading
   anything from the network, starts at once, and reads input; a worker keeps an endless loop
   from freezing the page. The TypeScript, Go and Rust a program becomes are still shown in their
   tabs.
5. Go and Rust have no practical in-browser toolchain.
