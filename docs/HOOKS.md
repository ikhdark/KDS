# Hooks

KDS V1 installs an automatic PowerShell hook by default on Windows.

The hook is allowlisted and conservative. If it is uncertain, it runs the
original command unchanged.

The hook must not wrap KDS itself, precise searches, interactive sessions,
password prompts, SSH sessions, long-running daemons, or commands likely to
print secrets.

Managed PowerShell profile block:

```powershell
# kds-hook-start
# Managed by KDS. Remove with: kds hook uninstall powershell
# ...
# kds-hook-end
```
