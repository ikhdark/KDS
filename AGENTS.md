# KDS Project Instructions

KDS is a standalone public Rust CLI. It is not part of `codexKD2-main`.

## Product Rules

- KDS is automatic-hook-first: installing KDS installs the binary and the
  automatic allowlisted hook.
- KDS must not install, vendor, shell out to, copy, or depend on RTK.
- RTK may be mentioned only as install/adoption UX inspiration.
- Full raw logs are local archival data; summaries, sidecars, and indexes are
  the product surface.
- Never add telemetry in V1.
- Never add network calls in V1 except future explicit release installer
  downloads with checksum verification.

## Safety Rules

- Raw logs may contain secrets, paths, usernames, tokens, stack traces,
  environment values, and file contents.
- Do not print raw stored logs in V1.
- `doctor` and `hook doctor` are read-only and must not create runtime
  directories.
- Use `KDS_HOME` in tests so real user logs/metrics are not touched.
- Use temp `CODEX_HOME` and temp PowerShell profile paths in tests.
- Do not mutate the real global Codex config, global PATH, real PowerShell
  profile, Codex Desktop settings, `codexKD2-main`, or `codexKD2-devtools`
  during tests.
