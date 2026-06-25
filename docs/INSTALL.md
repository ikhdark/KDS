# Install

## Windows

Copy-paste install:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/main/scripts/bootstrap.ps1 | iex
```

The bootstrap installer downloads the KDS source archive, builds it locally,
and runs the Windows installer from that source. It does not download a
prebuilt binary. Rust/Cargo must already be available on PATH.

From an existing KDS source checkout:

```powershell
.\scripts\install.ps1
```

The installer builds KDS locally, copies `kds.exe` into
`%LOCALAPPDATA%\CodexKD\bin`, adds that directory to the user PATH when needed,
installs the automatic PowerShell hook, installs or updates the Codex Desktop
hook for detected Codex homes, and writes the matching Codex hook trust state so
the installed hook is active without a manual approval step. If an existing
PowerShell profile, Desktop hook script, `hooks.json`, or `config.toml` is
rewritten, KDS writes a timestamped `.kds-backup-*` copy next to the file first.

After copying the binary, the installer validates that `kds.exe` exists, that
`kds --version` runs, that the PowerShell hook is installed unless `--no-hook`
was requested, whether detected Codex Desktop hook files were updated, and
whether the install directory is visible on the user PATH.

Dry-run:

```powershell
.\scripts\install.ps1 --dry-run
```

Binary-only local install without automatic hook activation:

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
