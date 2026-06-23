# Install

## Windows

```powershell
.\scripts\install.ps1
```

The installer builds KDS locally, copies `kds.exe` into
`%LOCALAPPDATA%\CodexKD\bin`, and installs the automatic PowerShell hook.

The installer prints every path it writes. It does not silently edit PATH. If
the install directory is not already on PATH, it prints the command/user action
needed.

Dry-run:

```powershell
.\scripts\install.ps1 --dry-run
```

## Linux/macOS

```sh
./scripts/install.sh
```

The installer builds KDS locally and installs to `$HOME/.local/bin`.
