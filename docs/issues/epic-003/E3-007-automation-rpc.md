---
id: E3-007
epic: EPIC-003
title: Expose governed automation lifecycle RPCs
status: in_progress
priority: high
depends_on: [E3-004, E3-005, E3-006]
adrs: [ADR-0003, ADR-0013, ADR-0016, ADR-0019, ADR-0030]
created: 2026-07-11
updated: 2026-07-12
---

# E3-007: Automation RPC

## Tasks

- [x] Add authenticated draft, update, get, list, and versions methods.
- [x] Add validate and deterministic simulate methods.
- [x] Add approve/reject and exact-evidence activation gates.
- [x] Add atomic activate, rollback, disable, and retire methods.
- [x] Add run list/get, trace cursor, cancel, and explicit catch-up methods.
- [ ] Derive Actor exclusively from authentication and enforce ownership/grants.
- [ ] Stream durable lifecycle/run transitions on the existing event channel.
- [ ] Add stable error mappings and bounded filters/cursors.
- [ ] Document executable agent-oriented examples without hand-built internal IDs.
- [ ] Test RPC/internal parity, isolation, conflicts, governance, and redaction.

## Acceptance criteria

- [ ] Complete lifecycle management is possible solely through RPC.
- [ ] Sensitive profiles cannot activate without explicit version approval.
- [ ] Comfort and constrained comfort-motion follow the simple auto-ready rule.
- [ ] Tokens, secrets, vendor payloads, and untrusted actor fields never leak.

## Progress

- 2026-07-12: Accepted ADR-0030 and added the transport-independent
  `AutomationLifecycleService`. SQLite evidence proves actor ownership,
  optimistic draft conflicts, exact validation evidence, data-only simulation
  with internally derived IDs, automatic comfort readiness, and atomic exact
  activation. RPC query/transition methods remain open.
- 2026-07-12: Added the first authenticated JSON-RPC lifecycle surface for
  draft put/get, validation, version get, deterministic simulation,
  approve/reject, exact activation, and explicit catch-up. Production uses the
  same lifecycle and scheduler instances as the engine. RPC/internal parity
  evidence proves an extra untrusted `actor_id` cannot override the bearer
  actor, and stable error mappings omit repository and simulation internals.
- 2026-07-12: Added bounded newest-first repository and lifecycle queries for
  actor-owned drafts, immutable versions, and runs plus run-local trace cursor
  reads. JSON-RPC now exposes draft/version lists and run get/list/trace without
  cross-actor visibility; query limits clamp to 1..100.
- 2026-07-12: Added optimistic operational transitions and RPCs for exact
  rollback, disable, and permanent retire; storage now rejects any attempt to
  reactivate a retired identity. Actor-owned run cancellation atomically
  appends its terminal outcome and cancels all pending/ready timers, with trace
  sequencing recovered through bounded cursor pages.
