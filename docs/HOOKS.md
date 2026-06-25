# Hooks

KDS V1 installs automatic PowerShell activation by default on Windows. The
Windows installer also installs or updates a Codex Desktop `PreToolUse` hook for
detected Codex homes so allowlisted Desktop shell commands are routed through
KDS without manual configuration.

When installing or repairing hooks, KDS backs up an existing PowerShell profile,
Desktop hook script, Desktop `hooks.json`, or Codex `config.toml` before
rewriting it. For Desktop hooks, the installer also writes the matching
`hooks.state` trust entry so the installed hook is active without a manual
approval step.

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
Other script or recipe names run natively because they may deploy, prompt, or
print sensitive operational output.

For Python, the automatic hook wraps test runners only: `pytest`,
`python -m pytest`, and `python -m unittest`. Other `python ...` commands run
natively because they may be interactive, long-running, or print sensitive
operational output.

The Codex Desktop hook rewrites matched shell commands to `KDS -- ...` so
Desktop status text shows the short KDS command rather than the full local
install path. It only rewrites simple commands without shell control operators.

Managed PowerShell profile block:

```powershell
# kds-hook-start
# Managed by KDS. Remove with: kds hook uninstall powershell
# ...
# kds-hook-end
```
