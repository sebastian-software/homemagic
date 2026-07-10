# Shelly Discovery Prototype

## What it proves

The prototype exercises the first complete HomeMagic path:

1. discover a local manufacturer device;
2. read identity, configuration, and current status;
3. separate stable identity from the device's display name;
4. project vendor components into normalized capabilities;
5. store current snapshots in the application registry;
6. return them through a HomeMagic-owned RPC method.

The implementation never sends a state-changing Shelly method.

## Requirements

- Rust 1.85 or newer;
- macOS Apple Silicon or Linux x86_64;
- the host and Shelly devices on an mDNS-reachable local network;
- unauthenticated Shelly Gen2+ devices for full status in this first version.

On macOS, the process may need Local Network permission. HomeMagic first tries a
pure-Rust mDNS implementation. If that returns no services, the macOS adapter uses
`/usr/bin/dns-sd -Z` to read services through the system mDNSResponder and then
continues with Rust HTTP RPC and projection. Linux uses the pure-Rust path.

## One-shot scan

```sh
cargo run --locked -- scan --summary
```

The full scan contains local device metadata and current state. Treat its output
as private household data:

```sh
cargo run --locked -- scan
```

The default discovery window is four seconds. It can be changed when multicast
responses are slow:

```sh
cargo run --locked -- scan --discovery-seconds 8 --summary
```

Discovery is best effort. Sleeping, offline, filtered, or slow devices may not be
present in every bounded scan. Repeating a scan can therefore produce a different
count without indicating identity loss.

## Server mode

```sh
RUST_LOG=info cargo run --locked -- serve
```

The daemon performs one refresh before listening. It still starts if the refresh
fails, allowing `devices.refresh` to retry. The default bind address is loopback
only. Keep it that way until authentication and authorization are implemented.

Useful calls:

```sh
curl -s http://127.0.0.1:8787/health

curl -s http://127.0.0.1:8787/rpc \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}'

curl -s http://127.0.0.1:8787/rpc \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"devices.refresh","params":{}}'
```

Stop the daemon with `Ctrl-C`.

## Current projections

| Shelly component | HomeMagic capabilities |
| --- | --- |
| `switch:<id>` | on/off, power, energy |
| `light:<id>` | on/off, level, power/energy when reported |
| `cover:<id>` | position/motion, power, energy |
| device | availability, diagnostics |

Unrecognized component data remains in the namespaced `shelly.status` vendor
payload so protocol evidence is not discarded while the common vocabulary is
still small.

## Known limitations

- read-only;
- no Shelly authentication;
- no Gen1/CoIoT support;
- no persistent WebSocket subscription;
- in-memory registry only;
- no stale-device removal within a running process;
- no API authentication or authorization;
- no automatic periodic refresh;
- device counts can vary with the bounded mDNS window.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

