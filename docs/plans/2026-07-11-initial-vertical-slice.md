# Initial Shelly Discovery Vertical Slice

- Date: 2026-07-11
- Status: In progress

## Outcome

Run one HomeMagic process on macOS Apple Silicon or Linux x86_64, discover local
Shelly Gen2+ devices, read their public device information and available status,
project known components into common capabilities, and list those snapshots over
a HomeMagic JSON-RPC endpoint.

## Scope

### Included

- Rust workspace with domain, Shelly adapter, API, and application crates;
- mDNS browse for `_shelly._tcp.local.`;
- `Shelly.GetDeviceInfo` and `Shelly.GetStatus` over HTTP;
- in-memory registry with stable IDs and freshness metadata;
- projections for `switch:*`, `light:*`, and `cover:*` status;
- JSON-RPC methods `system.health`, `devices.list`, `devices.get`, and
  `devices.refresh`;
- fixture-backed unit/integration tests;
- operator documentation and example calls.

### Explicitly excluded

- commands that change a physical device;
- Shelly authentication;
- persistent WebSocket notifications;
- Gen1 CoIoT;
- durable storage;
- Matter, MCP, and automation execution.

These exclusions define the first safe slice, not the product boundary.

## Implementation sequence

1. Define stable device, endpoint, capability, and observation types.
2. Implement a concurrency-safe in-memory registry.
3. Implement Shelly HTTP calls and fixture parsing.
4. Convert component status objects into typed capabilities while retaining raw
   vendor data for diagnosis.
5. Implement mDNS discovery and bounded concurrent refresh.
6. Add JSON-RPC request validation, method dispatch, structured errors, and
   health/list/get/refresh methods.
7. Compose lifecycle, tracing, and graceful shutdown in the binary.
8. Add tests, README instructions, sample payloads, and validation commands.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cargo run -- serve
curl -s http://127.0.0.1:8787/rpc \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}'
```

Hardware verification records the host platform, HomeMagic commit, Shelly model,
firmware version, discovery result, projected capabilities, and any authentication
limitation.

