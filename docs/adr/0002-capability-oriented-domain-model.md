# ADR-0002: Model devices through composable capabilities

- Status: Accepted
- Date: 2026-07-11

## Context

Home Assistant's entity domains provide a successful compatibility vocabulary,
but they also expose historical and UI-facing conventions to users. Renaming,
grouping, and selecting the right entity can become harder than expressing the
underlying intent.

A Shelly relay, dimmer, or cover and a Matter endpoint are vendor-specific
objects, but they share behaviors such as switching, level control, position,
energy measurement, and availability.

## Decision

HomeMagic separates physical identity from behavior:

- `Device` represents a physical or logical product with stable adapter identity.
- `Endpoint` represents an independently addressable part of a device.
- `Capability` is a typed behavior attached to an endpoint.
- `Observation` is time-stamped reported state.
- `Command` requests a capability transition and carries causation metadata.
- `Event` records a fact that occurred and cannot be changed.

Stable opaque IDs are never derived from display names. Names, aliases, spaces,
and semantic tags are mutable metadata. Users and agents normally address things
by intent and resolved identity rather than by constructing entity identifiers.

Initial capabilities include availability, on/off, level, position, power,
energy, and device diagnostics. Vendor-specific data remains available as
namespaced extensions without contaminating the common model.

## Consequences

- Automations can target behavior across vendors.
- Future UIs can be generated from capability schemas and context.
- Adapter authors must perform an explicit semantic mapping.
- Capability evolution needs versioning and conformance tests.

