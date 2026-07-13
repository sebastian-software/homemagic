# matter.js sidecar contingency audit

## Verdict

The exact matter.js pin builds and imports its controller on both target hosts,
but it still fails the mandatory two-host independent lifecycle gate. Against
the pinned official ConnectedHomeIP light fixture, Linux x86_64 passes the
complete lifecycle while macOS ARM64 stalls at the first operational CASE
reconnect. The earlier rs-matter fixture stalls at the same stage on both hosts.
The official Linux pass excludes a general matter.js commissioning failure; the
matching macOS result across two devices narrows the active defect to matter.js
operational discovery/CASE behavior on macOS or its host environment.

A second, explicitly diagnostic run seeds the fixture's already known `::1`
address into the operational peer immediately before CASE. Both hosts then pass
the full lifecycle. This proves the macOS CASE/credential path and isolates the
unmodified failure to operational mDNS address acquisition in the same-host CI
topology. It does not prove physical-network mDNS on macOS and is not accepted
as a production patch. The matching upstream issue also resolved a macOS
reconnect failure as blocked mDNS/firewall traffic: [matter.js issue
#1363](https://github.com/matter-js/matter.js/issues/1363).

The candidate is rejected before weighted scoring. The private sidecar protocol
remains a useful future boundary design, not an accepted ADR-0005 exception.

## Build evidence

| Measurement | macOS ARM64 | Linux x86_64 |
| --- | ---: | ---: |
| Build/import result | Pass | Pass |
| Build seconds | 28 | 35 |
| Source checkout KiB | 63,960 | 66,564 |
| Full development `node_modules` KiB | 593,892 | 605,916 |
| Built workspace distributions KiB | 127,476 | 130,180 |
| Installed package manifests | 874 | 873 |
| Installed native `.node` files | 59 | 58 |

These values describe a clean monorepo development install. They do not claim a
production sidecar footprint; pruning, bundling, licenses, signing, and rollback
remain unproven. Both reports use Node `v24.18.0`, npm `11.16.0`, the exact
upstream commit, and the exact upstream package-lock hash.

## Private package prototype

The first pruned executable prototype bundles the exact SDK into a 2,148,316
byte ESM file and packages the exact Node `v24.18.0` runtime. On the local macOS
ARM64 Homebrew runtime, the executable is a 68,384-byte launcher plus the
60,198,928-byte `libnode.137.dylib`; the complete directory is 61,132 KiB. Every
runtime component has a committed SHA-256 manifest, and the packaged process
passes the real Rust supervisor handshake, health request, and drain path.

This is positive boundary and footprint evidence, not a production package.
It advertises only `health_check` and `process_drain`, includes Node and
matter.js top-level licenses but not yet a generated bundled-input license
closure, and has no signing, rollback, real controller storage, or device
operations. The exact local report is
[matter-js-sidecar-package-local-macos-arm64.json](matter-js-sidecar-package-local-macos-arm64.json);
the two-host workflow owns reproducible runner packages.

Public run `29215942175` passes package build, real Rust handshake/health/drain,
canary scan, and artifact upload on both targets. The minified SDK bundle is
byte-identical across hosts (`c39fbcd...`, 2,148,316 bytes). The official
setup-node runtimes are self-contained: 120,965,360 bytes on macOS ARM64 and
123,655,872 bytes on Linux x86_64. Exact manifests are committed for
[macOS ARM64](matter-js-sidecar-package-macos-arm64.json) and
[Linux x86_64](matter-js-sidecar-package-linux-x86_64.json).

## Rust-owned fabric storage prototype

The next package slice replaces matter.js file persistence with a custom
in-memory storage driver. Each matter.js namespace is encoded with the SDK's
typed JSON codec and persisted under a private `matter/storage/<namespace>`
handle through reverse secret RPC. Rust owns the bytes, revisions, and
compare-and-swap decisions; the child holds no durable file and serializes
concurrent SDK commits before updating the Rust revision.

The real packaged macOS ARM64 process passes this sequence through the Rust
supervisor: handshake, `fabric_create`, reverse secret writes, controlled
drain, fresh process, `fabric_load` from the same Rust store, and controlled
drain. The package now advertises `fabric_create`, `fabric_load`,
`health_check`, and `process_drain`. This closes the local architecture proof
for Rust-owned fabric persistence. It does not yet prove missing-fabric
recovery beyond fail-closed rejection, encrypted production storage, two-host
packaging, downgrade and rollback behavior, or Matter node operations. The
exact local package manifest is committed as
[matter-js-sidecar-fabric-package-local-macos-arm64.json](matter-js-sidecar-fabric-package-local-macos-arm64.json).

## Source capabilities and gaps

- `CommissioningController` provides on-network commissioning, inventory,
  interaction clients, automatic subscriptions, persistence, and removal.
- The source includes local device-attestation validation and an optional DCL
  certificate service. Some DCL device-software/certificate checks remain
  explicit upstream TODOs.
- Discovery can be cancelled. The legacy controller facade used by the spike
  does not expose a complete operation handle for all later commissioning
  phases; the newer node API has narrower abort support that remains unproven in
  this lifecycle.
- matter.js storage is pluggable. The package prototype now uses an in-memory
  driver backed only by Rust reverse secret RPC; local create/restart/load is
  proven, while production encryption, recovery, and two-host evidence remain
  open.
- The committed private-boundary proposal covers framing, version negotiation,
  reverse secret callbacks, event backpressure, partial outcomes, supervision,
  packaging, and removal criteria. None of those contracts has a production
  implementation or fault suite yet.

## Independent lifecycle

The official ConnectedHomeIP reference separates the platform result:

| Phase | macOS ARM64 | Linux x86_64 |
| --- | --- | --- |
| Fabric create | Pass | Pass |
| Reference | ConnectedHomeIP `v1.5.1.0` light | ConnectedHomeIP `v1.5.1.0` light |
| Commission | Timeout at `18.1 Reconnect` | Pass |
| Inventory/read/invoke/subscribe | Not run | Pass |
| Process restart/removal | Not run | Pass |

The test-only direct-address diagnostic passes every row on both hosts. Its
exact reports are [macOS ARM64](matter-js-direct-case-macos-arm64.json) and
[Linux x86_64](matter-js-direct-case-linux-x86_64.json). The upstream patch is
committed beside the spike and applied only when the diagnostic flag is set.

The spike disables `connectNodeAfterCommissioning`, proving the timeout is in
commissioning itself rather than the later automatic connect/subscription path.
Secret-safe stage tracing narrows the macOS failure to step `18.1 Reconnect`:
initial data, fail-safe, regulatory configuration, time synchronization, device
attestation, certificates/NOC, and access control all complete before the first
operational CASE reconnect stalls. Linux completes that reconnect, commissioning
complete, fabric-label update, interaction, restart, and removal.
The workflow exits successfully because its job is to persist partial evidence,
not to turn candidate failure into missing evidence.

## Mandatory-gate disposition

| ADR-0038 gate | Result | Evidence |
| --- | --- | --- |
| License and provenance | Pass | Apache-2.0, exact commit and lockfile |
| Build/run on both targets | Pass for build and fabric start | Two-host build and lifecycle reports |
| SDK-neutral production port | Fail | Protocol specified but not implemented |
| Complete independent lifecycle | Fail | Linux passes; macOS times out at operational reconnect |
| ADR-0008/ADR-0037 secrets | Partial | Rust-owned reverse-RPC storage passes local create/restart/load; production encrypted-store integration remains open |
| Errors/cancel/partial/subscription loss | Fail | Partial phase evidence exists; production boundary does not |
| Reproducible production packaging | Partial | Health-only package passes both hosts; fabric-storage package evidence is local only |
| ADR-0005 exception | Fail | Boundary fault suite exists, but the exception, complete operations, signing, rollback, and production sandbox are not accepted |

## Required remediation

The protocol lifecycle is now sufficient to begin implementing the private
boundary, Rust-owned storage driver, pruned package, and fault suite. Production
selection still requires a real-network macOS mDNS run (or an upstream-accepted
address fallback) in addition to those gates; the same-host diagnostic cannot
stand in for discovery compatibility.

Exact reports: [macOS ARM64](matter-js-connectedhomeip-macos-arm64.json) and
[Linux x86_64](matter-js-connectedhomeip-linux-x86_64.json).
