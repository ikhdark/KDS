# Privacy

KDS has no telemetry in V1.

KDS makes no network calls in V1 except future explicit release installer
downloads. Future release-download installers must verify checksums before
installing binaries.

Raw logs are local only. They may contain secrets, local paths, usernames,
tokens, stack traces, environment values, and file contents. Review and redact
raw logs before sharing them.

KDS summary, evidence, gain, doctor, and log-index commands are designed not to
print raw stdout/stderr bodies by default.
