# matter.js private sidecar

This directory contains the isolated non-Rust contingency behind the
`homemagic-matter` private protocol. It is not a public API and is not selected
as the production Matter controller yet.

The executable imports the exact pinned matter.js SDK. It supports
`fabric_create`/`fabric_load` through a Rust-backed, revisioned in-memory storage
driver, SDK-neutral `node_inventory`, plus `health_check` and `process_drain`.
It intentionally does not advertise commissioning, node mutation, interaction,
subscription, event, or cancellation capabilities
until their production implementations pass the Rust contract suite. Standard
output is protocol-only; ordinary diagnostic payloads are not written by the
child.

`scripts/build-matter-js-sidecar.sh` builds the pinned upstream workspace,
bundles this entrypoint, copies the exact Node runtime and license, and emits a
manifest with hashes and byte counts. The build output remains evaluation
evidence until packaging, license closure, signing, rollback, secrets, and both
platform jobs pass.
