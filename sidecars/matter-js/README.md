# matter.js private sidecar

This directory contains the isolated non-Rust contingency behind the
`homemagic-matter` private protocol. It is not a public API and is not selected
as the production Matter controller yet.

The first executable slice imports the exact pinned matter.js SDK and supports
only `health_check` and `process_drain`. It intentionally does not advertise
fabric, node, interaction, subscription, event, cancellation, or reverse-secret
capabilities until their production implementations pass the Rust contract
suite. Standard output is protocol-only; ordinary diagnostic payloads are not
written by the child.

`scripts/build-matter-js-sidecar.sh` builds the pinned upstream workspace,
bundles this entrypoint, copies the exact Node runtime and license, and emits a
manifest with hashes and byte counts. The build output remains evaluation
evidence until packaging, license closure, signing, rollback, secrets, and both
platform jobs pass.
