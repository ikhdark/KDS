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
- Prints compact evidence summaries for model-visible output.
- Stores `.summary.json` sidecars and `state/runs.jsonl` for fast drilldown.
- Detects repeated failure signals and tiny deltas between exact-match runs.
- Provides safe drilldown commands and compact evidence packs.

## Quick Start

```powershell
git clone https://github.com/kds-ai/kds
Set-Location kds
.\scripts\install.ps1
kds doctor
```

After install and a new PowerShell session, allowlisted noisy commands such as
`cargo test` and `git status` are routed through KDS automatically.

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

## When Not To Use KDS

Do not use KDS for interactive commands, password prompts, SSH sessions,
long-running daemons, commands likely to print secrets, exact `rg` or
`git grep` searches, or tiny commands where wrapping adds no value.

## V1 Limits

- No telemetry.
- No stored raw-log display command.
- No exact token-savings claims; KDS reports estimated line-based output
  reduction.
- Raw mode prints captured stdout then captured stderr; exact stream
  interleaving is not preserved in V1.
