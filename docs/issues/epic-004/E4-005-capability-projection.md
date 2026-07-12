---
id: E4-005
epic: EPIC-004
title: Project Matter devices and recover subscriptions safely
status: planned
priority: critical
depends_on: [E4-003, E4-004]
adrs: [ADR-0002, ADR-0009, ADR-0010, ADR-0034]
created: 2026-07-12
updated: 2026-07-12
---

# E4-005: Capability Projection and Subscription Recovery

## Outcome

Simulated Matter descriptors and reports become stable HomeMagic devices,
endpoints, capabilities, observations, and availability without leaking cluster
details or creating duplicate identities after change or restart.

## Tasks

- [ ] Parse bounded descriptor hierarchy, server/client roles, device types,
  cluster revisions, feature maps, and mandatory attribute availability.
- [ ] Project the simulated light to `on_off.v1` from applicable On/Off server
  semantics.
- [ ] Project the simulated lock to a versioned access-control capability from
  applicable Door Lock server semantics.
- [ ] Define later-fixture projection rules for Level Control to `level.v1` and
  Window Covering to constrained `position.v1` without enabling them from cluster
  presence alone.
- [ ] Preserve unmapped standard/vendor data as bounded, read-only, versioned,
  namespaced diagnostics.
- [ ] Persist projection revision and invalidate command assumptions after
  descriptor, feature, or command-support changes.
- [ ] Normalize attribute reports into observations with report/data version,
  source time, receive time, freshness, and causation.
- [ ] Reject stale/out-of-order reports while treating duplicates idempotently.
- [ ] Detect subscription loss, mark freshness explicitly, perform a bounded gap
  read, and resubscribe with bounded jitter/backoff.
- [ ] Recover projections and logical subscriptions after daemon restart without
  duplicating device, endpoint, or capability IDs.
- [ ] Bound wildcard/targeted subscriptions and sleepy-device reads.

## Acceptance criteria

- [ ] A light and lock appear through common capability queries only.
- [ ] Stable IDs survive label, address, session, and controller restart changes.
- [ ] Descriptor changes prevent commands from using stale feature assumptions.
- [ ] Subscription loss is visible and converges without reporting cached state
  as fresh.
- [ ] Unmapped data cannot become a public raw-write escape hatch.

## Verification

- [ ] Projection fixture matrix covers supported and malformed descriptors,
  missing optional data, features, revisions, and role mismatches.
- [ ] Duplicate, stale, reordered, and gap-read report tests pass.
- [ ] Restart/resubscription tests preserve identities and event order.
- [ ] Resource-bound and sleepy-device polling tests pass.
- [ ] Public JSON fixtures contain no cluster-write request type.

## Progress log

- 2026-07-12: Initial executable scope fixed to light and lock; level and cover
  projection rules remain explicit later-fixture work.
