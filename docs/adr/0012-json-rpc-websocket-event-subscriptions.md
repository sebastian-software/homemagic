# ADR-0012: Stream durable events over JSON-RPC WebSockets

- Status: Accepted
- Date: 2026-07-11

## Context

HomeMagic clients and agents need ordered lifecycle and observation events after
the initial read model has loaded. The existing JSON-RPC endpoint uses one HTTP
request and response, which cannot deliver a long-lived subscription. The event
history already has monotonic durable cursors and bounded retention under
ADR-0009.

The transport must bound slow consumers, make gaps explicit, and allow a client
to resume without inventing a second non-RPC event vocabulary.

## Decision

HomeMagic adds a JSON-RPC 2.0 WebSocket endpoint at `/rpc/ws`. A client sends an
`events.subscribe` request with the last fully processed cursor, or omits the
cursor to start after the current durable tail. The successful response reports
the subscription identifier, earliest available cursor, current tail, batch
limit, and live notification capacity.

Committed events are delivered as `events.next` JSON-RPC notifications. Each
notification contains the durable cursor and typed domain event. Delivery order
is cursor order, not task completion order.

The database remains the source of truth. An in-process bounded broadcast channel
only wakes subscribers; after every wake-up, the subscriber reads the next
bounded cursor page from durable storage. This avoids assigning cursors twice and
allows recovery if concurrent commits and fan-out complete in different orders.

If the requested cursor predates retained history, the server returns the typed
`cursor_expired` error with the earliest available cursor. If the wake-up channel
overruns while a socket is blocked, the server emits one `events.lagged`
notification containing the last delivered cursor, then catches up from durable
storage. The stream closes when durable catch-up is impossible or the WebSocket
disconnects. Clients reconnect and resubscribe with their last processed cursor.

The first implementation uses these fixed safety bounds:

- 128 events per durable catch-up page;
- 256 pending live wake-ups per process;
- one active subscription per WebSocket;
- no unbounded per-client queue.

## Consequences

- HTTP JSON-RPC remains the request/response control plane.
- Streaming uses the same method, error, and typed-event vocabulary.
- Slow clients cannot create unbounded memory growth.
- Resume works across process restarts while the cursor remains retained.
- WebSocket transport authentication remains deferred; like the HTTP prototype,
  it must only bind to trusted interfaces until an authentication ADR is accepted.
