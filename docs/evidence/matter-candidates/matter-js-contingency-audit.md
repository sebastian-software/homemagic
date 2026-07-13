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
- matter.js storage is pluggable, but the candidate audit uses its default file
  driver. A Rust-backed in-memory/reverse-RPC driver satisfying ADR-0008 and
  ADR-0037 has only been specified, not implemented.
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
| ADR-0008/ADR-0037 secrets | Fail | Rust-owned storage driver unimplemented |
| Errors/cancel/partial/subscription loss | Fail | Partial phase evidence exists; production boundary does not |
| Reproducible production packaging | Fail | Development build only |
| ADR-0005 exception | Fail | No accepted sandboxed package or boundary fault suite |

## Required remediation

The protocol lifecycle is now sufficient to begin implementing the private
boundary, Rust-owned storage driver, pruned package, and fault suite. Production
selection still requires a real-network macOS mDNS run (or an upstream-accepted
address fallback) in addition to those gates; the same-host diagnostic cannot
stand in for discovery compatibility.

Exact reports: [macOS ARM64](matter-js-connectedhomeip-macos-arm64.json) and
[Linux x86_64](matter-js-connectedhomeip-linux-x86_64.json).
