---
id: E4-007-05-01
epic: EPIC-004
parent: E4-007-05
title: Publish Matter RPC contracts and authenticated reads
status: done
priority: high
depends_on: [E4-007-04]
adrs: [ADR-0003, ADR-0013, ADR-0016, ADR-0041, ADR-0042]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-05-01: RPC Contracts and Reads

## Outcome

The router composes Matter services explicitly and exposes versioned,
authenticated, bounded, redacted fabric, operation, node, subscription, and
diagnostic read methods with stable transport errors.

## Tasks

- [x] Add a Matter service bundle to API state without global singletons.
- [x] Define `matter.*.v1` params, result envelopes, and stable error mapping.
- [x] Implement fabric, operation list/get, node list/get, and diagnostics reads.
- [x] Reject actor, policy, raw cluster, attribute, command, and oversized params.
- [x] Publish machine-readable JSON schemas for every read method.

## Acceptance criteria

- [x] Actor context always comes from bearer authentication.
- [x] Foreign operation/node reads are indistinguishable from missing.
- [x] DTOs and schemas contain no setup, secret, SDK, or native network fields.

## Verification

- [x] Happy, empty, invalid, denied, foreign, bounded, and reopen RPC tests pass.
- [x] Serialized schemas pass secret and raw-mutation canaries.
- [x] Full local workspace, strict Clippy, boundary, and secret-scan gates pass.
- [x] Public Linux x86_64/macOS ARM64 CI passes for the committed slice.

## Progress log

- 2026-07-12: Implemented explicit `MatterApiServices` composition, six strict
  authenticated read methods, stable errors, committed executable schemas, and
  a SQLite-backed actor/bounds/reopen contract. The all-feature workspace,
  strict Clippy, Matter boundaries, and secret scan pass in commits `0ef7dab`
  and `48a500f`.
- 2026-07-12: Public CI run `29208029880` passed Linux x86_64 Rust quality and
  Linux x86_64/macOS ARM64 Matter simulator verification. This issue is done;
  E4-007-05-02 is ready.
