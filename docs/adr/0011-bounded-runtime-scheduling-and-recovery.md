# ADR-0011: Bound runtime scheduling and recovery loops

- Status: Accepted
- Date: 2026-07-11

## Context

Discovery, HTTP refreshes, WebSocket sessions, and freshness evaluation are
independent sources of work. If each failure starts an unconstrained retry,
one offline device or network outage can exhaust sockets, tasks, and device
authentication limits. Shutdown must stop producers before closing persistence.

Shelly sleeping devices require a distinct semantic state: silence is expected
and must not be treated as the same signal as a failed mains-powered device.

## Decision

HomeMagic uses four bounded loops:

1. **Periodic discovery:** runs once at startup and then at a configurable
   interval. At most one discovery cycle is active; missed ticks coalesce.
2. **Per-device session recovery:** reconnect delay follows exponential backoff
   with bounded proportional jitter. The delay is capped, and the attempt count
   resets after a configured stable-session duration.
3. **Gap refresh:** session gap requests enter a bounded channel keyed by
   `DeviceId`. Duplicate pending requests coalesce. A global concurrency limit
   and per-operation timeout protect convergence.
4. **Freshness evaluation:** a deterministic clock evaluates durable
   `last_success` timestamps. It changes lifecycle/availability metadata and
   publishes typed events without changing the latest observed values.

Default bounds are configuration, not protocol constants:

- reconnect base: 1 second;
- reconnect cap: 60 seconds;
- jitter: up to 25 percent of the exponential delay;
- stable-session reset: 5 minutes;
- discovery interval: 60 seconds;
- per-device network timeout: 5 seconds;
- complete refresh deadline: 30 seconds;
- concurrent device refreshes: 16;
- pending gap refreshes: 256.

Sleeping devices retain `AvailabilityState::Sleeping` and
`FreshnessState::Sleeping` until an adapter-specific wake expectation is
violated. Generic offline thresholds never override sleeping state.

Shutdown order is: stop periodic producers, close refresh intake, drain or
cancel bounded refresh work, cancel and join sessions, finish the active
repository transaction, then stop API transports.

## Consequences

- Recovery behavior is deterministic under an injected jitter value and clock.
- A network outage produces bounded work instead of synchronized retry bursts.
- Gap refresh may be delayed or coalesced, but never grows without limit.
- Freshness and availability can change while the last known capability values
  remain available to automations and read APIs.
- Adapter-specific sleeping policies can refine the generic rule later without
  changing the durable state model.
