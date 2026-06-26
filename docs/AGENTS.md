# Docs Agent Instructions

Scope: `docs/`.

## Ownership

- Treat docs as user-facing product behavior, not design notes.
- Keep docs aligned with `README.md`, `src/cli.rs`, installer scripts, hook
  behavior, privacy behavior, and validation requirements.
- Keep `docs/VALIDATION.md` as the authoritative broad validation checklist.

## Documentation Rules

- Use direct, precise technical language.
- Do not describe raw stored logs as safe to share.
- State that default runs are memory-only and saved artifacts are explicit.
- Do not frame saved artifacts or local logging as recommended next actions.
  Document them only as explicit user-directed opt-ins.
- State that saved raw logs stay local and may contain secrets, paths,
  usernames, tokens, stack traces, environment values, and file contents.
- Preserve the distinction between safe displayed summaries and explicitly
  saved local archival raw logs.
- Document `--show-paths` as the explicit opt-in for local paths.
- Preserve V1 constraints: no telemetry, no stored raw-log display command, no
  runtime network calls, and PowerShell-only automatic activation.
- Use the wording "same failure signal" for repeated-failure behavior.
- Keep install docs automatic-hook-first: supported Windows install means
  binary plus allowlisted PowerShell/Codex hook setup.
- Document `gc` and `prune` as KDS-artifact cleanup commands that must stay
  limited to KDS-owned runtime artifacts.

## Change Coupling

- When CLI flags or commands change, update `README.md` and the affected doc in
  the same task.
- When hook behavior changes, update `docs/HOOKS.md` and validation steps.
- When privacy, redaction, path hiding, retention, compression, or raw capture
  behavior changes, update `docs/PRIVACY.md` and validation steps.
- When installer behavior changes, update `docs/INSTALL.md` and
  `docs/UNINSTALL.md` as needed.

## Validation

- Validate documentation changes by inspection.
- For behavior-coupled docs, run the narrow command or test that proves the
  documented behavior still matches the implementation.
