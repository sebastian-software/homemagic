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

## Governed command methods

All command methods derive ownership from the authenticated bearer token. There
is no actor field in the request. Common capability payloads are accepted; raw
Shelly method names and vendor JSON are not.

`commands.validate` runs the complete durable validation and policy path but
never crosses the physical dispatch boundary. `commands.execute` uses the same
path and dispatches only after the received, validated, and allowed facts commit.

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "commands.validate",
  "params": {
    "device_id": "DEVICE_ID",
    "endpoint_id": "switch:0",
    "payload": {"capability": "on_off", "command": {"action": "set", "on": true}},
    "idempotency_key": "kitchen-on-2026-07-11T20:00Z",
    "deadline": "2026-07-11T20:00:15Z"
  }
}
```

Use a fresh idempotency key for a new intent. Reusing the same actor/key with the
same canonical request returns the original command; using it with a different
target, payload, deadline, precondition, or dry-run mode returns `-32023`.
Deadlines are absolute UTC timestamps and are checked around awaited adapter
work. A timeout is a durable command outcome, not permission to retry physical
dispatch.

`commands.get` and `commands.cancel` reveal only commands owned by the current
actor. Cancellation is limited to work that has not crossed the dispatch
boundary. `commands.list` accepts bounded `limit`, `state`, `device_id`, and
`correlation_id` filters. `commands.audit` accepts `id`, optional
`after_sequence`, and bounded `limit`; sequences are command-local durable
cursors.

```json
{"jsonrpc":"2.0","id":11,"method":"commands.get","params":{"id":"COMMAND_ID"}}
```

```json
{"jsonrpc":"2.0","id":12,"method":"commands.list","params":{"state":"confirmed","limit":50}}
```

```json
{"jsonrpc":"2.0","id":13,"method":"commands.audit","params":{"id":"COMMAND_ID","after_sequence":2,"limit":50}}
```

```json
{"jsonrpc":"2.0","id":14,"method":"commands.cancel","params":{"id":"COMMAND_ID"}}
```

For a device-query-to-command flow without copying internal IDs, use the
dependency-free helper from a clean checkout. It validates by default and reads
the bearer token only from the environment:

```bash
export HOMEMAGIC_TOKEN='token printed once by actor-bootstrap'
python3 scripts/rpc-command.py 'Kitchen light' --action on
```

Only add `--execute` after reviewing the validated command:

```bash
python3 scripts/rpc-command.py 'Kitchen light' --action on --execute \
  --idempotency-key 'kitchen-on-2026-07-11T20:00Z'
```

There is no generic rollback operation: a confirmed physical action is a fact.
Rollback means issuing a new governed compensating command with a new
idempotency key. For moving covers, use `position.v1` with `{"action":"stop"}`
as the software emergency stop, while preserving access to the physical stop or
power-isolation path. Never depend on network RPC as the only emergency control.

## Automation lifecycle methods

Automation methods derive the author or decision maker exclusively from the
authenticated bearer token. Supplying an extra `actor_id` parameter has no
effect. Drafts use optimistic revisions; immutable versions use positive
version numbers and exact compiler evidence.

The first lifecycle slice exposes:

- `automations.drafts.create` with authored `draft` content only; the server
  supplies schema, automation ID, version, authenticated author, and timestamp;
- `automations.drafts.put` with `document` and optional `expected_revision`;
- `automations.drafts.get` with `automation_id`, and `automations.drafts.list`
  with a bounded `limit`;
- `automations.validate` with `automation_id`;
- `automations.versions.get` with `automation_id` and `version`, and
  `automations.versions.list` with `automation_id` and bounded `limit`;
- `automations.simulate` with the exact version and synthetic `input` history;
- `automations.approve` / `automations.reject` with optional `rationale`;
- `automations.activate` with exact version and `expected_revision`;
- `automations.rollback` with an older exact ready version and current
  `expected_revision`; `automations.disable` and `automations.retire` use the
  same optimistic identity revision;
- `automations.catch_up` with one exact `scheduled_for` instant and
  actor-scoped `idempotency_key`.
- `automations.runs.get`, `automations.runs.list`, and
  `automations.runs.trace`; trace uses optional run-local `after_sequence` and
  bounded `limit`. `automations.runs.cancel` atomically cancels eligible timers
  and appends an outcome trace.

Simulation never accepts a plan, run ID, occurrence ID, correlation ID, or
dispatcher from the caller. Those values are derived by the lifecycle service.
Catch-up never scans a time range and rejects a schedule whose normal window is
still open.

An agent can create the first draft without hand-built automation, actor,
version, schema, or timestamp fields. The executable request is
`docs/api/examples/automation-draft-create-v1.json`; it also uses no device ID.

```bash
curl --fail-with-body http://127.0.0.1:8787/rpc \
  -H "Authorization: Bearer $HOMEMAGIC_TOKEN" \
  -H "Content-Type: application/json" \
  --data-binary @docs/api/examples/automation-draft-create-v1.json
```

The response returns the generated automation ID. Use
`docs/api/examples/automation-document-v1.json` as the complete full-document
shape for subsequent optimistic `automations.drafts.put` updates.

```json
{
  "jsonrpc": "2.0",
  "id": 31,
  "method": "automations.catch_up",
  "params": {
    "automation_id": "AUTOMATION_ID",
    "scheduled_for": "2026-07-11T16:00:00Z",
    "idempotency_key": "catch-up-2026-07-11"
  }
}
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
| `-32040` | Automation service unavailable |
| `-32041` | Automation access denied |
| `-32042` | Automation resource not found |
| `-32043` | Automation lifecycle state conflict |
| `-32044` | Automation validation failed; data contains bounded findings |
| `-32045` | Automation simulation failed |
| `-32046` | Automation persistence or scheduler stage failed |
| `-32047` | Explicit catch-up request rejected |
| `-32012` | A second subscription was attempted on one WebSocket |
| `-32020` | Command service unavailable in this runtime |
| `-32021` | Command not found or not owned by the actor |
| `-32022` | Command already crossed the cancellable boundary |
| `-32023` | Idempotency key conflicts with another canonical request |

## Durable device shape

A durable device record separates stable HomeMagic and adapter-native identities
from mutable display name, aliases, space assignments, lifecycle, availability,
timestamps, endpoint/capability descriptions, network locations, latest snapshots,
and namespaced vendor diagnostics. Capability contracts are versioned independently
from mutable metadata.
