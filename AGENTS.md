# KDS Project Instructions

KDS is a standalone public Rust CLI. It is not part of `codexKD2-main`.

## Product Rules

- KDS is automatic-hook-first: installing KDS installs the binary and the
  automatic allowlisted hook.
- KDS must not install, vendor, shell out to, copy, or depend on RTK.
- RTK may be mentioned only as install/adoption UX inspiration.
- Full raw logs are local archival data; summaries, sidecars, and indexes are
  the product surface.
- Never add telemetry in V1.
- Never add network calls in V1 except future explicit release installer
  downloads with checksum verification.

## Safety Rules

- Raw logs may contain secrets, paths, usernames, tokens, stack traces,
  environment values, and file contents.
- Do not print raw stored logs in V1.
- `doctor` and `hook doctor` are read-only and must not create runtime
  directories.
- Use `KDS_HOME` in tests so real user logs/metrics are not touched.
- Use temp `CODEX_HOME` and temp PowerShell profile paths in tests.
- Do not mutate the real global Codex config, global PATH, real PowerShell
  profile, Codex Desktop settings, `codexKD2-main`, or `codexKD2-devtools`
  during tests.

## Contract-First Readiness Guardrail

### Summary

Readiness is computed from evidence, not confidence. Use this guardrail for
KDS implementation, local install or publish preparation, installed binary
replacement, hook activation claims, runtime verification, or readiness
claims. Do not use it for answer-only, review-only, ranking, planning, or
explanation turns unless the user asks for edits or a readiness verdict.

Publish-ready means the combined diff is ready for the KDS publish or install
route being claimed. It does not mean published, installed, shell-visible,
Desktop-visible, or runtime verified.

### Continue After Reporting

Reporting in is a checkpoint, not a stopping point. After sending a progress,
validation, Wiring Proof, or readiness report that is not the final answer,
continue automatically to the next already-approved in-contract action.

Do not ask the user to say "continue" when the next step is already covered by
the approved contract. Stop only when approval is required, the contract must
expand, the user explicitly pauses or stops the work, a true blocker leaves no
safe in-contract action, or the requested final answer has been delivered.

### Contract First

Before patching, produce an Implementation Contract and wait for approval.

The contract must include:

- contract version and diff baseline
- baseline changed-file inventory
- every changed file assigned to exactly one bucket
- out-of-contract files present: yes/no
- contract IDs, e.g. `C1`, `C2`, `C3`
- agreed behaviors and non-goals
- affected KDS entry points and owner modules/files
- old paths to remove, delegate, or prove unchanged
- required test matrix per contract ID
- required validation commands from nearest owner guidance; use
  `docs/VALIDATION.md` when no narrower guidance exists
- cleanup review criteria

If a new behavior category appears while editing, stop, increment the contract
version, explain the expansion, and wait for approval before coding it.

### Inventory And Freshness

Changed-file inventory source:

```powershell
git diff --name-only HEAD --
git status --short
```

Required inventory points:

```text
Baseline changed-file inventory:
<from git before patching>

Final changed-file inventory:
<from git after final edit>
```

Tracked diff hash command:

```powershell
git diff --binary HEAD -- | git hash-object --stdin
```

Every validation report must include:

```text
Validated tracked diff hash:
Current tracked diff hash:
Tracked diff fresh: yes/no

Validated changed-file inventory:
Current changed-file inventory:
Inventory fresh: yes/no

Fresh overall: yes/no
```

If either the tracked diff hash or changed-file inventory differs from the
validated snapshot, validation is stale.

### File Buckets

Every changed file must be assigned to exactly one bucket.

A changed file may be marked user-owned unrelated only when:

- it was already present in the baseline inventory before patching;
- Codex did not modify it during the task;
- it is not required by any contract ID, validation command, generated
  artifact, publish route, install route, or runtime verification route.

User-owned unrelated files must be excluded from the readiness claim.
Codex-owned out-of-contract files block combined readiness.

### Contract IDs And Tests

Every agreed behavior must have a contract ID before patching:

```text
C1: <behavior>
C2: <behavior>
C3: <behavior>
```

For each contract ID, define required tests before editing:

- reject tests
- allow tests
- ambiguous-but-valid allow tests where relevant
- entry-point tests
- regression tests where relevant

Every guard, parser, policy, hook, installer, storage, log-safety, or command
routing change needs both reject and allow coverage.

### Wiring Proof

Before any implementation claim, produce a Wiring Proof table:

```text
Contract ID:
Behavior:
Code location:
Entry points:
Old paths:
Required tests:
Test status: added/existing
Validation fresh: yes/no
Cleanup blocker: yes/no
Ready: yes/no
```

No contract row can be ready unless it has code, entry-point, old-path, test,
fresh-validation, and cleanup proof.

### Validation Waivers

If the user explicitly waives tests, report:

```text
Focused validation waived by user: yes
Commands not run:
Non-test checks run:
```

A validation waiver may allow patch delivery, but it does not allow
publish-ready unless the waived validation is not required for the selected
readiness target.

### Readiness Targets

- **Implementation-ready**: every contract ID is wired, tested, freshly
  validated, cleanup-reviewed, and every changed file is bucketed.
- **Publish-ready**: implementation-ready plus publish-route or install-route
  proof for the affected KDS surface. On Windows, the product install route is
  `.\scripts\install.ps1`. On Unix, `./scripts/install.sh --binary-only` is
  only a development/manual binary helper because Unix automatic shell hooks
  are not implemented in V1; do not use it as proof of automatic-hook-first
  product installation.
- **Published**: final publish or install command ran and target binary
  replacement was proven. For KDS local install claims, use the target path
  printed by the installer as `Binary:` or `Wrote:`; do not assume a hardcoded
  installed binary path.
- **Runtime verified**: the relevant running `kds` path is proven to use the
  expected local artifact, with restart, reload, or new-shell proof when a hook
  or PATH claim depends on refreshed process state.
- **Desktop-visible**: a subtype of runtime-verified. KDS must not mutate
  Codex Desktop settings. If the claim is specifically Desktop-visible,
  runtime visibility proof must include the Desktop/VS Code/app-server process
  path, restart/reload proof, hook/profile evidence where relevant, and
  evidence that the running process uses the expected local KDS binary.

### Status Ladder

Final status must be exactly one of:

- Contract drafted, not patched
- Patched, not wired
- Wired, not validated
- Focused validated, cleanup pending
- Implementation-ready, publish pending
- Publish-ready
- Published
- Runtime verified

### Hard Rules

- No approved contract -> no patch.
- No baseline inventory -> no approved contract.
- No final inventory -> no Wiring Proof.
- No Wiring Proof -> do not say implemented, complete, correctly wired, fixed,
  or publish ready.
- No fresh diff hash + inventory match -> no publish-ready claim.
- No cleanup review -> no publish-ready claim.
- Out-of-contract files present: yes -> combined readiness must be no.
- No final publish/install command + target binary proof -> do not say
  published.
- No runtime path proof -> do not say runtime verified.
- No Desktop/VS Code/app-server process-path proof plus local-binary proof ->
  do not say Desktop-visible.

### Final Readiness Format

Every readiness report must end with:

```text
Combined readiness: yes/no
Readiness target: implementation-ready / publish-ready / published / runtime-verified
Reason:
- contract coverage: yes/no
- changed-file inventory fresh: yes/no
- tracked diff hash fresh: yes/no
- required validation fresh: yes/no
- cleanup review done: yes/no
- publish route proof: yes/no/not applicable
- runtime visibility proof: yes/no/not applicable
Out-of-contract files present: yes/no
Final status: <one status from ladder>
```
