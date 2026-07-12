# EPIC-002 Exit Audit

- Audit date: 2026-07-12
- Local host: macOS `aarch64`
- Epic status: In progress pending state-changing hardware reports

## Acceptance criteria

| Criterion | Status | Evidence |
| --- | --- | --- |
| AC1 equivalent retry never repeats a physical action | Pass in automated fixtures | `execute_should_commit_each_fact_and_retry_without_redispatch`, `recovery_should_dispatch_only_pre_dispatch_states`, and bounded Shelly timeout/Digest fixtures |
| AC2 actor and allowed policy commit before dispatch | Pass | SQLite transition invariant rejects dispatch without allowed policy; `dry_run_and_policy_denial_should_never_dispatch`; authenticated RPC fixture |
| AC3 distinct rejection, acknowledgement, confirmation, timeout, and failure | Pass | typed command lifecycle/domain persistence tests, Shelly acknowledgement/confirmation fixtures, RPC serialization tests |
| AC4 common switch, dimmer, and cover commands | Pass in adapter fixtures; hardware pending | typed Switch/Light/Cover mapping tests and no public raw RPC path; state-changing reports not yet captured |
| AC5 unauthorized and unsafe work never reaches adapter | Pass | default-deny policy matrix, mechanical freshness/calibration constraints, zero-dispatch denial test |
| AC6 restart-safe command/audit causation | Pass | every non-terminal restart state, immutable ordered audit tests, and durable `CommandTransitioned` event projection |
| AC7 RPC, CLI, and internal callers share one service | Pass | SQLite-backed `command_rpc_should_share_internal_path_and_enforce_actor_ownership`, daemon composition, and query-based helper scripts |

## Exit items

| Exit item | Status | Evidence or unresolved condition |
| --- | --- | --- |
| Every acceptance criterion is linked | Pass | Table above |
| Required ADRs accepted and indexed | Pass | ADR-0013 through ADR-0016 and `docs/adr/README.md` |
| No public raw-command adapter bypass | Pass | `ShellyCommandAdapter` exposes only typed `CommandDispatcher` and `CommandConfirmation`; mapping tests reject mismatched capabilities |
| Hardware tests restore state and produce redacted reports | Pending | Cleanup-first harness and procedure exist, but physical execution has not been authorized or performed |
| Threat model and operator recovery cover shipped surface | Pass | `docs/security/command-control-threat-model.md`, `docs/operations/command-recovery.md`, and command hardware procedure |
| EPIC-003 and EPIC-004 consume finalized contracts | Pass | Explicit finalized EPIC-002 contract sections in both epic documents |
| macOS ARM full quality gate | Pass | Workspace tests/all features, strict Clippy, formatting, doc tests, helper syntax, and secret scan completed locally |
| Linux x86_64 full quality gate | Pass | Official `rust:1-bookworm` image under `x86_64-unknown-linux-gnu` Rust 1.97.0: format, strict all-target Clippy, complete workspace tests/all features, doc tests, and all migration fixtures passed |

## Automated safety evidence

- A command is inserted with its receipt before validation; allowed policy and
  `dispatched` are atomic durable transitions before adapter I/O.
- Equivalent idempotent retries return the existing aggregate. Conflicting
  payloads receive stable RPC error `-32023`.
- Recovery dispatches only `received`/`validated` commands. `dispatched` and
  `acknowledged` commands use observation-only confirmation.
- Toggle uses fresh observed state to materialize an explicit target before
  persistence and physical dispatch.
- The Shelly adapter performs one normal HTTP attempt, permits only the bounded
  Digest challenge exchange, and does not retry a timed-out physical command.
- Push observation is preferred; one bounded `GetStatus` read is the fallback.
- Cross-actor get/audit/cancel returns the same not-found response as absence.
- Secret values have no serialization path; fixture/evidence scanning is a
  mandatory repository and CI gate.

## Hardware evidence boundary

The existing read-only compatibility report proves macOS ARM discovery and
normalized state for firmware 1.7.5 switch (`S3SW-001P8EU`), dimmer
(`S3DM-0A101WWL`), and cover (`S3SW-002P16EU`, `SNSW-102P16EU`) devices. It does
not prove physical commands.

`scripts/hardware-command-smoke.py` is ready to create separate redacted switch,
dimmer, and cover command reports. It captures original state, executes cleanup
from `finally`, re-reads restored state, and fails the report when cleanup is not
verified. Cover execution requires an explicit physical-stop precondition and
sends stop before movement. Actual actuation remains pending operator approval
and physical supervision.

The 2026-07-12 dual-platform rerun also reconfirmed default-deny zero-dispatch,
durable actor/policy/audit facts, restart no-redispatch, typed Shelly mapping,
timeouts, and redaction. It does not substitute for physical movement or cleanup
evidence.
