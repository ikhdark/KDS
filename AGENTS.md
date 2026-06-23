# KDS Project Instructions

KDS is a standalone public Rust CLI. It is not part of `codexKD2-main`, and
this repository must stay independent from `codexKD2-main` and
`codexKD2-devtools`.

## Current Project Shape

- `src/main.rs` wires the binary entrypoint to `cli::run`.
- `src/cli.rs` defines the Clap command surface: `run`, `raw`, `gain`,
  `doctor`, `logs`, `evidence`, `init`, and `hook`.
- `src/runner.rs` executes wrapped commands, preserves exit codes, captures
  stdout/stderr, writes raw logs, writes summary sidecars, updates indexes,
  metrics, digest state, and handles `git diff` passthrough.
- `src/summarize.rs` owns compact output, evidence output, safe log metadata,
  path hiding, file/error extraction, and redaction.
- `src/storage.rs` owns `KDS_HOME` discovery, run IDs, runtime paths,
  private file/directory helpers, sidecars, `state/runs.jsonl`,
  `state/digest-index.json`, `state/metrics.json`, and state locking.
- `src/digest.rs` owns repeated-failure signal state. Wording must remain
  "same failure signal", not "same root cause".
- `src/logs.rs`, `src/evidence.rs`, `src/gain.rs`, and `src/doctor.rs` are
  command implementations for safe drilldown, evidence packs, metrics, and
  read-only health checks.
- `src/hook.rs` owns the managed PowerShell hook block and its allowlist.
- `src/init_codex.rs` owns managed Codex guidance under `CODEX_HOME`.
- `scripts/install.ps1` is the Windows product installer. It builds the binary,
  installs it under `%LOCALAPPDATA%\CodexKD\bin`, and installs the PowerShell
  hook. `scripts/install.sh --binary-only` is only a Unix/manual helper in V1.
- `tests/wrap.rs` covers end-to-end binary behavior and must keep using temp
  storage.
- `docs/` contains the user-facing install, privacy, hook, uninstall,
  validation, and decision docs. Keep docs aligned with behavior changes.

## Product Rules

- KDS is automatic-hook-first: supported install means binary plus automatic
  allowlisted hook.
- KDS must not install, vendor, shell out to, copy, or depend on RTK.
- RTK may be mentioned only as install/adoption UX inspiration.
- Summaries, sidecars, indexes, evidence packs, and gain metrics are the
  product surface. Raw logs are local archival data.
- By default, KDS saves raw stdout/stderr logs locally. If `KDS_MAX_RAW_BYTES`
  is set, KDS may cap persisted raw bytes per stream, but it must keep draining
  child output and write a truncation note.
- Never add telemetry in V1.
- Never add network calls in V1 except future explicit release installer
  downloads with checksum verification.
- Do not add a stored raw-log display command in V1.
- Digest/delta logic must never skip command execution.

## Hook Rules

- V1 automatic activation is PowerShell-only.
- The PowerShell hook is conservative and allowlisted. If behavior is
  uncertain, run the native command unchanged.
- The hook may wrap noisy non-interactive verification commands:
  `cargo check/test/build/clippy`, safe `just` recipes, safe
  `npm`/`pnpm` scripts, `pytest`, `python -m pytest`, and
  `python -m unittest`.
- Safe package/recipe names are `test`, `build`, `check`, `lint`,
  `typecheck`, `ci`, and `clippy`.
- Git commands are not wrapped automatically. `git diff ...` must passthrough
  even when explicitly invoked through KDS.
- Do not wrap KDS itself, exact-output proof commands, precise searches,
  interactive commands, password prompts, SSH sessions, long-running daemons,
  deploy/publish commands, or commands likely to print secrets.

## Safety Rules

- Raw logs may contain secrets, paths, usernames, tokens, stack traces,
  environment values, and file contents.
- Do not print raw stored logs in V1.
- Default compact, logs, and evidence output must avoid absolute log paths and
  CWD paths. `--show-paths` is the explicit local opt-in.
- Redaction applies to summaries, evidence, sidecars, indexes, and displayed
  command metadata. Treat it as a guardrail, not a guarantee.
- `doctor` and `hook doctor` are read-only and must not create runtime
  directories.
- `logs`, `evidence`, `gain`, and `doctor` should read existing state safely
  and should not expose raw stdout/stderr bodies.
- Use private runtime storage where the platform supports it: Unix directories
  `0700`, files `0600`, and refusal of world-writable existing `KDS_HOME`.

## Development and Testing Rules

- Use `KDS_HOME` in tests and ad hoc wrapped-command validation so real user
  logs, indexes, metrics, and digest state are not touched.
- Use temp `CODEX_HOME` when testing `init` behavior.
- Use temp `KDS_POWERSHELL_PROFILE` paths when testing hook install,
  uninstall, or repair behavior.
- Do not mutate the real global Codex config, global PATH, real PowerShell
  profile, Codex Desktop settings, `codexKD2-main`, or `codexKD2-devtools`
  during tests.
- Keep command output exact when exact lines are the deliverable. Run readiness
  evidence natively, including `git status`, `git diff --name-only`,
  `git diff --check`, tracked diff hash commands, and publish/install
  proof-line extraction.
- When changing CLI behavior, update `README.md` and the relevant docs under
  `docs/`.
- When changing sidecar, index, metrics, or digest formats, update schema
  constants and tests deliberately.
- Prefer focused unit tests for parser/formatting/storage changes and
  integration tests in `tests/wrap.rs` for binary, path, passthrough, and
  artifact behavior.

## Task-Scope Auto-Fix

If an accepted task exposes a fixable issue that directly prevents the change
from being complete, durable, or correctly validated, fix it in the same scoped
change instead of only reporting it.

This does not authorize broad cleanup, dependency upgrades, generated-file
edits, safety or permission behavior changes, or unrelated failure fixes. Keep
the fix inside the accepted task's natural ownership boundary and report
unrelated blockers separately.

## Validation

Use native Cargo validation for code changes:

```powershell
cargo check
cargo test
```

For wrapped-command behavior, set `KDS_HOME` to a temp directory first. The
validation checklist lives in `docs/VALIDATION.md`.
