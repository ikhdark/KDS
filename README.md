# KDS

KDS turns long build and test output into a short, useful summary.

It helps you and AI coding tools see what failed, which files matter, and what
to check next without dumping hundreds of log lines into the chat.

It can wrap a command or summarize existing log text from stdin or a file. For
wrapped commands, it still runs the real command. By default, KDS does not save
your command output. It only prints a short summary. To save local
troubleshooting files for later, run with `--save-artifacts`.

## What It Does

- Wraps allowlisted build and test commands automatically after install.
- Summarizes pasted output, CI logs, crash logs, and existing log files.
- Prints a short summary by default without saving command output.
- Ships built-in profiles for JavaScript/TypeScript, Python, Go, Java/Kotlin,
  .NET, PHP, Ruby, Elixir, C/C++, and common task runners.
- Shows a short summary first instead of dumping the whole command output.
- Avoids absolute log paths and working-directory paths by default.
- Supports saved local troubleshooting files when you explicitly opt in with
  `--save-artifacts`.
- Spots repeated failure signals and small deltas between matching runs, with
  shorter output for unchanged repeat failures.
- Gives you safe follow-up commands to inspect more details and run local
  health checks.
- Suggests focused rerun commands and the first file to inspect when KDS has a
  specific failure hint.
- Extracts common compiler, linter, and test failure formats into primary
  failures and file hits.
- Reports PowerShell hook, Codex Desktop hook, Desktop hook trust, and local
  state health with `kds doctor`.
- Shows safe local storage stats and can remove old saved KDS files.
- Tracks aggregate line, character, and approximate token reduction for default
  summaries and saved local troubleshooting files.
- Records command-start failures as compact summaries; saved local
  troubleshooting files also keep them in the saved-run list.

## How It Saves Context And Usage

Build and test commands can dump hundreds or thousands of lines into your
terminal. In an AI coding workflow, that output often becomes model-visible
context. KDS cuts that down by showing the useful first pass: run ID, exit code,
timing, summary, warnings, errors, and the follow-up commands to inspect more.

By default, KDS does not save your command output. It only prints a short
summary. To save local troubleshooting files for later, run with
`--save-artifacts`.

`kds gain` leads with approximate token reduction using a simple chars/4
estimate, then shows character and line reduction. It is not an exact
tokenizer. Default runs contribute aggregate-only metrics; saved
troubleshooting runs also allow run-level drilldown.

## Quick Start

Copy this into PowerShell:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/v0.1.0/scripts/bootstrap.ps1 | iex
kds doctor
```

This installs KDS for your Windows user account. It builds KDS locally, adds
`kds.exe` to your user PATH, and turns on automatic summaries for supported
PowerShell and Codex Desktop build/test commands. You need Rust installed first
because KDS does not download Rust or a prebuilt app.

What changes?

KDS will:

- install `kds.exe` under `%LOCALAPPDATA%\CodexKD\bin`
- add that folder to your user PATH
- add a managed PowerShell hook
- update Codex Desktop hooks when it finds a Codex home
- back up files before changing them

The bootstrap downloads the versioned KDS release source archive and its
matching `.sha256` file, verifies the archive, builds it locally, and runs the
installer from that source.

After that, noisy non-interactive verification commands are routed through KDS
automatically in PowerShell and in Codex Desktop where the hook is installed.

You can also run KDS directly or summarize logs that already exist:

```powershell
kds -- <command...>
kds run --budget verbose -- <command...>
kds raw -- <command...>
Get-Content .\ci.log | kds summarize --name github-actions
kds summarize --budget tiny --file .\ci.log --name github-actions --exit-code 1
```

## Useful Commands

```powershell
kds gain
kds logs
kds logs <run-id|last> [--summary|--errors|--error-window|--tail|--file-hits|--show-paths]
kds evidence last
kds summarize --file .\ci.log --name github-actions
kds clean --older-than 30d
kds update check
kds doctor
kds hook status
kds hook uninstall powershell
```

## Privacy

By default, KDS does not save your command output. It only prints a short
summary. To save local troubleshooting files for later, run with
`--save-artifacts`.

Saved local troubleshooting files stay on your machine, but treat them
carefully: command output can contain secrets, local paths, usernames, tokens,
stack traces, environment values, and file contents. Review those files before
sharing them.

The normal summary and detail commands do not print raw stdout/stderr bodies.
That includes `kds gain`, `kds doctor`, `kds logs last`, default
`kds logs <id>`, and `kds evidence last`.

In default mode, KDS prints `Saved logs: no`. When you choose to save local
troubleshooting files, KDS prints a run ID and commands to inspect more details
instead of absolute paths. Use `--show-paths` with `kds run`,
`kds logs last`, `kds logs <id>`, or `kds evidence <id>` when you
explicitly want local paths.

### Advanced privacy details

Default KDS runs do not write raw logs, temp stdout/stderr files, summary
metadata files, saved-run lists, or repeat-failure tracking data. They do write
aggregate `kds gain` metrics such as raw/shown/saved counts, approximate token
counts, command kind, and summary budget. Default metrics do not keep run IDs,
local paths, sidecars, or command strings.

When local troubleshooting files are saved, KDS caps persisted raw stdout and
stderr at 10 MiB per stream by default. Set `KDS_MAX_RAW_BYTES` to a positive
byte count such as `1m` or `250000` to change that cap. KDS still drains child
output after the cap so the wrapped command does not block. Set
`KDS_UNCAPPED_RAW_LOGS=1` when you intentionally want uncapped raw persistence.

Use `KDS_RETENTION_DAYS` to remove old saved local troubleshooting files on run
start, and `KDS_MAX_TOTAL_LOG_BYTES` to keep local KDS files under a disk
budget.
`KDS_COMPRESS_AFTER_DAYS` gzips older raw `.log` files and updates matching
summary metadata files to point at the compressed path.

KDS does not check for updates during normal commands. The bootstrap installer
prints installed and latest release versions before installing. To check from
the CLI, run `kds update check`; that command is an explicit network opt-in.

After deleting saved files, KDS reconciles current lookup state by dropping
saved-run list entries whose summary metadata files are gone, rebuilding
`latest-by-command`, and retiring repeat-failure tracking data that points to
removed raw logs. `kds gain` metrics are lifetime counters and report that
scope explicitly, including how many counted runs had saved artifacts.

KDS redacts common token, API key, password, bearer-token, and URL credential
patterns from summaries, evidence, summary metadata files, and saved-run lists.
That is a guardrail, not a promise that every possible secret-like value was
found.

## When to use KDS

Use KDS for noisy commands where you mainly need to know what failed:

- `npm test`
- `pnpm build`
- `cargo test`
- `pytest`
- `go test`
- `dotnet test`

## When not to use KDS

Do not use KDS when the exact output is the thing you need to keep or share:

- `git status`
- `git diff`
- `git diff --check`
- `git show --stat`
- `git ls-files`
- `git describe`
- `git tag`
- commands that ask for passwords
- SSH sessions
- long-running dev servers
- commands that may print secrets

When in doubt, run the command normally.

## Current Limits

- No telemetry.
- No stored raw-log display command.
- No exact token-savings claims. `kds gain` reports approximate token reduction
  from character counts.
- Summary budgets are `auto`, `tiny`, `normal`, and `verbose` through
  `--budget` or `KDS_SUMMARY_BUDGET`; `auto` keeps unknown failures less
  aggressive while shrinking success and structured output.
- Raw mode tees stdout/stderr live through KDS; exact stream interleaving is
  best-effort.
- Compact mode stays quiet while the command runs except for one short
  long-running notice after the progress threshold.
