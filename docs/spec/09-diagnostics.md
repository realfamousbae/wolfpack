# Chapter 9: Diagnostics

Status: **DRAFT, awaiting author review.**

This chapter fixes the code ranges, the policy and the style of compiler diagnostics. The codes
themselves are listed in one file in the code (`wolf_diag`) **[PLAN]**; the tables in chapters 1–7
are the initial set.

Decisions taken by the author for this chapter (2026-10-07):

- **Only errors, no warnings** in v1.
- Output is human-readable text, and JSON with `--message-format json`.

## 1. Codes

1. A code is `E` followed by four digits. Each code has exactly one meaning, a source range and a test
   **[PLAN]**. A code is never reused, even after the error it named is removed; removed codes stay
   listed as retired. **[PLAN]**
2. Ranges:

   | Range | Area | Chapter |
   | --- | --- | --- |
   | E00xx | driver: reading files, command-line usage, internal compiler errors | 9 |
   | E01xx | lexical | 1 |
   | E02xx | file skeleton | 2 |
   | E03xx | parser | 3 |
   | E04xx | types | 4 |
   | E05xx | ownership | 5 |
   | E06xx | names, modules, manifest, layout | 6 |
   | E07xx | std and `extern` | 7 |
   | E08xx | backends and target toolchains (for example a missing `node` for `wolf run`) | 8 |
   | E09xx | reserved | |

3. Initial driver codes: E0001 cannot read a file or directory; E0002 invalid command-line usage;
   E0003 internal compiler error. **[PROPOSAL]**
4. **Internal compiler error.** The compiler never panics on user input **[PLAN]**. If an internal
   invariant breaks anyway, the driver catches it and reports E0003 with a request to file a bug,
   instead of a Rust panic trace. **[PROPOSAL]**

## 2. Errors only

1. Every diagnostic is an error: code either compiles or it does not (author). There are no warnings
   and no lint levels in v1.
2. As a consequence of this policy (author):
   - an unused import is an error (E0615, chapter 6);
   - an **unused local variable** is an error (E0618, added to chapter 6). A value that is computed
     only for its effect is discarded with `let _ = expr` (chapter 6, section 8);
   - unused parameters, unused private functions and unused private types are **not** errors.
3. The compiler does not stop at the first error **[PLAN]**: it reports every error it can find. An
   error in an earlier phase may hide later phases for the affected declarations, but not for
   unrelated ones. Duplicate follow-up errors caused by an earlier one are suppressed. **[PROPOSAL]**
4. Errors are sorted by file path, then by position, so the output is deterministic. **[PROPOSAL]**

## 3. Message style

**[PROPOSAL]** for the whole section.

1. Messages are English, start with a lower-case letter and have no final period:
   `` use of moved value `user` ``.
2. Code is in backticks. Types are printed as in source: `List<User>`.
3. Every error has a **primary label** on the exact range. Related places get **secondary labels**:
   "value moved here", "first declared here".
4. Where a fix is known, a `help:` line shows it, if possible as the changed source line.
   A `note:` line explains a rule when the reason is not obvious.
5. Human output looks like this (colours only when writing to a terminal):

   ```
   error[E0501]: use of moved value `user`
     --> src/main.wolf:12:11
      |
   11 |     let old = archive(own user)
      |                       -------- value moved here
   12 |     print(user.name)
      |           ^^^^ value used here after the move
      |
      = help: pass a copy instead: `archive(own user.copy())`
   ```

6. `--message-format json` prints one JSON object per diagnostic and line, with the code, message,
   severity, labels (file, byte and line/column ranges, text), help and notes. The schema is defined
   with milestone M1 and kept stable after v0.1.
7. `wolf explain <code>` shows a longer description with a wrong and a fixed example **[PLAN]**
   (section 10, milestone M12). Each code gets this text when it is introduced.

## 4. Exit codes of `wolf`

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | the project has errors (any E01xx–E08xx diagnostic) |
| 2 | invalid usage of `wolf` itself (E0002) |
| 3 | internal compiler error (E0003) |

`wolf run` and `wolf test` return the exit code of the program or test run when compilation
succeeded. **[PROPOSAL]**

## 5. Tests

1. Every code has at least one UI test: a `.wolf` file in `tests/ui/` and a `.stderr` file with the
   exact expected output **[PLAN]** (section 12).
2. Ownership rules have a pair of tests each: valid code and code with the error **[PLAN]** (M8).
3. The lexer and the parser are fuzzed and must never panic **[PLAN]**.

## 6. Decisions for the author

| # | Decision | Recommendation |
| --- | --- | --- |
| D9.1 | Code ranges and driver codes E0001–E0003, internal compiler error handling | yes |
| D9.2 | Suppression of follow-up errors, sorted output | yes |
| D9.3 | Message style (section 3) and the JSON format defined in M1 | yes |
| D9.4 | Exit codes of `wolf` (section 4) | yes |
