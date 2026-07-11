---
id: E2-004
epic: EPIC-002
title: Authenticate actors and evaluate default-deny policy
status: done
priority: critical
depends_on: [E2-002, E2-003]
adrs: [ADR-0013, ADR-0015]
created: 2026-07-11
updated: 2026-07-11
---

# E2-004: Actor Authentication and Policy

## Tasks

- [x] Add one-time token bootstrap and Argon2id verification.
- [x] Require actor authentication for HTTP RPC and WebSocket subscriptions.
- [x] Add actor disable/rotation and narrow capability/target/space grants.
- [x] Implement deterministic comfort, mechanical, and security policy rules.
- [x] Add per-actor/device rate and concurrency limits.
- [x] Persist explainable allow/deny decisions without token material.
- [x] Add authentication canaries and complete policy-matrix tests.

## Acceptance criteria

- [x] Request actor identity cannot be spoofed by parameters.
- [x] Default deny applies identically to RPC, internal, dry-run, and future MCP calls.
- [x] Mechanical/security commands require explicit risk-appropriate grants.

## Progress log

- 2026-07-11: Added a pure, versioned default-deny evaluator for actor state,
  action, capability, target, space, risk, freshness, constraints, rate, and
  device concurrency. Security risk accepts only an exact capability grant.
- 2026-07-11: Added sliding per-actor request limits and RAII per-device
  concurrency permits; focused policy and capacity tests pass.
- 2026-07-11: Added 256-bit one-time bearer bootstrap, Argon2id hash-only
  persistence, bounded off-executor verification, rotation, disable, and grant
  replacement. Authentication failures are deliberately indistinguishable.
- 2026-07-11: Protected HTTP RPC and WebSocket handshakes, reduced unauthenticated
  health to liveness, bound metadata causation to the authenticated actor, and
  added persistent rotation/disable/redaction plus transport-spoofing canaries.
