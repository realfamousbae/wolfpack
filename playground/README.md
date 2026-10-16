# The Wolfpack playground

A static page that compiles Wolfpack in the browser: edit `src/main.wolf`, see errors as you
type, run the program, and read the TypeScript, Go and Rust it compiles to. The compiler is the
crate `crates/wolf_wasm` compiled to WebAssembly; programs run as their generated TypeScript in a
worker (ADR 0012).

## Building and serving

```bash
rustup target add wasm32-unknown-unknown
```

```bash
brew install wasm-pack
```

```bash
./playground/build.sh
```

```bash
python3 -m http.server 8642 --directory playground
```

Then open http://localhost:8642. The page needs network access once, to load the TypeScript
compiler for running programs.

The examples in `examples/` are checked by the tests of the compiler
(`crates/wolf_cli/tests/guide.rs`); add a new one there and to the list in `index.html`.
