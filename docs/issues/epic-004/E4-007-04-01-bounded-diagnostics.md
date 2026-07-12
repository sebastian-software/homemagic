---
id: E4-007-04-01
epic: EPIC-004
parent: E4-007-04
title: Expose bounded redacted Matter diagnostics
status: ready
priority: high
depends_on: [E4-007-03]
adrs: [ADR-0013, ADR-0033, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-007-04-01: Bounded Redacted Diagnostics

## Outcome

Authenticated readers can inspect bounded durable fabric, node, endpoint,
operation, and subscription health without invoking controller writes or
receiving native identifiers, network material, setup data, secret references,
or raw SDK objects.

## Tasks

- [ ] Define versioned secret-free diagnostic summary DTOs.
- [ ] Revalidate exact installation-scoped `matter_read` authority per request.
- [ ] Combine durable fabric, node, endpoint, operation, repair, and
  subscription health into deterministic bounded pages.
- [ ] Include controller availability only as normalized counts and timestamps.
- [ ] Expose freshness and explicit repair eligibility without mutating state.

## Acceptance criteria

- [ ] Diagnostics contain no state-changing controller path.
- [ ] Foreign resources are indistinguishable from missing resources.
- [ ] Pagination and ordering remain stable after reopen.
- [ ] DTOs contain no raw native, network, setup, secret, or SDK fields.

## Verification

- [ ] Empty, populated, bounded, foreign, disabled-actor, and reopen tests pass.
- [ ] Diagnostic JSON and secret-canary scans remain clean.

## Progress log

- 2026-07-12: E4-007-03 completed with public cross-platform CI. This child is
  ready.
