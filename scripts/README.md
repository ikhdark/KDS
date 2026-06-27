# Scripts Agent Instructions

Scope: `scripts/`.

## Ownership

- `bootstrap.ps1` is the copy-paste bootstrap installer. It downloads the public
  KDS source archive and runs `scripts/install.ps1` from the extracted source.
- `install.ps1` is the supported Windows product installer. It builds KDS with
  Cargo, installs `kds.exe` under `%LOCALAPPDATA%\CodexKD\bin`, updates user
  PATH, installs the PowerShell hook, and updates Codex Desktop hooks when a
  Codex home is found.
- `install.sh --binary-only` is a Unix/manual helper in V1. Do not make it the
  primary supported install path without an explicit task.

## Safety Rules

- Preserve `--dry-run` behavior. Dry runs must avoid downloads, builds,
  installs, PATH changes, profile edits, Codex hook edits, and destructive file
  operations.
- Do not mutate the real global Codex config, global PATH, real PowerShell
  profile, or Codex Desktop settings during tests.
- Use temporary paths when validating hook or install behavior.
- Guard recursive deletion with resolved-path checks. Deletion must be limited
  to installer-created temporary directories or explicit KDS-owned targets.
- Preserve existing user profile content outside the managed KDS block.
- Do not add telemetry.
- Do not add runtime network calls beyond explicit source-archive bootstrap
  downloads, installer-time release metadata checks, and checksum verification.
  Future binary downloads require checksum verification.

## Installer Behavior

- Keep install automatic-hook-first on Windows.
- Keep Cargo as the build mechanism. Do not download a prebuilt binary in V1.
- Do not download or install Rust/Cargo. If Cargo is missing, fail clearly and
  tell the user to install it separately.
- Generated PowerShell and Codex Desktop hooks must leave Cargo commands native.
  Do not reintroduce automatic wrapping for `cargo check`, `cargo test`,
  `cargo build`, or `cargo clippy`.
- Preserve clear failure modes for missing Cargo, failed build, failed install,
  and hook update failures.
- Keep install output suitable for proof-line extraction when validation needs
  exact evidence.

## Validation

- Use temporary profile and Codex home paths for hook and installer validation.
- Prefer `-DryRun` checks before any installer path that would mutate user
  configuration.
