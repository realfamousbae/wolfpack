# Examples

Ten example projects written against the specification in `docs/spec/`. They are the acceptance
tests of the milestones in `PLAN.md`: every example must pass `wolf check` (M5), run on every target
and print exactly `expected.txt` (M6, M9, M10), and run its `tests` sections (M7).

`expected.txt` is the standard output of `wolf run` with exit code 0. For the library `textkit` it
is the output of `wolf run --example demo`.

| Example | Kind | What it covers | Generics | Tests |
| --- | --- | --- | --- | --- |
| `hello` | cli | the smallest program, `print` | no | no |
| `fizzbuzz` | cli | ranges, `match` with alternatives, `to_string` | no | yes |
| `calculator` | cli | enums with data, `Result`, `?`, a user `Error` type, `main` returning `Result` | no | yes |
| `word_count` | cli | string methods, `config`, `Map` in insertion order, closures, `list.sort_by` | no | no |
| `todo` | cli | three modules, `pub` fields and methods, `mut self`, `for x in mut`, `match mut` | no | no |
| `shapes` | cli | an interface as a value type, `Comparable`, sorting | uses `list.max` | no |
| `generic_stack` | cli | a generic struct, a generic method, bounds | yes | yes |
| `binary_tree` | cli | a recursive enum, `own self`, `match own` | no | yes |
| `bank` | cli | `mut` and `own` parameters, moves, errors | no | yes |
| `textkit` | lib | re-exports in `lib.wolf`, `extern` for all three targets, `tests/`, `examples/` | no | yes |

Coverage of the matrix in `docs/spec/00-overview.md`, section 4:

| Criterion | Examples |
| --- | --- |
| no generics (M4), at least 4 | hello, fizzbuzz, calculator, word_count, todo, binary_tree, bank |
| several modules (M3) | todo, textkit |
| `tests` sections (M7), at least 3 | fizzbuzz, calculator, generic_stack, binary_tree, bank, textkit |
| ownership (M8), at least 2 | todo, bank, binary_tree |
| generics, interfaces, closures (M5), at least 3 | word_count, shapes, generic_stack |
| `extern` for all three targets (M11) | textkit |
| kind `lib` (M3) | textkit |
