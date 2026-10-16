#!/bin/sh
# Builds the compiler for the playground into playground/pkg/ (needs wasm-pack and the
# wasm32-unknown-unknown target of Rust). Serve the playground/ directory with any static server.
set -e
cd "$(dirname "$0")/.."
wasm-pack build crates/wolf_wasm --profile wasm --target web --out-dir ../../playground/pkg --out-name wolf_wasm --no-typescript --no-pack
