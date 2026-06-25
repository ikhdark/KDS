# Privacy

KDS has no telemetry in V1.

KDS itself makes no network calls in V1. The public PowerShell bootstrap
installer fetches the KDS installer script and source archive from GitHub, then
builds KDS locally. It does not download a prebuilt binary.

Raw logs are local only. They may contain secrets, local paths, usernames,
tokens, stack traces, environment values, and file contents. Review and redact
raw logs before sharing them.

KDS summary, evidence, gain, doctor, and log-index commands are designed not to
print raw stdout/stderr bodies by default.

Default compact, logs, and evidence output also avoids absolute log and CWD
paths. It prints the run ID and local drilldown commands such as
`kds logs show <id> --show-paths` or `kds logs dir` instead. Use `--show-paths`
only for local interactive output where path disclosure is acceptable.

`kds doctor` is a local health check and may print local runtime, install, and
PowerShell profile paths. It still does not print raw stdout/stderr bodies.

KDS redacts common token, API key, password, bearer-token, and URL credential
patterns from summaries, evidence, sidecars, and indexes. Raw stdout/stderr
bytes in raw logs and `kds raw` output are not redacted, but KDS writes the raw
log command header from redacted argv. Treat redaction as a guardrail, not as
proof that every possible secret-like value was removed.

On Unix, KDS creates runtime directories with `0700` permissions and log/state
files with `0600` permissions. If an existing `KDS_HOME` storage directory is
world-writable, KDS refuses to use it.

Set `KDS_MAX_RAW_BYTES` to a positive byte count to cap persisted raw stdout
and stderr per stream. KDS continues draining output after the cap and writes a
truncation note into the raw log. Unset it or set it to `0` for unlimited raw
capture.
