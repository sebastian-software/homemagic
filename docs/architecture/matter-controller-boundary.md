# Matter Controller Boundary

ADR-0033 keeps controller SDKs and protocol runtime details outside the domain
and application crates. E4-002 establishes this dependency direction before a
simulator or production adapter exists:

```text
homemagic-domain
       ^
       |
homemagic-application
       ^
       |
homemagic-matter (E4-004 onward)
       ^
       |
simulator or selected SDK adapter
```

## Domain ownership

`homemagic-domain::matter` owns only SDK-neutral, serializable contracts:

- opaque fabric, projection, subscription, operation, and controller-event IDs;
- validated fabric-scoped node IDs, endpoint numbers, and descriptor revisions;
- bounded device-type, cluster, command, endpoint, text, and octet collections;
- normalized desired and reported state, freshness, convergence, and uncertainty;
- validated durable operation kinds, phases, targets, revisions, and timestamps;
- normalized controller events and closed structured controller errors.

Every persisted constructor invariant is reapplied during deserialization. The
domain crate has no async runtime, network, storage, SDK, FFI, or integration
dependency.

## Application ownership

`homemagic-application::MatterController` is an object-safe async port used at
the runtime composition boundary. Its requests and responses contain only
domain types, `SecretRef`, and non-serializable `SecretValue` for immediate
sensitive input/output.

The port covers fabric status/create, commission/cancel, descriptor inventory,
subscriptions, bounded reads, governed capability commands, removal, protected
export/restore, and normalized event paging. `MatterControllerCommand` is a
closed normalized enum; callers cannot supply cluster IDs plus arbitrary command
payloads as a public write mechanism.

Application services will own durable workflow state and policy. A controller
acknowledgement remains distinct from reported physical confirmation.

## Integration ownership

E4-004 creates `homemagic-matter`. That crate may depend on domain and
application contracts and implement the port. A selected SDK, native library,
simulator state, protocol callbacks, and SDK-specific persistence interfaces may
exist only there or below it.

The dependency must never reverse: `homemagic-domain` and
`homemagic-application` cannot depend on `homemagic-matter`, a Matter SDK,
Matter.js, or Connected Home over IP runtime code. API and MCP layers may call
application services but cannot construct a raw cluster write.

## Executable check

Run the dependency guard from the repository root:

```sh
./scripts/check-matter-boundaries.sh
```

The guard inspects normal Cargo dependency trees and the two core manifests.
The E4-009 adapter must continue passing it after the production SDK is added.
