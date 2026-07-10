# JSON-RPC Prototype API

## Status

This is an inspectable prototype transport for the application contract described
in ADR-0003. Method names and payloads may change before the first stable API
version. The server accepts JSON-RPC 2.0 requests at `POST /rpc`.

The default listener is `127.0.0.1:8787`. The prototype has no authentication and
must not be bound to an untrusted interface.

## Methods

### `system.health`

Returns process status and package version.

```json
{"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}
```

### `devices.list`

Returns every current device snapshot in stable HomeMagic ID order.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}
```

### `devices.get`

Returns one current snapshot by opaque HomeMagic device ID.

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "devices.get",
  "params": {"id": "00000000-0000-0000-0000-000000000000"}
}
```

### `devices.refresh`

Runs configured integration scanners, updates the in-memory registry, and returns
the refresh summary and current snapshots. A Shelly refresh waits for the bounded
mDNS discovery window before reading device RPC endpoints.

```json
{"jsonrpc":"2.0","id":1,"method":"devices.refresh","params":{}}
```

## Errors

The prototype uses standard JSON-RPC codes where possible:

| Code | Meaning |
| ---: | --- |
| `-32600` | Invalid JSON-RPC version/request |
| `-32601` | Method not found |
| `-32602` | Invalid method parameters |
| `-32000` | Integration refresh failed |
| `-32004` | Device not found |

## Snapshot shape

A device has stable HomeMagic and adapter-native identities, a mutable display
name obtained from the device, manufacturer/model metadata, network locations,
an observation timestamp, endpoints, normalized capability snapshots, and
namespaced vendor data for diagnostics.

Capability objects carry a versioned `kind` such as `on_off`, `level`, `position`,
`power`, or `energy`. This prototype serializes the capability enum directly. A
later contract version will include explicit schema discovery and compatibility
rules before third-party clients are supported.

