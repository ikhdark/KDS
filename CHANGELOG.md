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
- Added a one-line PowerShell bootstrap installer that downloads the source
  archive, builds KDS, installs the binary, updates PATH, and activates
  PowerShell plus Codex Desktop hooks automatically.
- Added automatic Codex Desktop hook trust-state updates for KDS-installed
  Desktop hooks.
- Added exact-output passthrough for proof-style Git commands, including
  `git status`, `git rev-parse`, `git hash-object`, `git diff ...`, and
  `git log --oneline`.
- Added Codex Desktop hook install/trust/script/hooks.json checks to
  `kds doctor`.
- Added `kds logs stats` plus `kds gc --older-than ...` with dry-run support
  for old local KDS artifacts.
- Improved repeated failure matching with multiple normalized error lines, file
  hits, and detected test/package hints.
- Added a compact "Next action" line and a one-shot long-running command
  notice.
- Moved summary construction into the capture path, gated temp-file fsync behind
  `KDS_DURABLE_LOGS=1`, and amortized stale-temp cleanup.
- Batched repeat state, sidecar, run index, latest-by-command, and metrics
  writes under one state lock.
- Switched repeat-failure state to sharded digest files and added exact plus
  normalized digests to sidecars.
- Added budgeted compact summaries with `--budget tight|normal|wide`, shorter
  unchanged repeat-failure output, copyable suggested KDS drilldown commands,
  and bounded error windows.
- Added live raw-mode teeing, character and approximate token savings, richer
  `kds gain` intelligence, `kds prune --before`, and retention controls for
  age and total local log bytes.
