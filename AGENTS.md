# KDS Agent Instructions

KDS is a standalone public Rust CLI. Keep this repository independent from
`codexKD2-main`, `codexKD2-devtools`, and local Codex runtime state.

## Current Project Shape

- `src/main.rs` wires the binary entrypoint to `cli::run`.
- `src/cli.rs` defines the Clap command surface: `run`, `raw`, `gain`, `gc`,
  `prune`, `doctor`, `logs`, `evidence`, `init`, and `hook`.
- `src/runner.rs` executes wrapped commands, preserves exit codes, captures
  stdout/stderr, produces memory-only summaries by default, writes raw logs and
  summary sidecars only in saved-artifact mode, updates indexes/metrics/digest
  state only in saved-artifact mode, and handles Git passthrough.
- `src/summarize.rs` owns compact output, evidence output, safe log metadata,
  path hiding, file/error extraction, and redaction.
- `src/storage.rs` owns `KDS_HOME` discovery, run IDs, runtime paths, private
  file/directory helpers, sidecars, `state/runs.jsonl`,
  `state/digest-index.json`, `state/metrics.json`, and state locking.
- `src/digest.rs` owns repeated-failure signal state. Preserve the wording
  "same failure signal"; do not replace it with "same root cause".
- `src/logs.rs`, `src/evidence.rs`, `src/gain.rs`, `src/gc.rs`, and
  `src/doctor.rs` implement safe drilldown, evidence packs, metrics, local
  artifact pruning, and read-only health checks.
- `src/hook.rs` owns the managed PowerShell hook block and automatic hook
  allowlist.
- `src/init_codex.rs` owns managed Codex guidance under `CODEX_HOME`.
- `scripts/bootstrap.ps1` downloads the public source archive and runs the
  installer from that extracted source.
- `scripts/install.ps1` is the Windows product installer. It builds the binary,
  installs it under `%LOCALAPPDATA%\CodexKD\bin`, updates user PATH, installs
  the PowerShell hook, and updates Codex Desktop hooks when possible.
- `scripts/install.sh --binary-only` is a Unix/manual helper in V1, not the
  supported automatic install path.
- `tests/wrap.rs` covers end-to-end binary behavior and must use temporary
  runtime storage.
- `.github/workflows/ci.yml` runs formatting, clippy, tests, release build, and
  cargo-audit checks.
- `docs/` contains the user-facing install, privacy, hook, uninstall,
  validation, and decision docs. Keep docs aligned with behavior changes.

## Product Rules

- KDS is automatic-hook-first: supported install means binary plus automatic
  allowlisted hook.
- Do not install, vendor, shell out to, copy, or depend on another command
  wrapping tool.
- Mention other command wrapping tools only as install/adoption UX inspiration.
- Treat compact summaries as the default product surface. Treat sidecars,
  indexes, evidence packs, gain metrics, and raw logs as saved-artifact surfaces.
- Do not write raw logs, temp stdout/stderr files, sidecars, run indexes, or
  metrics by default. Saved artifacts are permitted only when
  `--save-artifacts` or `KDS_SAVE_ARTIFACTS=1` is explicitly set. In
  saved-artifact mode, cap persisted raw bytes per stream while continuing to
  drain child output and writing a truncation note.
- Do not suggest saving logs or enabling saved artifacts as routine guidance,
  next actions, or default troubleshooting steps. Treat saved artifacts as an
  explicit user-directed opt-in only.
- Do not add telemetry in V1.
- Do not add network calls in V1 except future explicit release installer
  downloads with checksum verification.
- Do not add a stored raw-log display command in V1.
- Never let digest/delta logic skip command execution.

## Hook Rules

- V1 automatic activation is PowerShell-only.
- Keep the PowerShell hook conservative and allowlisted. If behavior is
  uncertain, run the native command unchanged.
- The hook may wrap noisy non-interactive verification commands:
  `cargo check/test/build/clippy`, safe `just` recipes, safe `npm`/`pnpm`
  scripts, `pytest`, `python -m pytest`, and `python -m unittest`.
- Safe package and recipe names are `test`, `build`, `check`, `lint`,
  `typecheck`, `ci`, and `clippy`.
- Do not wrap Git commands automatically. Preserve native passthrough for
  `git diff ...` and other proof-style Git commands even when explicitly run
  through KDS.
- Do not wrap KDS itself, exact-output proof commands, precise searches,
  interactive commands, password prompts, SSH sessions, long-running daemons,
  deploy/publish commands, or commands likely to print secrets.

## Safety Rules

- Raw logs may contain secrets, paths, usernames, tokens, stack traces,
  environment values, and file contents.
- Do not print raw stored logs in V1.
- Default compact, logs, and evidence output must avoid absolute log paths and
  CWD paths. `--show-paths` is the explicit local opt-in.
- Apply redaction to summaries, evidence, sidecars, indexes, and displayed
  command metadata. Treat redaction as a guardrail, not a guarantee.
- Keep `doctor` and `hook doctor` read-only. They must not create runtime
  directories.
- Keep `logs`, `evidence`, `gain`, and `doctor` safe over existing state. They
  must not expose raw stdout/stderr bodies.
- Use private runtime storage where the platform supports it: Unix directories
  `0700`, files `0600`, and refusal of world-writable existing `KDS_HOME`.
- Limit deletion logic to KDS-owned artifacts under the resolved KDS logs tree.

## Development And Testing Rules

- Use `KDS_HOME` in tests and ad hoc wrapped-command validation so real user
  logs, indexes, metrics, and digest state are not touched.
- Use temporary `CODEX_HOME` when testing `init` behavior.
- Use temporary `KDS_POWERSHELL_PROFILE` paths when testing hook install,
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
- When changing sidecar, index, metrics, digest, or retention formats, update
  schema constants and tests deliberately.
- Prefer focused unit tests for parser, formatting, storage, redaction, and
  pruning changes.
- Prefer integration coverage in `tests/wrap.rs` for binary, path,
  passthrough, artifact, hook, and CLI behavior.

## Task-Scope Auto-Fix

If an accepted task exposes a fixable issue that directly prevents the change
from being complete, durable, or correctly validated, fix it in the same scoped
change.

Do not use this rule for broad cleanup, dependency upgrades, generated-file
edits, unrelated safety or permission behavior changes, or unrelated failure
fixes. Keep the fix inside the accepted task's natural ownership boundary and
report unrelated blockers separately.

## Validation

Use native Cargo validation for code changes:

```powershell
cargo check
cargo test
```

For CI parity or release-facing changes, also run the relevant stricter checks:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release
```

For wrapped-command behavior, set `KDS_HOME` to a temporary directory first.
Use the checklist in `docs/VALIDATION.md` for broader validation.
