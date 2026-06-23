# KDS Decisions

- KDS is a standalone public Rust CLI.
- License is MIT.
- KDS is automatic-hook-first.
- Supported product install equals activation: binary plus automatic
  allowlisted hook.
- Unix shell hooks are not implemented in V1; Unix binary-only install is an
  explicit development/manual helper, not a product-style activated install.
- RTK is UX inspiration only; no RTK dependency, code, filters, hooks, assets,
  branding, wording, or command catalog.
- V1 saves full raw logs locally and never truncates saved logs.
- V1 does not provide stored raw-log dumping.
- First response must stay compact.
- Success path compression is required for exit code `0`.
- Sidecars and `runs.jsonl` are required product surfaces.
- Repeated-failure wording is advisory: "same failure signal," never "same
  root cause."
- Digest/delta state must never skip command execution.
- `doctor` and `hook doctor` are read-only.
- Tests use `KDS_HOME`, temp `CODEX_HOME`, and temp PowerShell profile paths.
