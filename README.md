# KDS

KDS, short for KD Savings, keeps noisy developer commands readable.

It still runs the real command. It still saves the full stdout/stderr logs on
your machine. The difference is that the first thing you see is a compact
summary with a stable run ID, so you can stay oriented and pull more evidence
only when you need it.

## What It Does

- Wraps allowlisted build and test commands automatically after install.
- Keeps full raw stdout/stderr logs locally.
- Shows a short summary first instead of dumping the whole command output.
- Avoids absolute log paths and working-directory paths by default.
- Stores `.summary.json` sidecars and `state/runs.jsonl` for later drilldown.
- Spots repeated failure signals and small deltas between matching runs, with
  shorter output for unchanged repeat failures.
- Gives you safe follow-up commands for logs, evidence packs, and local health
  checks.
- Reports PowerShell hook, Codex Desktop hook, Desktop hook trust, and local
  state health with `kds doctor`.
- Shows safe local log storage stats and can prune old local KDS artifacts.
- Tracks line, character, and approximate token reduction locally.
- Records spawn failures as normal KDS runs and cleans up stale temp files from
  prior abnormal exits.

## How It Saves Context And Usage

Build and test commands can dump hundreds or thousands of lines into your
terminal. In an AI coding workflow, that output often becomes model-visible
context. KDS cuts that down by showing the useful first pass: run ID, exit code,
timing, summary, warnings, errors, and the follow-up commands to inspect more.

The full output is still saved locally, so you are not throwing evidence away.
You only pull the raw log, error slice, or evidence pack when it is actually
needed.

`kds gain` reports estimated line and character reduction, plus approximate
token reduction using a simple chars/4 estimate. It is not an exact tokenizer,
but it gives you a practical read on how much terminal noise KDS kept out of
the conversation and which commands are worth wrapping.

## Quick Start

Copy this into PowerShell:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/main/scripts/bootstrap.ps1 | iex
kds doctor
```

The installer downloads the KDS source archive, builds it locally, installs
`kds.exe` under `%LOCALAPPDATA%\CodexKD\bin`, adds that directory to your user
PATH, installs the PowerShell hook, and updates Codex Desktop hooks when it can
find a Codex home. Rust/Cargo must already be on PATH. KDS does not download a
prebuilt binary.

After that, noisy verification commands like `cargo test` and `npm test` are
routed through KDS automatically in PowerShell and in Codex Desktop where the
hook is installed.

You can also run KDS directly:

```powershell
kds -- cargo test
kds run -- npm test
kds run --budget tight -- cargo test
kds raw -- node --version
```

## Useful Commands

```powershell
kds gain
kds logs dir
kds logs stats
kds logs last
kds logs show <run-id> --show-paths
kds logs show <run-id> --errors
kds logs show last --error-window
kds logs show <run-id> --error-window
kds evidence last
kds gc --older-than 30d --dry-run
kds gc --older-than 30d
kds prune --before 30d --dry-run
kds doctor
kds hook status
kds hook uninstall powershell
```

## Privacy

Raw logs stay local, but treat them carefully. They can contain secrets, local
paths, usernames, tokens, stack traces, environment values, and file contents.
Review raw logs before sharing them.

The normal summary and drilldown commands do not print raw stdout/stderr bodies
by default. That includes `kds gain`, `kds doctor`, `kds logs last`, default
`kds logs show <id>`, and `kds evidence last`.

By default, KDS prints a run ID and local drilldown commands instead of absolute
paths. Use `--show-paths` with `kds run`, `kds logs last`, `kds logs show <id>`,
or `kds evidence <id>` when you explicitly want local paths.

Set `KDS_MAX_RAW_BYTES` to a positive byte count if you want to cap persisted
raw stdout and stderr per stream. KDS still drains child output after the cap so
the wrapped command does not block. Unset it or set it to `0` for unlimited raw
capture.

Use `KDS_RETENTION_DAYS` to prune old local run artifacts on run start, and
`KDS_MAX_TOTAL_LOG_BYTES` to keep local KDS artifacts under a disk budget.
`KDS_COMPRESS_AFTER_DAYS` gzips older raw `.log` files and updates matching
sidecars to point at the compressed path.

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
- Raw mode tees stdout/stderr live while still saving local logs; exact stream
  interleaving is best-effort.
- Compact mode stays quiet while the command runs except for one short
  long-running notice after the progress threshold.
