# EPIC-003 Automation Engine Implementation Plan

- Date: 2026-07-11
- Status: Active
- Design: [Agent-Authored Automation Engine](../superpowers/specs/2026-07-11-agent-authored-automation-engine-design.md)
- Issue index: [EPIC-003 issues](../issues/epic-003/README.md)

## Delivery rules

- Implement issues in dependency order and keep frontmatter/checklists current in
  the same commit as evidence.
- Use English for code, ADRs, issues, API documentation, and commit messages.
- Use Conventional Commits and create at least one independently verifiable
  commit per issue.
- Keep automation logic inside the modular monolith and route every physical
  action through `CommandService`.
- Never mark an issue done from narrow tests when its acceptance criteria require
  storage, restart, RPC, or end-to-end evidence.

## Sequence

### 1. E3-001 decisions

Accept ADR-0017 through ADR-0020 for IR compatibility, deterministic scheduling,
Safety Profiles/approval, and retention. Update the ADR index and epic links.

Verification: decision consistency review, placeholder scan, Markdown links.

### 2. E3-002 domain and schema

Add stable automation/run/occurrence identities, immutable document versions,
typed IR constructs, lifecycle state machines, normalized plan and trace
contracts, canonical serialization, bounds, and published schema/examples.

Primary targets: `homemagic-domain`, `docs/api`, `docs/evidence/fixtures`.

Verification: round trips, invalid transitions, schema fixtures, canonical hash
stability, persisted-contract tests, property tests.

### 3. E3-003 validation and compilation

Implement structural validation, stable reference resolution, type checking,
Safety Profile aggregation, exact JSON Pointer errors, bounded plan compilation,
cycle/impossible-branch detection, and desired-state reduction.

Primary targets: `homemagic-application` with domain-owned output contracts.

Verification: missing/ambiguous/stale/incompatible reference matrices, bound
tests, reducer invariants, deterministic plan snapshots.

### 4. E3-004 durable persistence

Add a forward-only automation migration and application-owned repository port for
identities, immutable versions, evidence, active pointers, occurrences, runs,
timers, queues, trace, atomic rollback, recovery, and independent retention.

Primary targets: `homemagic-application`, `homemagic-storage`.

Verification: migration fixtures, rollback/reopen tests, optimistic draft
conflicts, append-only trace ordering, recovery queries, retention protection.

### 5. E3-005 virtual-time simulator

Introduce clock/scheduler and state/command-evaluation ports with virtual
implementations. Run normalized plans without a physical dispatcher and emit
byte-stable traces for schedules, DST, delays, waits, retries, branches,
parallel/race groups, reduction, and policy outcomes.

Primary targets: `homemagic-application`, fixture snapshots.

Verification: repeated byte-equivalent traces and construction tests proving no
adapter path exists.

### 6. E3-006 durable runtime

Implement the step interpreter, durable run coordinator, event subscription,
timers, missed/skipped occurrences, explicit catch-up, run modes, suppression,
restart semantics, isolation, and exclusive `CommandService` submission.

Primary targets: `homemagic-application`, `homemagic`, storage contracts.

Verification: virtual and real clock parity, restart matrices, queue/parallel
bounds, self-loop suppression, interrupted command recovery, fault isolation.

### 7. E3-007 governance and RPC

Implement authenticated draft/version, validate/simulate, approve/reject,
activate/rollback/disable/retire, run/trace/cancel, and explicit catch-up RPCs.
Derive Actor identity exclusively from authentication and stream durable
automation/run transitions over the existing event channel.

Primary targets: `homemagic-api`, daemon composition, API/operator docs.

Verification: SQLite-backed RPC/internal parity, actor isolation, optimistic
conflicts, exact-hash evidence gates, atomic active pointer, error mapping.

### 8. E3-008 exit audit

Complete operator recovery/rollback/stuck-run guidance, redacted fixtures,
retention evidence, macOS ARM gates, Linux x64 CI evidence, threat-model update,
EPIC-005 contract handoff, and criterion-by-criterion exit audit.

## Repository-wide gate per issue

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
./scripts/scan-secrets.sh
git diff --check
```
