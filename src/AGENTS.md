# Source Agent Instructions

Scope: `src/`.

## Module Boundaries

- `main.rs` should remain a thin binary entrypoint that calls `cli::run`.
- `cli.rs` owns Clap parsing and command dispatch.
- `runner.rs` owns wrapped command execution, mode handling, exit-code
  preservation, memory-only default capture, saved-artifact raw capture,
  sidecar writes, metrics updates, digest updates, and Git passthrough.
- `summarize.rs` owns compact summaries, evidence-safe formatting, path hiding,
  file/error extraction, and redaction.
- `storage.rs` owns runtime path discovery, private file/directory helpers,
  sidecar/index paths, state locking, and KDS-owned artifact lifecycle helpers.
- `digest.rs` owns repeated-failure signal tracking. Preserve the phrase
  "same failure signal".
- `logs.rs`, `evidence.rs`, `gain.rs`, `gc.rs`, and `doctor.rs` own their
  respective CLI command behavior.
- `hook.rs` owns managed PowerShell hook text, hook install/uninstall/status,
  and hook allowlist behavior.
- `init_codex.rs` owns managed Codex guidance under `CODEX_HOME`.
- `update.rs` owns the explicit opt-in GitHub release update check.

## Product Invariants

- Always execute the requested wrapped command unless using documented native
  passthrough for proof-style Git commands.
- Preserve child exit codes.
- Drain child stdout and stderr in both memory-only and saved-artifact modes.
- Keep default displayed output compact and safe.
- Do not write raw logs, temp stdout/stderr files, sidecars, run indexes, or
  metrics by default.
- Do not suggest saved artifacts, local logging, or raw-log persistence as
  routine next actions. Only use saved-artifact mode when the user explicitly
  asks for local persisted evidence.
- Do not print raw stored stdout/stderr bodies in V1.
- Hide absolute log paths and CWD paths unless `--show-paths` is set.
- Apply redaction before writing summaries, sidecars, indexes, evidence, and
  displayed command metadata.
- Keep `doctor` and `hook doctor` read-only.
- Keep cleanup commands limited to KDS-owned artifacts under the resolved logs
  tree.
- Do not add telemetry. Runtime network calls are allowed only for explicit
  user opt-in update checks such as `kds update check`.

## Hook Invariants

- V1 automatic activation is PowerShell-only.
- Keep hook wrapping conservative and allowlisted.
- Do not automatically wrap Git commands, KDS itself, exact-output proof
  commands, interactive commands, password prompts, SSH sessions, daemons,
  deploy/publish commands, or commands likely to print secrets.

## Validation

- Run `cargo check` and `cargo test` for source changes.
- For release-facing or shared behavior changes, also run `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo build --release`.
