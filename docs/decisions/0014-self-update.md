# 0014: `wolf self update` and the daily question

Status: accepted (author, 2026-10-18, D8.6 and D8.7). Items 1 to 3 were decided by the author and are in
chapter 8, section 2b; the other items are [INTERNAL] choices recorded so that the author can
object.

## Decision

1. **`wolf self update [--version x.y.z]` and `--check`.** The command replaces the running `wolf`
   with the latest stable release `v*` of GitHub, or with the given one, also an older one.
   `--check` only prints both versions and exits with 0 (up to date) or 1 (newer release).
2. **The daily question.** At most once a day a background check asks GitHub for the latest
   release; the next command in a terminal asks `Update now? [y/N]` on standard error. Never in a
   pipe, with `CI`, with `WOLFPACK_NO_UPDATE_CHECK=1`, with `update.check = false`, or for
   `wolf self`, `--version`, `--help` and `explain`. `y` updates and runs the command again with
   the new `wolf`; anything else runs it as it is.
3. **What is not replaced.** A `wolf` under `~/.cargo/bin` or a Homebrew prefix names the command
   of its package manager and changes nothing. A failed update is E0805.
4. **The latest release is read from a redirect**: `github.com/<repo>/releases/latest` answers with
   the tag in `Location`, which has no rate limit of the API of 60 requests an hour.
5. **The background check is a detached child process**, `wolf self update --refresh-cache`, with
   its output thrown away: the command that runs never waits for the network. The answer is kept
   in `<home>/update-check.json` (`checked`, `latest`, `asked_version`, `asked_at`).
6. **The replacement follows the installers.** The archive and `SHA256SUMS.txt` are downloaded with
   the code of `wolf setup`, the sum is checked, the new `wolf` is unpacked next to the old one and
   must answer `--version`; only then it is renamed over the old file. On Windows the running
   `wolf.exe` becomes `wolf.old.exe`, which a later start removes. The new `wolf` then runs
   `wolf setup`.
7. **Tests.** Debug builds read releases from `WOLF_TEST_UPDATE_MIRROR`, so the end-to-end test
   serves a release from a local server. The release workflow tries the real path on clean
   machines: the `wolf` of the run, put in place by the terminal installer, updates itself to the
   oldest release of GitHub (`try-update`). It goes back in time because a release that knows
   `self update` cannot exist before the first one is published.

## Reasons

1. A compiler that is installed once and forgotten gets old quietly; the question makes new
   releases visible without a package manager, and never stops a script.
2. Replacing a running binary is the risky part: checking the sum and starting the new file before
   the rename means a failed update leaves a working `wolf`.
3. Package managers own their files; overwriting them would break their next upgrade.

## Costs

- One small request to GitHub a day, in the background, and a file in `<home>`.
- The real download is tested only by the release workflow, not by every push.
