# ADR-0034: Project Matter through versioned capability rules

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0002, ADR-0009, ADR-0010, ADR-0033, EPIC-004, E4-001

## Context

Matter describes nodes through endpoint descriptors, device types, clusters,
feature maps, attributes, events, and commands. Exposing those objects directly
would force agents and future UIs to learn protocol details and would make
behavior depend on mutable endpoint metadata.

Cluster presence alone is not enough to infer safe semantics. Roles, device
types, feature bits, mandatory data, revisions, and supported commands affect
whether an endpoint can satisfy a common capability contract.

## Decision

Matter adapters project protocol data into the ADR-0002 device model through
explicit versioned rules. Identity is persisted as:

```text
HomeMagic fabric ID -> Matter fabric identity
HomeMagic device ID -> fabric-scoped Matter node ID
HomeMagic endpoint ID -> node ID plus Matter endpoint number
HomeMagic capability ID -> endpoint plus projection rule version
```

Labels, aliases, spaces, network addresses, sessions, and subscription handles
are mutable metadata and never identity inputs.

Every rule declares eligible device types and endpoint role, required clusters,
feature/command constraints, required attributes, source revisions, capability
schema version, state mapping, command mapping, freshness requirements, and
invalidation conditions.

The initial rules are:

- applicable On/Off server behavior maps to `on_off.v1`;
- applicable Door Lock server behavior maps to a versioned access-control
  capability with governed `lock` and `unlock` actions.

Later fixtures add supported Level Control to `level.v1` and Window Covering to
constrained `position.v1`. Window covering remains subject to calibration,
feature, stop, freshness, serialization, and mechanical policy requirements.

Descriptor, feature-map, cluster-revision, or command-support changes invalidate
the affected projection. A bounded rediscovery must succeed before another
command relies on the changed assumptions.

Unmapped standard or vendor data may be exposed only as bounded, read-only,
versioned, namespaced diagnostics. It cannot add a public raw read/write or
invoke escape hatch. Reliable battery, reachability, firmware, power, energy,
attestation, certification, and OTA visibility may map to existing common
capabilities after fixture-backed semantic review.

## Consequences

- Agents and generated UIs use the same cross-vendor capability vocabulary.
- Projection changes are reviewable and fixture-tested instead of implicit SDK
  behavior.
- Some cluster data remains diagnostic-only until semantics are proven.
- Descriptor changes can temporarily make a capability stale or unavailable.
- Compatibility claims must name exact rule version and verified features.
