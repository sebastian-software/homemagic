# Final Matter controller candidate matrix

The ADR-0038 scorecard is gate-first. No candidate passed every mandatory gate,
so weighted scores and tie-breaks are intentionally not calculated.

| Candidate | License/provenance | Both hosts | SDK-neutral boundary | Independent lifecycle | Secrets | Failure semantics | Packaging/ADR-0005 | Disposition |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| rust-matc `c829d2a1` | Pass | Pass | Feasible, incomplete | Fail | Fail | Fail | Native build pass | Reject before scoring |
| rs-matter `42d3c221` | Pass | Pass | Feasible, incomplete | Fail | Fail | Fail | Lock/unsafe proof incomplete | Reject before scoring |
| ConnectedHomeIP `v1.5.1.0` | Pass | Pass | Fail | Not run | Fail | Fail | Fail | Reference-only |
| matter.js `b539372f` | Pass | Pass | Specified, unimplemented | Fail | Fail | Fail | Fail | Reject before scoring |
| chip-tool-rs `524a4de5` | Fail | Not evaluated | Not evaluated | Not evaluated | Not evaluated | Not evaluated | Not evaluated | Reject: no license |
| python-matter-server `4c820ed1` | Pass | Not evaluated | Archived stack | Not evaluated | Not evaluated | Not evaluated | Fail | Reject: archived |

## Evidence by candidate

### rust-matc

- Builds, tests, and packages natively on both hosts with no detected
  first-party unsafe or native source in the default path.
- The independent rs-matter lifecycle times out during Linux commissioning after
  `ArmFailSafe`. On macOS it passes commission, inventory, read, and subscription
  establishment, then invoke fails; restart and removal are not run.
- Device attestation is disabled in the candidate path, full commissioning
  cancellation is absent, and the convenient device manager hard-codes a
  plaintext file certificate manager.

### rs-matter

- Builds its commissioner and device references on both hosts.
- The direct controller mapping is source-proven, but it has no separate
  completed independent lifecycle beyond its role as the device fixture.
- Production attestation remains deferred, cancellation/partial outcomes and
  ADR-0008 storage are unproven, the upstream root has no lockfile, and compiled
  default-path unsafe usage is not yet isolated from 211 repository unsafe
  source lines.

### ConnectedHomeIP

- The official no-BLE/no-Wi-Fi/no-Thread `chip-tool` builds on both hosts.
- It has a mature C++ controller but no stable narrow C ABI or production
  process protocol. The existing Python binding has 199 `pychip_*` symbols and
  Python/C++ callback ownership.
- The development footprint is several GiB per host. A bespoke adapter, secret
  boundary, fault semantics, and production package do not exist.

### matter.js

- The exact Node runtime and lockfile build/import successfully on both hosts.
- Against official ConnectedHomeIP `v1.5.1.0`, Linux x86_64 passes commission,
  inventory, read, invoke, subscription, restart, and removal. macOS ARM64
  stalls at operational reconnect, matching its rs-matter result. The required
  two-host lifecycle therefore remains failed.
- A minimal private RPC boundary is specified, but Rust-owned secret storage,
  full cancellation, fault handling, and a pruned production package are not
  implemented.

## Selection result

ADR-0039 selects no production implementation. The pure-Rust deterministic
controller remains the only composed runtime implementation and carries no
Matter device compatibility claim. E4-008-05 owns remediation; E4-009 remains
blocked until a superseding ADR selects a candidate that passes every gate.
