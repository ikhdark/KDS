# Validation

Run:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1 --dry-run
pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1 --dry-run
cargo check
cargo test
cargo run -- --help
cargo run -- --version
cargo run -- gain
cargo run -- doctor
cargo run -- logs dir
cargo run -- logs stats
cargo run -- gc --older-than 30d --dry-run
cargo run -- prune --before 30d --dry-run
cargo run -- -- node --version
cargo run -- run --budget tight -- node --version
cargo run -- raw -- node --version
cargo run -- hook status
cargo run -- hook doctor
```

Run a failing command twice:

```powershell
cargo run -- -- pwsh -NoProfile -Command "Write-Error 'KDS synthetic failure'; exit 7"
```

Verify exit code preservation, raw log creation, sidecar creation, index
append, latest-by-command update, digest shard creation for failures, compact
summary, repeated "same failure signal" wording, safe drilldown, and no
mutation of real global config during tests.

To validate corrupt-state resilience, write a malformed line to a temp
`state/runs.jsonl` and run `kds doctor`. Doctor should report the malformed
line count, Desktop hook install/trust status, and hooks.json parse health
without creating logs or printing raw stdout/stderr bodies.

To validate runtime resilience, use temp `KDS_HOME` runs to verify missing
commands still create a raw log, sidecar, and index entry; parallel wrapped
commands append valid `runs.jsonl` lines; and stale `*.tmp` files under the KDS
logs tree are cleaned up only after they are old enough to be considered
abandoned.

To validate summary behavior, run a failing command with `--budget tight`,
`--budget normal`, and `--budget wide`; run the same failure twice and confirm
the repeat output is shorter; then check `kds logs show <id> --error-window`
and `kds logs show last --error-window`.
Raw mode should tee command output live while still writing local artifacts.

To validate exact-output passthrough, run proof-style Git commands through
`kds --` with a temp `KDS_HOME`: `git status`, `git rev-parse`,
`git hash-object`, `git diff ...`, and `git log --oneline` should print native
Git output and should not create KDS artifacts.

To validate retention, create old `.log` and `.summary.json` files under a temp
KDS logs directory, run `kds gc --older-than 30d --dry-run` and
`kds prune --before 30d --dry-run`, then run the same command without
`--dry-run`. KDS should only remove KDS artifacts under the logs tree. Also
validate `KDS_RETENTION_DAYS` and `KDS_MAX_TOTAL_LOG_BYTES` against temp
storage.
