# Validation

Run:

```powershell
cargo check
cargo test
cargo run -- --help
cargo run -- --version
cargo run -- gain
cargo run -- doctor
cargo run -- logs dir
cargo run -- -- node --version
cargo run -- raw -- node --version
cargo run -- hook status
cargo run -- hook doctor
```

Run a failing command twice:

```powershell
cargo run -- -- pwsh -NoProfile -Command "Write-Error 'KDS synthetic failure'; exit 7"
```

Verify exit code preservation, raw log creation, sidecar creation, index
append, compact summary, repeated "same failure signal" wording, safe drilldown,
and no mutation of real global config during tests.

To validate corrupt-state resilience, write a malformed line to a temp
`state/runs.jsonl` and run `kds doctor`. Doctor should report the malformed
line count without creating logs or printing raw stdout/stderr bodies.

To validate runtime resilience, use temp `KDS_HOME` runs to verify missing
commands still create a raw log, sidecar, and index entry; parallel wrapped
commands append valid `runs.jsonl` lines; and stale `*.tmp` files under the KDS
logs tree are cleaned up only after they are old enough to be considered
abandoned.
