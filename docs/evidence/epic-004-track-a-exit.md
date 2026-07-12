# EPIC-004 Track A Exit Evidence

- Evidence date: 2026-07-12
- Evidence class: deterministic simulator application semantics
- Local host: macOS ARM64
- Public CI: Linux x86_64 and macOS ARM64
- Track status: Pass

## Scope boundary

Track A proves HomeMagic-owned domain, persistence, authorization, workflow,
RPC, event, command, restart, and failure semantics against the deterministic
controller simulator. It does not prove a production Matter stack, physical
radio or network behavior, BLE or Thread commissioning, IPv6/mDNS/CASE
interoperability, certification, protected cross-controller fabric import, or
compatibility with Nuki or any other physical device. Those remain Track B and
later EPIC-004 work, beginning with E4-008 and E4-009.

## Exit matrix

| Track A claim | Status | Executable evidence |
| --- | --- | --- |
| SDK-neutral bounded controller port | Pass | `controller_contract`, Matter boundary scan |
| Deterministic light and lock fixtures | Pass | normalized trace hash and randomized command-order contracts |
| Fabric create/export/restore durability | Pass | fabric workflow restart and disclosure contracts |
| Commissioning, projection, cancellation, removal | Pass | complete phase, atomic projection, cancellation rollback, and partial-cleanup contracts |
| Subscription health and repair | Pass | gap, retry deadline, exhaustion, restart, and stale-report contracts |
| Authenticated read and mutation RPC | Pass | Matter read/mutation API contracts and executable example catalog |
| Sensitive setup/export/restore isolation | Pass | endpoint allowlist, timeout, SQLite canaries, schema canaries |
| Common command boundary for light and lock | Pass | common command RPC parity, Matter adapter command mapping, exact unlock approval contracts |
| Actor-scoped durable operation events | Pass | creation/phase/rollback/reopen storage contract and exact API visibility filter |
| Restart yields explicit outcome | Pass | every simulator commissioning/removal checkpoint and subscription repair dispatch barrier |

## RPC and operator artifacts

- `docs/api/schemas/matter-rpc-reads-v1.json`
- `docs/api/schemas/matter-rpc-mutations-v1.json`
- `docs/api/schemas/json-rpc-envelope-v1.json`
- `docs/api/examples/matter-rpc-v1.json`
- `docs/architecture/matter-rpc.md`
- `docs/operations/matter-simulator-recovery.md`

The `matter_rpc_examples_should_match_every_executable_schema` test requires
exactly one request, success, and stable error example for every published read,
ordinary mutation, and sensitive method. It validates endpoint classification,
params, JSON-RPC envelopes, result envelopes, and numeric/data error pairs.

## Cross-platform evidence

| Slice | Public CI run | Linux x86_64 | macOS ARM64 |
| --- | --- | --- | --- |
| Read RPC and schemas | `29208029880` | Pass | Pass |
| Sensitive mutation handoff | `29208555337` | Pass | Pass |
| Atomic operation events | `29208961425` | Pass | Pass |
| Executable examples and exit procedures | `29209289949` | Pass | Pass |

All listed runs execute formatting, strict Clippy, the full Rust workspace
suite, historical migration fixtures, disclosure scan, and the committed
simulator trace hash. E4-007-05 and E4-007 closed only after the final run.

## Redaction statement

This report contains no setup material, export or recovery bytes, bearer
credentials, secret references, native network identifiers, device addresses,
or controller diagnostic text. IDs shown in the executable examples are fixed
non-operational placeholders.
