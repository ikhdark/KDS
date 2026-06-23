# Security Policy

## Supported Versions

KDS is pre-1.0. Security fixes target the latest public release.

## Reporting a Vulnerability

Please report security issues privately to the project maintainers. Do not open
public issues containing secrets, exploit details, or sensitive logs.

## Log Privacy

KDS raw logs are local files and may contain secrets, usernames, paths, tokens,
stack traces, environment values, and file contents. Do not share raw logs
publicly without reviewing and redacting them.

Default compact, logs, and evidence output avoids absolute log and CWD paths.
Use `--show-paths` only for local interactive inspection. On Unix, KDS creates
runtime directories as `0700` and log/state files as `0600`, and refuses an
existing world-writable `KDS_HOME`.
