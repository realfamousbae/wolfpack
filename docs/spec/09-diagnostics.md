# Chapter 9: Diagnostics

Status: **APPROVED** (author, 2026-10-07). Every **[PROPOSAL]** item in this chapter is accepted.

This chapter fixes the code ranges, the policy and the style of compiler diagnostics. The codes
themselves are listed in one file in the code (`wolf_diag`) **[PLAN]**; the tables in chapters 1–7
are the initial set.

Decisions taken by the author for this chapter (2026-10-07):

- **Only errors, no warnings** in v1.
- Output is human-readable text, and JSON with `--message-format json`.

## 1. Codes

1. A code is `E` followed by four digits. Each code has exactly one meaning, a source range and a test
   **[PLAN]**. Exception: driver errors (E00xx) may have no source range, because they are not about
   a place in a source file (author, 2026-10-08). The same holds for layout errors (E0612) about a
   file that has no source to point at, such as an unexpected `src/notes.txt`. A code is never reused, even after the error it named is removed; removed codes stay
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
   E0003 internal compiler error. **[PROPOSAL]** Added in M3: E0004 no `wolfpack.toml` found in the
   current directory or its parents. Backend and toolchain errors (E08xx, chapter 8, section 9) have no source
   range either.
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
3. Every error except a driver error (E00xx) has a **primary label** on the exact range. Related places get **secondary labels**:
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
      = help: pass a copy instead: `archive(user.copy())`
   ```

6. After the diagnostics, human output ends with the line `error: aborting due to N errors`
   (`1 error` for one). JSON output has no such line. (Author, 2026-10-08.)
7. `--message-format json` prints one JSON object per diagnostic and line on standard error. The
   schema (defined in milestone M1, kept stable after v0.1):

   ```json
   {
     "code": "E0107",
     "severity": "error",
     "message": "malformed number literal `007`",
     "labels": [
       {
         "file": "src/main.wolf",
         "start": 59, "end": 62,
         "line_start": 5, "column_start": 19,
         "line_end": 5, "column_end": 22,
         "primary": true,
         "message": "leading zeros are not allowed; octal numbers are written `0o7`"
       }
     ],
     "help": [],
     "notes": []
   }
   ```

   `start` and `end` are byte offsets (end exclusive); lines and columns are one-based and columns
   count Unicode scalar values. `severity` is always `"error"` in v1. `labels` may be empty (for
   example for E0001). The object is printed on a single line.
8. `wolf explain <code>` shows a longer description with a wrong and a fixed example **[PLAN]**
   (section 10, milestone M12). Each code gets this text when it is introduced.
   - The code is written as in the output (`E0501`); the letter may be lower case (`e0501`).
   - It works anywhere: no project is needed and `wolfpack.toml` is not searched for.
   - The text is printed to standard output as written, in Markdown: a heading with the code and
     its title, one or more paragraphs on the rule and the usual fix, then `Wrong:` and `Fixed:`
     with a complete example each. Driver and tool errors (E00xx, E08xx) may show a command or a
     manifest instead of `.wolf` code.
   - A code that does not exist is a usage error (E0002, exit code 2). A retired code prints that
     it is no longer used and exits with 0.
   - Diagnostics do not mention `wolf explain`; the output of section 3 stays as it is.

   The rules of `wolf explain` were accepted by the author on 2026-10-16.

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
| D9.1 | Code ranges and driver codes E0001–E0003, internal compiler error handling | **answered**: yes |
| D9.2 | Suppression of follow-up errors, sorted output | **answered**: yes |
| D9.3 | Message style (section 3) and the JSON format defined in M1 | **answered**: yes |
| D9.4 | Exit codes of `wolf` (section 4) | **answered**: yes |
