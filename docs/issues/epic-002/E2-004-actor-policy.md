---
id: E2-004
epic: EPIC-002
title: Authenticate actors and evaluate default-deny policy
status: in_progress
priority: critical
depends_on: [E2-002, E2-003]
adrs: [ADR-0013, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-004: Actor Authentication and Policy

## Tasks

- [ ] Add one-time token bootstrap and Argon2id verification.
- [ ] Require actor authentication for HTTP RPC and WebSocket subscriptions.
- [ ] Add actor disable/rotation and narrow capability/target/space grants.
- [x] Implement deterministic comfort, mechanical, and security policy rules.
- [x] Add per-actor/device rate and concurrency limits.
- [x] Persist explainable allow/deny decisions without token material.
- [ ] Add authentication canaries and complete policy-matrix tests.

## Acceptance criteria

- [ ] Request actor identity cannot be spoofed by parameters.
- [ ] Default deny applies identically to RPC, internal, dry-run, and future MCP calls.
- [x] Mechanical/security commands require explicit risk-appropriate grants.

## Progress log

- 2026-07-11: Added a pure, versioned default-deny evaluator for actor state,
  action, capability, target, space, risk, freshness, constraints, rate, and
  device concurrency. Security risk accepts only an exact capability grant.
- 2026-07-11: Added sliding per-actor request limits and RAII per-device
  concurrency permits; focused policy and capacity tests pass.
