# HomeMagic

HomeMagic is an experimental, local-first home automation system written in Rust.
It uses Home Assistant as a compatibility and behavior reference, but deliberately
does not copy Home Assistant's configuration, integration, or UI architecture.

The project is built around three ideas:

- a capability-oriented device model instead of user-facing entities;
- a versioned RPC API as the primary interface for people, agents, and future UIs;
- declarative, auditable automations that agents can propose, validate, simulate,
  and activate according to policy.

The first executable milestone discovers Shelly Gen2+ devices on the local
network and exposes their identity and current component status through the API.

## Project status

HomeMagic is at the architecture-prototype stage. The current scope and evidence
are tracked in [the roadmap](docs/roadmap.md) and the
[initial vertical-slice plan](docs/plans/2026-07-11-initial-vertical-slice.md).

The current prototype has been verified on macOS Apple Silicon against real
Shelly Gen2/3 devices. It is read-only: it discovers devices and projects their
reported switch, light, cover, power, and energy state, but cannot change them.

## Try the prototype

Show a count without printing device metadata:

```sh
cargo run --locked -- scan --summary
```

Print all normalized device snapshots:

```sh
cargo run --locked -- scan
```

Start the loopback-only JSON-RPC server:

```sh
cargo run --locked -- serve
```

Then list the current device registry:

```sh
curl -s http://127.0.0.1:8787/rpc \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}'
```

See the [prototype operator guide](docs/operations/shelly-prototype.md) and
[JSON-RPC reference](docs/api/json-rpc.md) for details and limitations.

## Documentation

- [Home Assistant architecture analysis](docs/research/home-assistant-architecture.md)
- [Target architecture](docs/architecture/target-architecture.md)
- [Roadmap](docs/roadmap.md)
- [Architecture decisions](docs/adr/README.md)
- [Prototype operator guide](docs/operations/shelly-prototype.md)
- [JSON-RPC reference](docs/api/json-rpc.md)

## Development conventions

- Project language: English
- Implementation language: at least 95% Rust for first-party runtime code
- Architecture changes: Architecture Decision Records (ADRs)
- Commit messages: Conventional Commits
- Initial release targets: macOS Apple Silicon and Linux x86_64
