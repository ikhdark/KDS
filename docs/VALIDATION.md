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
