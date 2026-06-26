# GitHub Agent Instructions

Scope: `.github/`.

## Ownership

- Treat GitHub workflows as release and regression gates for the public Rust
  CLI.
- Keep `.github/workflows/ci.yml` aligned with the local validation commands
  documented in `docs/VALIDATION.md`.
- Preserve cross-platform CI coverage on Windows and Ubuntu unless a task
  explicitly changes the support target.

## Workflow Rules

- Keep the stable Rust toolchain unless the project deliberately pins or
  upgrades Rust.
- Preserve these gates when changing CI: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, and
  `cargo build --release`.
- Keep security auditing separate from build/test validation. If changing the
  audit step, preserve a locked install or an equivalent reproducible setup.
- Do not add publish, deploy, release upload, secret-dependent, or telemetry
  jobs without an explicit task.
- Do not use CI to mutate repository contents.

## Validation

- Validate workflow syntax by inspection for small edits.
- For command changes, run the affected local commands natively where possible
  and report any CI-only coverage that was not run locally.
