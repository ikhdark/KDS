# Hooks

KDS V1 installs an automatic PowerShell hook by default on Windows.

The hook is allowlisted and conservative. If it is uncertain, it runs the
original command unchanged.

The hook must not wrap KDS itself, precise searches, interactive sessions,
password prompts, SSH sessions, long-running daemons, or commands likely to
print secrets.

For Git, the automatic hook wraps `git status` only. Commands such as
`git diff`, `git log`, `git grep`, and `git show` may expose code, file
contents, or exact search results and must be run explicitly if KDS capture is
desired.

For package scripts and `just`, the automatic hook wraps common verification
tasks only: `test`, `build`, `check`, `lint`, `typecheck`, `ci`, and `clippy`.
Other script or recipe names run natively because they may deploy, prompt, or
print sensitive operational output.

Managed PowerShell profile block:

```powershell
# kds-hook-start
# Managed by KDS. Remove with: kds hook uninstall powershell
# ...
# kds-hook-end
```
