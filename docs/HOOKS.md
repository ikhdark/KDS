# Hooks

KDS V1 installs an automatic PowerShell hook by default on Windows.
When installing or repairing the hook, KDS backs up an existing PowerShell
profile before writing the managed block.

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
the short executable name so wrapped commands display as `kds`/`kds.exe` rather
than the full install path.

The hook must not wrap KDS itself, precise searches, interactive sessions,
password prompts, SSH sessions, long-running daemons, or commands likely to
print secrets.

Git commands are not wrapped automatically because `git status` output is often
captured by scripts, prompt themes, and other tools. Run `kds -- git status`
explicitly when KDS capture is desired.

`git diff ...` is an exact-output workflow. If it is accidentally invoked
through KDS, KDS passes it through to native Git without writing KDS run
artifacts.

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

Managed PowerShell profile block:

```powershell
# kds-hook-start
# Managed by KDS. Remove with: kds hook uninstall powershell
# ...
# kds-hook-end
```
