# Privacy

KDS has no telemetry in V1.

KDS normal runtime commands make no network calls in V1. The explicit
`kds update check` command contacts GitHub to read the latest release metadata.
The public PowerShell bootstrap installer is fetched from a versioned tag, then
checks release metadata, downloads the matching release source archive and
`.sha256` file from GitHub, verifies the archive, and builds KDS locally. It
does not download a prebuilt binary and does not download or install
Rust/Cargo.

By default, KDS does not save your command output. It only prints a short
summary. To save local troubleshooting files for later, run with
`--save-artifacts`.

Saved local troubleshooting files stay on your machine. They may contain
secrets, local paths, usernames, tokens, stack traces, environment values, and
file contents. Review and redact them before sharing them.

Imported logs from default `kds summarize` runs are not saved. Default imports
write aggregate `kds gain` metrics only. When you choose to save local
troubleshooting files, imported logs are redacted before KDS writes the local
import file and before KDS writes summary metadata files, saved-run lists,
metrics, repeat-failure tracking data, or evidence output. This is still a
guardrail, not proof that every possible secret-like value was removed from
imported content.

KDS summary, evidence, gain, doctor, log-index, log-stats, and clean commands
are designed not to print raw stdout/stderr bodies by default.

Default compact output also avoids absolute log and CWD paths. Saved
troubleshooting output prints the run ID and local detail commands such as
`kds logs <id> --show-paths` instead of raw paths. Use
`--show-paths` only for local interactive output where path disclosure is
acceptable.

## Advanced privacy details

Default KDS runs do not write raw logs, temp stdout/stderr files, summary
metadata files, saved-run lists, or repeat-failure tracking data. They do write
aggregate `kds gain` metrics such as raw/shown/saved line, character, and
approximate token counts, command kind, summary budget, and memory-only versus
saved-artifact counts. Default metrics do not keep run IDs, local paths,
sidecars, or command strings. Saved local troubleshooting files are available
only through explicit opt-in with `--save-artifacts` or
`KDS_SAVE_ARTIFACTS=1`.

`kds doctor` is a local health check and may print local runtime, install, and
PowerShell profile paths. It still does not print raw stdout/stderr bodies.

KDS redacts common token, API key, password, bearer-token, URL credential, known
cloud-token, and keyed `.env`-style patterns from summaries, evidence, summary
metadata files, and saved-run lists. Raw stdout/stderr bytes in saved raw logs
and live `kds raw` output are not redacted, but KDS writes the raw log command
header from redacted argv. Treat redaction as a guardrail, not as proof that
every possible secret-like value was removed.

On Unix, KDS creates runtime directories with `0700` permissions and log/state
files with `0600` permissions. If an existing `KDS_HOME` storage directory is
world-writable, KDS refuses to use it.

When local troubleshooting files are saved, KDS caps persisted raw stdout and
stderr at 10 MiB per stream by default. Set `KDS_MAX_RAW_BYTES` to a positive
byte count such as `1m` or `250000` to change that cap. KDS continues draining
output after the cap and writes a truncation note into the raw log plus
truncation metadata into the summary metadata file. Set
`KDS_UNCAPPED_RAW_LOGS=1` when you intentionally want uncapped raw persistence.
For imported logs in saved-file mode, KDS still scans the full input for the
summary while only persisting redacted imported bytes up to the same limit.

Set `KDS_RETENTION_DAYS` or `KDS_MAX_TOTAL_LOG_BYTES` to remove old saved local
troubleshooting files automatically on run start. Set
`KDS_COMPRESS_AFTER_DAYS` to gzip older raw `.log` files and update matching
summary metadata files to the compressed path. After saved-file deletion, KDS
reconciles lookup state by removing saved-run list entries whose summary
metadata files are gone, rebuilding `latest-by-command`, and retiring
repeat-failure tracking data that points to removed raw logs. `kds gain`
metrics remain lifetime counters and report that scope explicitly, including
how many counted runs had saved artifacts.
