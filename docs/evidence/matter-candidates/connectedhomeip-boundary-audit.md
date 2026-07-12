# ConnectedHomeIP contingency boundary audit

## Verdict

The official `v1.5.1.0` controller sources build on both HomeMagic target hosts,
but the candidate fails the production-boundary gates. The repository provides
a mature C++ `DeviceCommissioner`, a test-oriented `chip-tool` CLI, and a broad
Python-owned native binding. It does not provide a stable narrow C ABI or a
versioned process protocol that can implement the SDK-neutral
`MatterController` port.

This is a boundary rejection, not a protocol-capability rejection. A bespoke
adapter around the C++ controller could be built later, but accepting that
adapter before its ownership, callback, secret, cancellation, crash, packaging,
and upgrade contracts exist would waive ADR-0005 and ADR-0038.

## Reproducible build evidence

| Measurement | macOS ARM64 | Linux x86_64 |
| --- | ---: | ---: |
| Build result | Pass | Pass |
| Build seconds | 262 | 425 |
| `chip-tool` bytes | 52,830,184 | 154,916,592 |
| Source checkout KiB | 1,276,016 | 1,357,772 |
| Source plus build tree KiB | 1,930,220 | 2,533,604 |
| Bootstrap environment KiB | 4,581,624 | 5,270,572 |
| Checked-out submodules | 81 | 84 |

The targets disable BLE, Wi-Fi, and Thread and retain on-network/IP controller
scope. Platform-specific submodule manifests are recorded in the JSON reports.
The CLI prints its command catalog and exits with status 1 when invoked without
a command; the harness records this behavior rather than treating it as an
embeddable health endpoint.

## Boundary inventory

- `src/controller` and `examples/chip-tool` contain 131,010 C/C++ lines at the
  exact pin.
- The existing Python native layer exposes 199 distinct `pychip_*` symbols. It
  passes C++ controller pointers and Python objects across the boundary and
  installs process-global callbacks. Reusing it would import Python-specific
  ownership and callback semantics into HomeMagic.
- `chip-tool` is a command-oriented test controller. Its per-command process
  model and interactive modes are not a versioned, authenticated, resumable
  HomeMagic controller protocol.
- The underlying C++ controller contains the needed commissioning,
  attestation, cancellation, interaction-model, subscription, and removal
  mechanisms. No narrow first-party adapter exposes them with HomeMagic error,
  partial-outcome, cursor, secret-store, or process-crash semantics.

## Mandatory-gate disposition

| ADR-0038 gate | Result | Evidence |
| --- | --- | --- |
| License and provenance | Pass | Official Apache-2.0 release and exact commit |
| macOS ARM64 and Linux x86_64 build | Pass | Two-host JSON reports |
| SDK-neutral production port | Fail | No stable narrow C ABI or process protocol |
| Complete independent lifecycle | Not run | No production adapter exists; CLI evidence would not prove the missing boundary |
| ADR-0008/ADR-0037 secret persistence | Fail | No Rust-owned secret callback boundary exists |
| Errors, cancellation, partial outcomes, subscription loss | Fail | Underlying hooks exist but are not surfaced through an accepted boundary |
| Reproducible production packaging | Fail | Reproducible development builds exist; no HomeMagic runtime package exists |
| ADR-0005 exception evidence | Fail | Trust, crash, upgrade, rollback, and replacement contracts remain unimplemented |

The failed SDK-neutral boundary gate rejects the candidate before weighted
scoring. The lifecycle task is deliberately classified `not run`, rather than
using successful `chip-tool` commands to imply an embeddable production
adapter.

## Smallest credible future exception

A future reconsideration must introduce one versioned, opaque-handle C ABI or
one supervised local process. It must expose only fabric lifecycle, commission,
inventory, bounded read, invoke, subscribe/event cursor, remove, cancel, and
health operations. C++ types, exceptions, raw callbacks, and SDK-owned secret
files must remain behind the boundary. Rust must own request validation,
idempotency, authorization, durable operation state, secret-store access, crash
reconciliation, and public RPC projection.

Replacement becomes mandatory when a Rust-native controller passes every
ADR-0038 gate. Until the bespoke boundary itself passes those gates, the
official SDK remains a development/reference contingency only.
