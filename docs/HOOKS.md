# Hooks

KDS V1 installs automatic PowerShell activation by default on Windows. The
Windows installer also installs or updates a Codex Desktop `PreToolUse` hook for
detected Codex homes so allowlisted Desktop shell commands are routed through
KDS without manual configuration.

When installing or repairing hooks, KDS backs up an existing PowerShell profile,
Desktop hook script, Desktop `hooks.json`, or Codex `config.toml` before
rewriting it through a same-directory temp file and replace operation. For
Desktop hooks, the installer also writes the matching `hooks.state` trust entry
so the installed hook is active without a manual approval step.

The hook is allowlisted and conservative. If it is uncertain, it runs the
original command unchanged.

Interactive PowerShell prompts show a `KDS` prefix after the hook loads. The
prompt marker is only a visibility signal; automatic capture still follows the
allowlist rules below.

The managed block installs PowerShell functions for allowlisted commands. If a
user-defined PowerShell alias with the same name already takes precedence, KDS
does not silently remove it; rename or remove that alias if automatic wrapping
is desired for that command.

The hook keeps the resolved KDS executable path internally, prepends that
directory to the current PowerShell session PATH when needed, and invokes KDS by
the short command name so wrapped commands display as `KDS` rather than the full
install path.

The hook must not wrap KDS itself, precise searches, interactive sessions,
password prompts, SSH sessions, long-running daemons, or commands likely to
print secrets.

Git commands are not wrapped automatically because their output is often
captured by scripts, prompt themes, readiness checks, and other tools.

Proof-style Git commands are exact-output workflows. If `git status`,
`git rev-parse`, `git hash-object`, `git diff ...`, or `git log --oneline` is
accidentally invoked through KDS, KDS passes it through to native Git without
writing KDS run artifacts.

For readiness workflows, keep exact evidence commands native. Do not route
proof-line commands through KDS when their output is the deliverable, including
`git status`, `git diff --name-only`, `git diff --check`, tracked diff hash
commands, and publish/install proof-line extraction.

For package scripts and `just`, the automatic hook wraps common verification
tasks only: `test`, `build`, `check`, `lint`, `typecheck`, `ci`, and `clippy`.
Hyphenated variants of those task names, such as `test-fast`, are treated as
the same verification family. Other script or recipe names run natively because
they may deploy, prompt, or print sensitive operational output.

For TypeScript and JavaScript, the automatic hook wraps common bounded
verification commands: `tsc`, `vue-tsc`, `eslint`, `vitest`, `jest`,
`playwright test`, `biome check`, `biome ci`, `biome lint`, and
`prettier --check`.

For Python, the automatic hook wraps test and static-analysis runners:
`pytest`, `python -m pytest`, `python -m unittest`, `python -m ruff`,
`python -m mypy`, `python -m pyright`, `ruff check`, `ruff format --check`,
`mypy`, `pyright`, and matching `uv run ...` forms. Other `python ...`
commands run natively because they may be interactive, long-running, or print
sensitive operational output.

For .NET and Go, the automatic hook wraps bounded build and test commands:
`dotnet build`, `dotnet test`, `go build`, and `go test`.

The Codex Desktop hook parses matched shell commands into PowerShell argv tokens
and rewrites only commands it can prove are simple argv-equivalent allowlist
matches. Ambiguous input, shell control operators, expansion, variables,
comments, parse errors, and wildcard tokens run natively. Rewritten commands use
the resolved local `kds.exe -- ...` path with each original argument quoted.

Managed PowerShell profile block:

```powershell
# kds-hook-start
# Managed by KDS. Remove with: kds hook uninstall powershell
# ...
# kds-hook-end
```
