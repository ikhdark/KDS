# Install

## Windows

Copy-paste install:

```powershell
irm https://raw.githubusercontent.com/ikhdark/KDS/v0.1.0/scripts/bootstrap.ps1 | iex
```

The bootstrap installer downloads the versioned KDS release source archive and
its matching `.sha256` file, verifies the archive, builds it locally, and runs
the Windows installer from that source. It does not download a prebuilt binary.
Rust/Cargo must already be available on PATH. KDS does not download or install
Rust/Cargo.

From an existing KDS source checkout:

```powershell
.\scripts\install.ps1
```

The installer requires Rust/Cargo to already be available on PATH. It does not
download or install Rust/Cargo. The installer builds KDS locally, copies `kds.exe` into
`%LOCALAPPDATA%\CodexKD\bin`, adds that directory to the user PATH when needed,
installs the automatic PowerShell hook, installs or updates the Codex Desktop
hook for detected Codex homes, and writes the matching Codex hook trust state so
the installed hook is active without a manual approval step. If an existing
PowerShell profile, Desktop hook script, `hooks.json`, or `config.toml` is
rewritten, KDS writes a unique `.kds-backup-*` copy next to the file first, then
writes replacement text through a same-directory temp file before replacing the
target.

After copying the binary, the installer validates that `kds.exe` exists, that
`kds --version` runs, that the PowerShell hook is installed unless `--no-hook`
was requested, whether detected Codex Desktop hook files were updated, and
whether the install directory is visible on the user PATH.

After install, run `kds doctor` to check the local runtime state, PowerShell
hook status, Codex Desktop hook install status, Codex Desktop hook trust state,
Desktop hook script validity, and whether the Desktop `hooks.json` file is
parseable.

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
