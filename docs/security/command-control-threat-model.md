# Command Control-Plane Threat Model

## Scope and assets

This model covers authenticated JSON-RPC command submission, durable command and
audit storage, Shelly and Matter dispatch and confirmation, event projection, operator
credentials, and restart recovery. The protected assets are physical safety,
device state, actor identity and grants, bearer and device credentials, audit
integrity, and service availability.

## Trust boundaries

1. An RPC client crosses the loopback HTTP boundary with a bearer token.
2. The API crosses into the transport-neutral `CommandService` with an
   authenticated `Actor`; request parameters cannot supply that actor.
3. The service crosses the durable SQLite boundary before adapter dispatch.
4. A typed adapter crosses the local network to a Shelly component or a Matter
   controller crosses the fabric boundary to a commissioned node.
5. Device observations cross back through push sessions or one bounded status
   read and remain distinct from transport acknowledgement.
6. Secret bytes cross only from `SecretStore` into the Digest implementation.

The unauthenticated `/health` endpoint exposes only liveness and package version.
Binding beyond loopback requires a separately reviewed TLS and token-distribution
boundary.

## Threats and controls

| Threat | Control | Residual risk |
| --- | --- | --- |
| Stolen bearer token | Argon2id hash-only storage, one-time display, rotation, disable, scoped grants | A live token has its granted authority until disabled |
| Actor spoofing in JSON | Actor is derived exclusively from authentication context; no actor request field | Compromised server process remains trusted |
| Unauthorized or over-broad action | Default deny, installation ownership, action/scope/risk grants, exact capability requirement for security risk | Operator can deliberately grant excessive comfort/mechanical scope |
| Delegated or replayed unlock approval | Unlock remains validated until a current interactive `user` with exact `approve_unlock` capability scope approves; request hash, target, action, desired and policy revisions are bound for at most sixty seconds and consumed with dispatch atomically | A compromised interactive user session can approve within its exact grant |
| Mechanical harm | Mechanical risk, fresh-state and calibration requirements, explicit grant, deadline, stop command, physical emergency path | Network/software stop can fail; physical controls remain mandatory |
| Duplicate physical action | Actor-scoped canonical idempotency, pre-dispatch persistence, no redispatch after durable `dispatched` | Crash after physical receipt but before local dispatched commit is prevented by committing dispatched first; device-side ambiguity still requires observation |
| Stale-state toggle or positioning | Fresh observation required; toggle becomes an explicit target before dispatch; calibrated position required | Physical state can change outside HomeMagic after validation |
| Acknowledgement mistaken for outcome | Separate acknowledged and observation-confirmed states | A device can report a state that differs from physical reality |
| Raw vendor bypass | No public raw adapter method or JSON payload API; common capability payloads only | New adapters require the same review discipline |
| Credential disclosure | Opaque references, zeroizing values, redacted errors/debug output, fixture/evidence secret scan | Process-memory compromise is outside this boundary |
| Audit deletion or rewriting | Atomic immutable transition append, optimistic versions, longer audit retention, durable event projection | Local database owner can replace files; signed remote audit is not provided |
| Resource exhaustion | Bounded request pages, event pages, rate limits, per-device concurrency, adapter and recovery deadlines | Many authenticated actors can still consume bounded aggregate capacity |
| Event subscriber lag | Durable cursors are authoritative; bounded wakeups trigger database catch-up | Expired retained cursors require a full read-model rebuild |

## Security invariants

- Every physical adapter dispatch has a durable received command, authenticated
  actor, allowed policy decision, and dispatched transition.
- Validation, RPC, future MCP, automation, and internal callers share one
  `CommandService`; transports cannot invoke adapters directly.
- A repeated equivalent idempotency key returns its original durable command.
- Recovery never blindly dispatches a command already marked dispatched.
- Lock uses normal exact security policy; unlock additionally requires one
  non-delegable, exact, unexpired authorization consumed in the dispatch
  transaction. Agent, automation, service, adapter, and broad grants cannot
  mint it.
- Cross-actor command lookup is indistinguishable from a missing command.
- Secrets never enter command envelopes, audit records, events, or API results.

## Review triggers

Revisit this model before exposing the listener off-host, adding cameras,
adding cloud relays, changing security-risk grants or unlock policy, changing retention, exposing
vendor extensions, or introducing any FFI/sidecar command path.
