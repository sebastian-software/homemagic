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

- [ ] Add authenticated draft, update, get, list, and versions methods.
- [ ] Add validate and deterministic simulate methods.
- [ ] Add approve/reject and exact-evidence activation gates.
- [ ] Add atomic activate, rollback, disable, and retire methods.
- [ ] Add run list/get, trace cursor, cancel, and explicit catch-up methods.
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
