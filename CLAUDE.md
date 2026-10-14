# Wolfpack: working rules

Wolfpack is a statically typed language whose compiler (`wolf`, written in Rust) translates `.wolf`
files into readable TypeScript, Go and Rust. The full plan is in `PLAN.md` (Russian, authoritative
for scope and milestones). This file holds the working rules for Claude Code.

## Language of the repository

Everything in the repository is written in **English**: the spec, docs, README, code, comments,
commit messages and compiler diagnostics. `PLAN.md` stays in Russian as the author's original plan.

## Order of work

1. Work on one milestone at a time (see section 11 of `PLAN.md`). Do not jump ahead.
2. **M11 is current.** Approved: M0 (specification) 2026-10-07, M1 (lexer) and M2 (parser)
   2026-10-08, M3 (modules and name resolution), M4 and M5 (types), M6 (IR and TypeScript), M7
   2026-10-09, M8 (ownership) and M9 (Go) 2026-10-13, M10 (Rust) 2026-10-14.
3. M0 is done chapter by chapter: draft one chapter of `docs/spec/`, the author reviews, then the next.
4. At the end of a milestone show the result: what works, which tests were added, what was deferred.
5. After each milestone briefly explain to the author how the result is built and why. Learning is
   one of the goals of the project.

## The specification is the source of truth

- Any change to the language starts with an edit of `docs/spec/` in a separate commit and needs the
  author's agreement.
- Do not add language features on your own initiative. Write ideas into `docs/ideas.md`.
- If the spec is incomplete or contradictory, stop and ask. Do not choose silently.
- Internal compiler design is yours to decide. Language behaviour is decided only with the author.
- Decisions that deviate from `PLAN.md` defaults are recorded in `docs/decisions/` (short ADRs).

## Code quality

- No `unsafe`. No `unwrap`, `expect` or `panic!` on any path that depends on user input.
- The compiler never panics on user input: every invalid file yields a diagnostic.
- Every diagnostic has a unique code (`E0101`), a source range and a test. Codes are listed in one
  file and never reused. Driver errors (E00xx) may have no source range.
- Backends read only the IR. Each backend builds a target-language tree and then prints it; direct
  string concatenation from IR is forbidden.
- Before every commit run `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- Commits are small and follow Conventional Commits.
- Public types and functions of crates are documented.

## Git

- Commits and pull requests are authored by the user only. Never add `Co-Authored-By`, "Generated
  with", or any other attribution to Claude in commits, PRs, issues or contributor lists.
- The author granted standing permission (2026-10-07) to commit and push to the private GitHub
  repository `Wolfpack` (`origin`, branch `main`). One logical change per commit; spec edits in
  their own commits. Do not change repository visibility, and do not push anywhere else.
- Never put personal or sensitive data of the user into files or commits: this repository is
  planned to be public.

## Repository documents

- `docs/spec/`: language specification.
- `docs/decisions/`: short records of decisions and their reasons.
- `docs/ideas.md`: deferred ideas.
- `CHANGELOG.md`: updated at the end of each milestone.
