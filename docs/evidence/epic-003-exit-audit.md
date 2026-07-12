# EPIC-003 Exit Audit

- Audit date: 2026-07-12
- Local host: macOS `arm64`
- Secondary host: Linux `x86_64` under an isolated official Rust container
- Epic status: Done

## Acceptance criteria

| Criterion | Status | Evidence |
| --- | --- | --- |
| AC1 complete lifecycle solely through RPC | Pass | `automation_create_rpc_should_generate_every_envelope_field` plus lifecycle RPC routes for create/update/get/list, validate, simulate, approve/reject, activate/rollback/disable/retire, run/trace/cancel, and catch-up |
| AC2 deterministic simulation | Pass | `identical_fixture_should_emit_byte_equivalent_trace`, timezone/DST and equal-time ordering fixtures |
| AC3 invalid references and types block activation with paths | Pass | compiler missing/ambiguous/stale/incompatible/type tests and RPC `-32044` findings |
| AC4 simulation cannot dispatch | Pass | simulator has no dispatcher construction; all command outcomes are input data |
| AC5 runtime uses shared command/policy/idempotency/audit | Pass | runtime command dependencies contain `CommandService` only; command crash-window and retry contracts |
| AC6 runs retain immutable version and causation | Pass | persisted run contract, command causation propagation, trace/event cursor tests |
| AC7 rollback restores an older ready pointer atomically | Pass | lifecycle service and SQLite operational transition contracts |

## Exit items

| Exit item | Status | Evidence |
| --- | --- | --- |
| Required ADRs accepted and indexed | Pass | ADR-0017 through ADR-0032 |
| IR schema/examples and compatibility rules published | Pass | `automation.document.v1` and `automation.plan.v1` schemas, full authored fixture, server-generated draft request |
| No arbitrary-code or raw-adapter escape | Pass | closed IR enums, compiler bounds, simulator construction test, runtime `CommandService` dependency |
| Operator recovery covers stuck runs, disable, rollback, trace, and catch-up | Pass | `docs/operations/automation-recovery.md` |
| Threat model covers shipped surface | Pass | `docs/security/automation-threat-model.md` |
| EPIC-005 consumes finalized contracts | Pass | finalized contract section in EPIC-005 |
| macOS ARM quality gate | Pass | format, strict all-target Clippy, full workspace tests/all features, migration fixtures, and secret scan |
| Linux x86_64 quality gate | Pass | official `rust:1-bookworm` image, `x86_64-unknown-linux-gnu` Rust 1.97.0 under QEMU; format, strict all-target Clippy, full workspace tests/all features, doc tests, and all five migration fixtures passed |

## End-to-end authored evidence

The redacted executable request
`docs/api/examples/automation-draft-create-v1.json` is parsed by
`automation_create_rpc_should_generate_every_envelope_field`. The test proves
that the server generates schema, identity, version, authenticated author, and
timestamp, then validates the schedule-only draft, recovers its operational
revision through get/list, and hides it from another actor.

`docs/api/examples/automation-document-v1.json` covers every authored IR
construct and validates against the published schema.
`docs/evidence/fixtures/automation-simulation-v1.json` records deterministic
synthetic history and expected trace evidence without device credentials,
network locations, native IDs, or vendor payloads.

## Runtime and governance evidence

- Version insertion, every version state edge, operational pointer changes, and
  run state edges append typed events in the same SQLite transaction.
- Comfort and constrained comfort-motion become ready after successful
  simulation; resolved security risk requires exact immutable approval.
- Event, schedule, timer, queue, restart, and command identities are stable
  across retries and process restart.
- Same-segment repeated targets reduce to the last desired state, while delay,
  wait, branch, event, and dispatch boundaries preserve intentional sequences.
- Automatic missed-run replay does not exist; catch-up is one exact,
  authenticated, idempotent request.
- Automation event delivery is owner-filtered and contains no document,
  rationale, plan, trace, credential, vendor, or untrusted transport payload.

## Commands executed

macOS ARM:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p homemagic-storage --test migration_fixtures
./scripts/scan-secrets.sh
```

Linux x86_64 uses the same commands with `--locked` inside a read-only repository
mount and a container-local target directory.

The full Linux workspace suite already included
`migration_fixtures` with all features. A redundant second feature-minimal
migration build was stopped after that result; it is not used as evidence.
The repository-content secret scan passed on the identical checkout during the
final macOS gate.
