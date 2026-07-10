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

## Documentation

- [Home Assistant architecture analysis](docs/research/home-assistant-architecture.md)
- [Target architecture](docs/architecture/target-architecture.md)
- [Roadmap](docs/roadmap.md)
- [Architecture decisions](docs/adr/README.md)

## Development conventions

- Project language: English
- Implementation language: at least 95% Rust for first-party runtime code
- Architecture changes: Architecture Decision Records (ADRs)
- Commit messages: Conventional Commits
- Initial release targets: macOS Apple Silicon and Linux x86_64

