# Privacy

KDS has no telemetry in V1.

KDS normal runtime commands make no network calls in V1. The explicit
`kds update check` command contacts GitHub to read the latest release metadata.
The public PowerShell bootstrap installer is fetched from a versioned tag, then
checks release metadata, downloads the matching release source archive and
`.sha256` file from GitHub, verifies the archive, and builds KDS locally. It
does not download a prebuilt binary and does not download or install
Rust/Cargo.

Default KDS runs are memory-only and do not write raw logs, temp stdout/stderr
files, sidecars, run indexes, or metrics. Saved artifact mode is available only
through explicit opt-in with `--save-artifacts` or `KDS_SAVE_ARTIFACTS=1`.
Saved raw logs are local only. They may contain secrets, local paths, usernames,
tokens, stack traces, environment values, and file contents. Review and redact
raw logs before sharing them.

Imported logs from default `kds summarize` runs are memory-only. In saved
artifact mode, imported logs are redacted before KDS writes the local import
artifact and before KDS writes sidecars, indexes, metrics, digest state, or
evidence output. This is still a guardrail, not proof that every possible
secret-like value was removed from imported content.

KDS summary, evidence, gain, doctor, log-index, log-stats, and clean commands
are designed not to print raw stdout/stderr bodies by default.

Default compact output also avoids absolute log and CWD paths. Saved artifact
output prints the run ID and local drilldown commands such as
`kds logs <id> --show-paths` instead of raw paths. Use
`--show-paths` only for local interactive output where path disclosure is
acceptable.

`kds doctor` is a local health check and may print local runtime, install, and
PowerShell profile paths. It still does not print raw stdout/stderr bodies.

KDS redacts common token, API key, password, bearer-token, URL credential, known
cloud-token, and keyed `.env`-style patterns from summaries, evidence, sidecars,
and indexes. Raw stdout/stderr bytes in saved raw logs and live `kds raw` output
are not redacted, but KDS writes the raw log command header from redacted argv.
Treat redaction as a guardrail, not as proof that every possible secret-like
value was removed.

On Unix, KDS creates runtime directories with `0700` permissions and log/state
files with `0600` permissions. If an existing `KDS_HOME` storage directory is
world-writable, KDS refuses to use it.

When artifacts are saved, KDS caps persisted raw stdout and stderr at 10 MiB
per stream by default. Set `KDS_MAX_RAW_BYTES` to a positive byte count such as
`1m` or `250000` to change that cap. KDS continues draining output after the cap
and writes a truncation note into the raw log plus truncation metadata into the
sidecar. Set `KDS_UNCAPPED_RAW_LOGS=1` when you intentionally want uncapped raw
persistence. For imported logs in saved artifact mode, KDS still scans the full
input for the summary while only persisting redacted imported bytes up to the
same limit.

Set `KDS_RETENTION_DAYS` or `KDS_MAX_TOTAL_LOG_BYTES` to remove old local KDS
artifacts automatically on run start. Set `KDS_COMPRESS_AFTER_DAYS` to gzip
older raw `.log` files and update matching sidecars to the compressed path.
After artifact deletion, KDS reconciles lookup state by removing index entries
whose sidecars are gone, rebuilding `latest-by-command`, and retiring digest
shards that point to removed raw logs. `kds gain` metrics remain lifetime
counters and report that scope explicitly.
