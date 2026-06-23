# Uninstall

Remove the PowerShell hook:

```powershell
kds hook uninstall powershell
```

Remove Codex guidance:

```powershell
kds init -g --codex --uninstall
```

Remove the installed binary from the path printed by the installer.

KDS logs and state live under `%LOCALAPPDATA%\CodexKD\kds` by default. Review
logs before deleting or sharing them.
