# Changelog

## 0.1.0

- Initial public V1 implementation plan and CLI surface.
- Added stronger read-only doctor diagnostics for install, hook, runtime path,
  and local state health.
- Improved failure extraction for pytest node IDs, TypeScript-style locations,
  and common test runner failure lines.
- Added installer validation for the copied binary, hook status, and PATH
  visibility.
- Hardened state reading around malformed `runs.jsonl`, metrics, and digest
  state.
- Added runtime resilience for Ctrl-C child cleanup, stale temp cleanup,
  spawn-failure artifacts, and concurrent run indexing.
- Updated the PowerShell hook to invoke KDS by short executable name instead of
  displaying the full install path.
