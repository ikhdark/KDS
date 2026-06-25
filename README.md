# KDS

KDS, short for KD Savings, is a command evidence runner for noisy developer
commands. It keeps first output compact, gives every run a stable ID, and lets
you drill into only the evidence slice you need later.

KDS is RTK-style in installation and adoption UX only. KDS does not copy RTK
code, command filters, hooks, branding, assets, README wording, or product
scope, and it does not depend on RTK.

## What KDS Does

- Routes allowlisted noisy commands through an automatic shell hook after
  install.
- Saves full raw stdout/stderr logs locally.
- Prints compact evidence summaries for model-visible output without absolute
  log/CWD paths by default.
- Stores `.summary.json` sidecars and `state/runs.jsonl` for fast drilldown.
- Detects repeated failure signals and tiny deltas between exact-match runs.
- Provides safe drilldown commands and compact evidence packs.
- Records spawn failures as normal KDS run artifacts and cleans up stale KDS
  temp files from prior abnormal exits.
- Reports local install, hook, runtime path, and state-file health with
  `kds doctor`.

## Quick Start

Copy and paste into PowerShell:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/main/scripts/bootstrap.ps1 | iex
kds doctor
```

The bootstrap installer downloads the KDS source archive, builds it locally,
installs `kds.exe` under `%LOCALAPPDATA%\CodexKD\bin`, adds that directory to
the user PATH, installs the PowerShell hook, and installs or updates the Codex
Desktop hook for detected Codex homes. It requires Rust/Cargo to be available
on PATH and does not download a prebuilt binary.

After install, allowlisted noisy build and test commands such as `cargo test`
and `npm test` are routed through KDS automatically in PowerShell and Codex
Desktop where the hook is installed.

Manual fallback/debug usage is also available:

```powershell
kds -- cargo test
kds run -- npm test
kds raw -- node --version
```

## Common Commands

```powershell
kds gain
kds logs dir
kds logs last
kds logs show <run-id> --show-paths
kds logs show <run-id> --errors
kds evidence last
kds hook status
kds hook uninstall powershell
```

## Privacy Warning

KDS raw logs are local only, but they may contain secrets, local paths,
usernames, tokens, stack traces, environment values, and file contents. Do not
share raw logs without reviewing and redacting them.

`kds gain`, `kds doctor`, `kds logs last`, default `kds logs show <id>`, and
`kds evidence last` do not print raw stdout/stderr bodies.

Default compact, logs, and evidence output prints the run ID plus local drilldown
commands instead of absolute log paths. Use `--show-paths` on `kds run`,
`kds logs last`, `kds logs show <id>`, or `kds evidence <id>` when you
explicitly want local paths in interactive output.

Set `KDS_MAX_RAW_BYTES` to a positive byte count to cap persisted raw stdout
and stderr per stream. KDS continues draining the child process after the cap so
the wrapped command does not block on a full pipe. Unset it or set it to `0` for
unlimited raw-log capture.

KDS redacts common token, API key, password, bearer-token, and URL credential
patterns from summaries, evidence, sidecars, and indexes. This is a safety
guardrail, not a guarantee that every possible secret-like value is removed.

## When Not To Use KDS

Use KDS for noisy build and test commands. Do not use KDS when exact output
lines are the deliverable, such as readiness evidence, `git status`,
`git diff --name-only`, `git diff --check`, tracked diff hash commands, or
publish/install proof-line extraction.

If `git diff ...` is accidentally invoked through KDS, KDS passes it through to
native Git without writing KDS run artifacts.

Do not use KDS for interactive commands, password prompts, SSH sessions,
long-running daemons, commands likely to print secrets, exact `rg` or
`git grep` searches, or tiny commands where wrapping adds no value.

## V1 Limits

- No telemetry.
- No stored raw-log display command.
- No exact token-savings claims; `kds gain` reports usage savings as estimated
  line-based output reduction.
- Raw mode prints captured stdout then captured stderr; exact stream
  interleaving is not preserved in V1.
- Wrapped command stdout/stderr is drained to local temp files while the command
  runs, then summarized from bounded line state. V1 still does not stream live
  progress to compact mode.
