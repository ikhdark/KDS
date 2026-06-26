# KDS Decisions

- KDS is a standalone public Rust CLI.
- License is MIT.
- KDS is automatic-hook-first.
- Supported product install equals activation: binary plus automatic
  allowlisted hook.
- Unix shell hooks are not implemented in V1; Unix binary-only install is an
  explicit development/manual helper, not a product-style activated install.
- KDS must stay original and must not depend on, copy, or vendor another command
  wrapping tool.
- KDS installers must not download or install Rust/Cargo. Source-based install
  requires Cargo to already be on PATH and must fail clearly when it is missing.
- Normal KDS runtime commands must not call the network. Update checks are
  explicit: installer-time release metadata checks and `kds update check`.
- V1 is memory-only by default: wrapped commands and imported logs must not
  write raw logs, temp stdout/stderr files, sidecars, run indexes, or metrics
  unless artifact saving is explicitly enabled.
- Saved artifact mode is opt-in through `--save-artifacts` or
  `KDS_SAVE_ARTIFACTS=1`. In saved artifact mode, `KDS_MAX_RAW_BYTES` may
  change the persisted raw-byte cap, and `KDS_UNCAPPED_RAW_LOGS=1` is the
  explicit opt-in for uncapped persistence, but KDS must keep draining child
  output.
- V1 does not provide stored raw-log dumping.
- First response must stay compact.
- Success path compression is required for exit code `0`.
- Sidecars and `runs.jsonl` are saved-artifact product surfaces.
- `kds gain` metrics are lifetime counters, while cleanup reconciliation keeps
  current lookup state aligned with retained sidecars.
- Repeated-failure wording is advisory: "same failure signal," never "same
  root cause."
- Digest/delta state must never skip command execution.
- `doctor` and `hook doctor` are read-only.
- Tests use `KDS_HOME`, temp `CODEX_HOME`, and temp PowerShell profile paths.
