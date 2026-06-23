# Install

## Windows

```powershell
.\scripts\install.ps1
```

The installer builds KDS locally, copies `kds.exe` into
`%LOCALAPPDATA%\CodexKD\bin`, and installs the automatic PowerShell hook.
If an existing PowerShell profile is rewritten, KDS writes a timestamped
`.kds-backup-*` copy next to the profile first.

The installer prints every path it writes. It does not silently edit PATH. If
the install directory is not already on PATH, it prints the command/user action
needed.

Dry-run:

```powershell
.\scripts\install.ps1 --dry-run
```

Binary-only local install without editing the PowerShell profile:

```powershell
.\scripts\install.ps1 --no-hook
```

## Linux/macOS

Unix shell hooks are not implemented in V1, so the Unix script is not a
product-style activated installer. For development or explicit manual use
without automatic hook activation:

```sh
./scripts/install.sh --binary-only
```

The helper builds KDS locally and installs to `$HOME/.local/bin`. Running
`./scripts/install.sh` without `--binary-only` refuses to install because KDS
install is automatic-hook-first.
