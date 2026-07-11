# ADR-0017: Version automation documents and normalized plans independently

- Status: Accepted
- Date: 2026-07-11

## Context

Agent-authored automations must remain diffable and understandable while the
runtime needs a canonical, fully resolved, bounded representation. Executing the
authored JSON directly would mix authoring compatibility, current registry
resolution, safety inference, and runtime semantics.

## Decision

HomeMagic stores immutable authored `automation.document.v1` versions and compiles
them into immutable `automation.plan.v1` versions. Document and plan schemas have
independent explicit version identifiers and canonical hashes.

An authored version contains provenance, typed triggers, conditions, actions,
variables, run mode, timeout/retry/failure behavior, and rationale. It cannot
contain arbitrary code, templates, loops, recursion, raw adapter calls, or vendor
command dictionaries.

Validation resolves references and types against an exact registry revision.
Compilation emits stable node IDs, deterministic ordering, resolved stable
targets, inferred types, Safety Profiles, explicit budgets, and desired-state
reduction segments. Successful evidence is keyed by document hash, plan hash,
and registry revision.

Readers reject unknown schema versions. Compatibility changes require a new
schema version and an explicit pure migration that creates a new immutable
automation version. Persisted documents and plans are never rewritten in place.
Equivalent canonical content must produce the same digest on supported hosts.

## Consequences

- Authoring and runtime formats can evolve without silent semantic drift.
- Activation cannot reuse evidence produced for other content or registry state.
- The schema, canonicalizer, migration fixtures, and normalized examples become
  public compatibility contracts.
