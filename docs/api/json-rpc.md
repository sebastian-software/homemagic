# JSON-RPC Prototype API

## Status and transport

This is the RPC-first transport described by ADR-0003. Request/response methods
use JSON-RPC 2.0 at `POST /rpc`. Durable event subscriptions use JSON-RPC 2.0
messages over a WebSocket at `GET /rpc/ws` as defined by ADR-0012.

The default listener is `127.0.0.1:8787`. `POST /rpc` and the `/rpc/ws`
WebSocket handshake require `Authorization: Bearer <token>`. Bootstrap a token
with `homemagic actor-bootstrap`; only its Argon2id hash is stored. Missing,
malformed, unknown, disabled, and incorrect credentials all receive the same
HTTP `401 Unauthorized` response.

`GET /health` is intentionally unauthenticated and returns only process liveness
and the package version. Repository details remain in authenticated
`system.health`. Keep the listener on loopback unless a trusted TLS boundary and
token-distribution policy are in place.

## Read methods

### `system.health`

Returns process status, package version, repository backend, migration version,
integrity result, WAL state, and retained event-cursor bounds.

```json
{"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}
```

### `devices.list`

Returns durable device records in stable HomeMagic ID order. Optional filters are
combined with logical AND: `lifecycle`, `availability`, `freshness`, `integration`,
and `space_id`. Every item includes freshness calculated with the runtime policy.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}
```

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "devices.list",
  "params": {"availability": "online", "freshness": "fresh", "integration": "shelly"}
}
```

### `devices.get`

Returns one durable aggregate, a connection/freshness summary, latest capability
observations, and retained repair records.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.get","params":{"id":"DEVICE_ID"}}
```

### `devices.refresh`

Runs configured integration scanners, durably reconciles the registry, and
returns the integration summary and current snapshots. Per-device requests and
the complete refresh cycle are bounded by runtime deadlines.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.refresh","params":{}}
```

### Repair methods

`repairs.list` returns retained records in stable repair-ID order and optionally
filters by `status` and `device_id`. `repairs.get` returns one retained record.

```json
{"jsonrpc":"2.0","id":1,"method":"repairs.list","params":{"status":"open","device_id":"DEVICE_ID"}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"repairs.get","params":{"id":"REPAIR_ID"}}
```

## Metadata methods

Display metadata is mutable; HomeMagic, native-device, endpoint, and capability
identities never change. Names and aliases are trimmed and bounded. Assigned
space IDs must already exist. Causation always uses the authenticated actor;
client-supplied `actor` fields are ignored and cannot spoof audit identity.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.rename","params":{"id":"DEVICE_ID","name":"Kitchen light"}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"devices.aliases.set","params":{"id":"DEVICE_ID","aliases":["Main light","Ceiling"]}}
```

```json
{"jsonrpc":"2.0","id":3,"method":"devices.spaces.set","params":{"id":"DEVICE_ID","spaces":["SPACE_ID"]}}
```

## Event subscriptions

Open `/rpc/ws` with the same bearer header and send exactly one
`events.subscribe` request. `cursor` is the last event the client fully
processed. Omitting it starts after the current tail; using `0` replays all
retained history.

```json
{"jsonrpc":"2.0","id":1,"method":"events.subscribe","params":{"cursor":42}}
```

The response reports `subscription_id`, the accepted cursor, retained cursor
bounds, the 128-event durable catch-up limit, and the 256-signal live capacity.
Events then arrive in durable cursor order:

```json
{
  "jsonrpc": "2.0",
  "method": "events.next",
  "params": {"subscription_id": "...", "item": {"cursor": 43, "event": {}}}
}
```

The bounded live channel only wakes the subscriber; event payloads are read from
durable storage. If wake-ups overrun while the socket is blocked, the server emits
`events.lagged` with `last_delivered_cursor`, then catches up from the database.
A disconnected client reconnects with its last fully processed cursor. A cursor
older than retained history receives `cursor_expired` with `earliest_cursor` and
must rebuild from the read APIs.

## Errors

| Code | Meaning |
| ---: | --- |
| `-32600` | Invalid JSON-RPC version or request |
| `-32601` | Method not found |
| `-32602` | Invalid method parameters or metadata |
| `-32000` | HomeMagic operation failed |
| `-32004` | Device not found |
| `-32005` | Space not found |
| `-32006` | Repair not found |
| `-32010` | Event cursor expired |
| `-32011` | Event subscriptions unavailable in this runtime |
| `-32012` | A second subscription was attempted on one WebSocket |

## Durable device shape

A durable device record separates stable HomeMagic and adapter-native identities
from mutable display name, aliases, space assignments, lifecycle, availability,
timestamps, endpoint/capability descriptions, network locations, latest snapshots,
and namespaced vendor diagnostics. Capability contracts are versioned independently
from mutable metadata.
