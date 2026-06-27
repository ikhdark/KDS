# Tests Agent Instructions

Scope: `tests/`.

## Ownership

- `tests/wrap.rs` covers end-to-end KDS binary behavior.
- Keep integration tests focused on user-visible CLI behavior, default
  memory-only behavior, saved artifact creation, safe output, passthrough, hook
  behavior, retention, and failure resilience.

## Isolation Rules

- Always use temporary `KDS_HOME` for wrapped-command tests.
- Use temporary `CODEX_HOME` for `init` behavior.
- Use temporary `KDS_POWERSHELL_PROFILE` for hook install, uninstall, and
  repair behavior.
- Do not mutate real user logs, indexes, metrics, digest state, global Codex
  config, global PATH, real PowerShell profile, or Codex Desktop settings.
- Do not depend on existing local runtime state.

## Coverage Rules

- Preserve coverage for exit-code preservation, default runs not writing local
  artifacts, saved-artifact raw log creation, sidecar creation, index updates,
  metrics updates, digest shards, repeated-failure output, safe drilldown, spawn
  failures, stale temp cleanup, and truncation.
- Preserve exact-output passthrough coverage for proof-style Git commands.
- Preserve hook coverage proving Cargo commands run natively while other
  allowlisted noisy commands still route through KDS.
- Assert that safe commands do not print raw stdout/stderr bodies unless the
  behavior is explicitly live raw-mode output.
- Assert that default summary output does not suggest enabling saved artifacts
  or local logging.
- Assert that path hiding remains the default and `--show-paths` is the opt-in.
- Use the phrase "same failure signal" in repeated-failure expectations.

## Validation

Run:

```powershell
cargo test
```
