# matter.js sidecar contingency audit

## Verdict

The exact matter.js pin builds and imports its controller on both target hosts,
but it fails the mandatory independent lifecycle gate. Against the pinned
rs-matter device, both hosts create the controller fabric and reach the device's
`ArmFailSafe` handler, then remain inside matter.js commissioning until the
explicit 180-second process budget expires. Commissioning never returns even
when automatic post-commission connection is disabled, so inventory, read,
invoke, subscription, process restart, and removal cannot begin.

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

Both final reports have the same outcome:

| Phase | macOS ARM64 | Linux x86_64 |
| --- | --- | --- |
| Fabric create | Pass | Pass |
| Reference observation | `ArmFailSafe` received | `ArmFailSafe` received |
| Commission | Timeout at 180 seconds | Timeout at 180 seconds |
| Inventory/read/invoke/subscribe | Not run | Not run |
| Process restart/removal | Not run | Not run |

The spike disables `connectNodeAfterCommissioning`, proving the timeout is in
commissioning itself rather than the later automatic connect/subscription path.
Secret-safe stage tracing narrows both hosts to step `18.1 Reconnect`: initial
data, fail-safe, regulatory configuration, time synchronization, device
attestation, certificates/NOC, and access control all complete before the first
operational CASE reconnect stalls.
The workflow exits successfully because its job is to persist partial evidence,
not to turn candidate failure into missing evidence.

## Mandatory-gate disposition

| ADR-0038 gate | Result | Evidence |
| --- | --- | --- |
| License and provenance | Pass | Apache-2.0, exact commit and lockfile |
| Build/run on both targets | Pass for build and fabric start | Two-host build and lifecycle reports |
| SDK-neutral production port | Fail | Protocol specified but not implemented |
| Complete independent lifecycle | Fail | Commission timeout on both hosts |
| ADR-0008/ADR-0037 secrets | Fail | Rust-owned storage driver unimplemented |
| Errors/cancel/partial/subscription loss | Fail | Partial phase evidence exists; production boundary does not |
| Reproducible production packaging | Fail | Development build only |
| ADR-0005 exception | Fail | No accepted sandboxed package or boundary fault suite |

## Required remediation

The next experiment must first instrument non-sensitive commissioning-stage
progress and reproduce against a second independent device implementation. It
must determine whether the deadlock is matter.js, rs-matter, or the fixture
contract before changing timeouts again. A sidecar may be reconsidered only
after the complete lifecycle passes on both hosts and the proposed private
boundary, Rust-owned storage driver, pruned package, and fault suite exist.
