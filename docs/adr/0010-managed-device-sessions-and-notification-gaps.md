# ADR-0010: Own one managed session per active device

- Status: Accepted
- Date: 2026-07-11

## Context

Shelly Gen2+ devices publish `NotifyStatus`, `NotifyFullStatus`, and `NotifyEvent`
frames over an inbound WebSocket connection. A client receives notifications
only after sending a request with a valid `src`. Status notifications are
partial overlays and do not carry a reliable, monotonic sequence number.

Uncoordinated WebSocket tasks would allow duplicate sessions, duplicate events,
lost cancellation, and different interpretations of subscription gaps. Treating
every partial frame as a complete snapshot would erase unchanged fields.

## Decision

The Shelly adapter owns a `ShellySessionSupervisor` keyed by stable `DeviceId`.
The supervisor provides replace semantics: starting a device atomically cancels
and joins its prior task before publishing the replacement. Removal and runtime
shutdown use the same cancellation path. No other module spawns Shelly session
tasks directly.

Each session:

1. connects to `ws://<device>/rpc`;
2. sends `Shelly.GetStatus` with a stable HomeMagic `src`, which both registers
   the notification destination and obtains a full baseline;
3. completes the documented Shelly digest exchange when authentication is
   enabled, using `dummy_method:dummy_uri` for HA2;
4. overlays `NotifyStatus` fields onto the baseline, treating explicit `null`
   as field removal;
5. replaces the baseline on `NotifyFullStatus`;
6. publishes normalized observation patches and typed device events through an
   application-owned sink only after durable persistence succeeds.

Identical status patches and replayed event fingerprints are idempotent. Field
timestamps remain unchanged when a partial notification omits the field.

### Gap policy

Shelly notifications have no general sequence counter, so HomeMagic does not
invent one. A bounded HTTP full-status refresh is required after:

- WebSocket reconnect;
- a malformed frame that prevents safe interpretation;
- a notification timestamp older than the accepted device timestamp;
- local sink backpressure or delivery failure;
- an explicit adapter indication that subscription state was lost.

A large forward timestamp jump alone is not a gap: devices may sleep or clocks
may be corrected. A successful `NotifyFullStatus` or refresh resets the baseline
and clears the gap condition.

## Consequences

- Session uniqueness is testable independently from network behavior.
- Partial updates preserve fields and source timestamps by construction.
- Reconnects cost one bounded full refresh but do not silently trust an
  incomplete cache.
- Event deduplication is bounded per session; durable event idempotency remains
  an application/storage responsibility.
- The same supervisor and sink contracts can support Matter subscriptions and
  camera event streams without importing Shelly frame types into the domain.

## References

- [Shelly RPC channels](https://shelly-api-docs.shelly.cloud/gen2/General/RPCChannels/)
- [Shelly notifications](https://shelly-api-docs.shelly.cloud/gen2/General/Notifications/)
- [Shelly authentication](https://shelly-api-docs.shelly.cloud/gen2/General/Authentication/)
