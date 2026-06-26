# KDS

KDS, short for KD Savings, is a local evidence compressor for AI coding
workflows.

It can wrap a command or ingest existing log text from stdin or a file. For
wrapped commands, it still runs the real command. By default it keeps capture
memory-only and writes no raw logs, temp stdout/stderr files, sidecars, run
index, or metrics.

## What It Does

- Wraps allowlisted build and test commands automatically after install.
- Summarizes pasted output, CI logs, crash logs, and existing log files.
- Keeps default capture memory-only, with durable artifacts as an explicit
  opt-in.
- Ships built-in profiles for JavaScript/TypeScript, Python, Go, Java/Kotlin,
  .NET, PHP, Ruby, Elixir, C/C++, and common task runners.
- Shows a short summary first instead of dumping the whole command output.
- Avoids absolute log paths and working-directory paths by default.
- Supports an explicit saved-artifact mode for local drilldown without making
  it the default path.
- Spots repeated failure signals and small deltas between matching runs, with
  shorter output for unchanged repeat failures.
- Gives you safe follow-up commands for logs, evidence packs, and local health
  checks.
- Extracts common compiler, linter, and test failure formats into primary
  failures and file hits.
- Reports PowerShell hook, Codex Desktop hook, Desktop hook trust, and local
  state health with `kds doctor`.
- Shows safe local log storage stats and can remove old local KDS artifacts.
- Tracks line, character, and approximate token reduction for saved artifacts.
- Records spawn failures as compact summaries; saved artifact mode also indexes
  them as normal KDS runs.

## How It Saves Context And Usage

Build and test commands can dump hundreds or thousands of lines into your
terminal. In an AI coding workflow, that output often becomes model-visible
context. KDS cuts that down by showing the useful first pass: run ID, exit code,
timing, summary, warnings, errors, and the follow-up commands to inspect more.

Default output is memory-only. Saved-artifact mode is an explicit opt-in for
users who deliberately want a capped local artifact set.

`kds gain` reports estimated line and character reduction, plus approximate
token reduction using a simple chars/4 estimate. It is not an exact tokenizer,
but it gives you a practical read for saved artifact runs.

## Quick Start

Copy this into PowerShell:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/v0.1.0/scripts/bootstrap.ps1 | iex
kds doctor
```

The bootstrap downloads the versioned KDS release source archive and its
matching `.sha256` file, verifies the archive, builds it locally, installs
`kds.exe` under `%LOCALAPPDATA%\CodexKD\bin`, adds that directory to your user
PATH, installs the PowerShell hook, and updates Codex Desktop hooks when it can
find a Codex home. Rust/Cargo must already be on PATH. KDS does not download a
prebuilt binary and does not download or install Rust/Cargo.

After that, noisy non-interactive verification commands are routed through KDS
automatically in PowerShell and in Codex Desktop where the hook is installed.

You can also run KDS directly or summarize logs that already exist:

```powershell
kds -- <command...>
kds raw -- <command...>
Get-Content .\ci.log | kds summarize --name github-actions
kds summarize --file .\ci.log --name github-actions --exit-code 1
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

Default KDS runs do not write raw logs. Saved artifacts are local, but treat
them carefully: raw logs can contain secrets, local paths, usernames, tokens,
stack traces, environment values, and file contents. Review raw logs before
sharing them.

The normal summary and drilldown commands do not print raw stdout/stderr bodies.
That includes `kds gain`, `kds doctor`, `kds logs last`, default
`kds logs <id>`, and `kds evidence last`.

In memory-only mode, KDS prints compact evidence and notes that artifacts were
not saved. In saved artifact mode, KDS prints a run ID and local drilldown
commands instead of absolute paths. Use `--show-paths` with `kds run`,
`kds logs last`, `kds logs <id>`, or `kds evidence <id>` when you
explicitly want local paths.

When artifacts are saved, KDS caps persisted raw stdout and stderr at 10 MiB
per stream by default. Set `KDS_MAX_RAW_BYTES` to a positive byte count such as
`1m` or `250000` to change that cap. KDS still drains child output after the cap
so the wrapped command does not block. Set `KDS_UNCAPPED_RAW_LOGS=1` when you
intentionally want uncapped raw persistence.

Use `KDS_RETENTION_DAYS` to remove old local run artifacts on run start, and
`KDS_MAX_TOTAL_LOG_BYTES` to keep local KDS artifacts under a disk budget.
`KDS_COMPRESS_AFTER_DAYS` gzips older raw `.log` files and updates matching
sidecars to point at the compressed path.

KDS does not check for updates during normal commands. The bootstrap installer
prints installed and latest release versions before installing. To check from
the CLI, run `kds update check`; that command is an explicit network opt-in.

After artifact deletion, KDS reconciles current lookup state by dropping run
index entries whose sidecars are gone, rebuilding `latest-by-command`, and
retiring digest shards that point to removed raw logs. `kds gain` metrics are
lifetime counters and report that scope explicitly.

KDS redacts common token, API key, password, bearer-token, and URL credential
patterns from summaries, evidence, sidecars, and indexes. That is a guardrail,
not a promise that every possible secret-like value was found.

## When To Skip KDS

Use KDS for noisy build and test commands. Skip it when exact output is the
point of the command, such as readiness evidence, `git status`,
`git diff --name-only`, `git diff --check`, tracked diff hash commands, or
publish/install proof lines.

If proof-style Git commands are accidentally run through KDS, KDS passes them
through to native Git and does not write KDS run artifacts. That includes
`git status`, `git rev-parse`, `git hash-object`, `git diff ...`, and
`git log --oneline`.

Do not use KDS for interactive commands, password prompts, SSH sessions,
long-running daemons, commands likely to print secrets, exact `rg` or
`git grep` searches, or tiny commands where wrapping adds no value.

## Current Limits

- No telemetry.
- No stored raw-log display command.
- No exact token-savings claims. `kds gain` reports approximate token reduction
  from character counts.
- Raw mode tees stdout/stderr live through KDS; exact stream interleaving is
  best-effort.
- Compact mode stays quiet while the command runs except for one short
  long-running notice after the progress threshold.
