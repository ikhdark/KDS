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
cargo run -- logs
cargo run -- logs --show-paths
cargo run -- clean --older-than 30d
cargo run -- update --help
cargo run -- -- node --version
cargo run -- raw -- node --version
"error: synthetic failure" | cargo run -- summarize --name synthetic-ci --exit-code 1
cargo run -- summarize --file .\README.md --name readme-log --exit-code 0
cargo run -- run --save-artifacts -- node --version
"error: synthetic failure" | cargo run -- summarize --save-artifacts --name synthetic-ci --exit-code 1
cargo run -- hook status
cargo run -- hook doctor
```

Run a failing command twice:

```powershell
cargo run -- -- pwsh -NoProfile -Command "Write-Error 'KDS synthetic failure'; exit 7"
```

Verify exit code preservation, compact summaries, default memory-only behavior,
saved-artifact opt-in behavior, raw log creation only when artifacts are saved,
sidecar creation, index append, latest-by-command update, digest shard creation
for saved failures, repeated "same failure signal" wording, safe drilldown, and
no mutation of real global config during tests.

Bootstrap dry-run should print the versioned release source archive and checksum
URL, skip the update check, and avoid downloading files. Installer dry-run
should avoid file writes, PATH edits, builds, Rust/Cargo installation, profile
edits, and Codex Desktop hook edits. With Cargo absent from PATH, bootstrap and
installer flows should fail clearly before any source download or build attempt.
Non-dry-run bootstrap output should print installed and latest release versions
before downloading source. `kds update check` is the only runtime update check
and is an explicit network opt-in.

Hook profile validation should cover both the managed PowerShell hook and Codex
Desktop hook matcher. Verify built-in profiles for JavaScript/TypeScript,
Python, Go, Java/Kotlin, .NET, PHP, Ruby, Elixir, C/C++, and task runners, plus
native passthrough for deploy/publish/watch/dev-style commands.

To validate corrupt-state resilience, write a malformed line to a temp
`state/runs.jsonl` and run `kds doctor`. Doctor should report the malformed
line count, Desktop hook install/trust status, and hooks.json parse health
without creating logs or printing raw stdout/stderr bodies.

To validate runtime resilience, use temp `KDS_HOME` runs to verify default runs
do not create `logs`, `state`, raw logs, temp stdout/stderr files, sidecars,
indexes, or metrics. For saved-artifact validation only, run with
`--save-artifacts` or `KDS_SAVE_ARTIFACTS=1` and verify missing commands create
a raw log, sidecar, and index entry; parallel saved wrapped commands append
valid `runs.jsonl` lines; and stale `*.tmp` files under the KDS logs tree are
cleaned up only after they are old enough to be considered abandoned.

To validate summary behavior, run a failing command twice and confirm default
output says artifacts were not saved. For saved-artifact repeat tracking and
drilldown validation, run the same failure twice with `--save-artifacts`; then
check `kds logs <id> --error-window` and `kds logs last --error-window`.
Raw mode should tee command output live while staying memory-only by default.
Set `KDS_SAVE_ARTIFACTS=1` and `KDS_MAX_RAW_BYTES=5` for a temp `KDS_HOME` run
and verify the raw log contains a truncation note and the sidecar records
truncation metadata. Set `KDS_UNCAPPED_RAW_LOGS=1` only when validating
intentional uncapped persistence.

To validate imported log behavior, pipe a small synthetic failure into
`kds summarize --name synthetic-ci --exit-code 1` with temp `KDS_HOME`.
Confirm that KDS exits successfully and does not create `logs` or `state`.
For saved-artifact import validation, run with `--save-artifacts`, confirm it
records exit code `1` in the sidecar, writes a redacted imported log artifact,
updates metrics/index state, and that `kds evidence last` does not print raw
imported bodies or secrets.

To validate exact-output passthrough, run proof-style Git commands through
`kds --` with a temp `KDS_HOME`: `git status`, `git rev-parse`,
`git hash-object`, `git diff ...`, and `git log --oneline` should print native
Git output and should not create KDS artifacts.

To validate retention, create old `.log` and `.summary.json` files under a temp
KDS logs directory, then run `kds clean --older-than 30d`. KDS should only remove
KDS artifacts under the logs tree and should reconcile state by removing index
entries whose sidecars are gone, rebuilding `latest-by-command`, and retiring
digest shards that point to removed raw logs. Also validate `KDS_RETENTION_DAYS`
and `KDS_MAX_TOTAL_LOG_BYTES` against temp storage.
