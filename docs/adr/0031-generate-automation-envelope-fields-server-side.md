# ADR-0031: Generate automation envelope fields server-side

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0003, ADR-0013, ADR-0030, E3-007

## Context

Agents should author automations from user intent and semantic device aliases.
Requiring them to manufacture automation IDs, version numbers, actor IDs,
schema discriminators, or creation timestamps makes the RPC harder to use and
creates opportunities to spoof ownership or violate lifecycle invariants.

Existing full-document updates remain useful because their optimistic revision
contract makes the exact authored document explicit.

## Decision

Add a separate `automations.drafts.create` method. Its input contains only
authored behavior and provenance text. The authenticated lifecycle service
generates the automation ID, version one, schema version, author ID, and
creation timestamp.

Full-document `automations.drafts.put` remains the optimistic update method.
Both methods converge on the same application service and repository path.
Device targets may use aliases so the initial request does not require durable
device IDs.

## Consequences

- A new automation can be authored without copying any internal identifier.
- Actor ownership cannot be supplied or overridden by request JSON.
- Server-owned fields have one canonical creation path.
- Agents must retain the returned automation ID for later lifecycle calls.
- Updating an existing draft continues to require its exact document and
  optimistic revision.
