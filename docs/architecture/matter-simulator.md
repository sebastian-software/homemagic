# Deterministic Matter Controller Simulator

## Evidence boundary

`homemagic-matter` is an in-process Rust implementation of the
application-owned `MatterController` port. It exercises HomeMagic lifecycle,
state, command, restart, and error contracts. It does not implement the Matter
wire protocol and does not prove IPv6, discovery, commissioning transport,
attestation, certification, SDK correctness, interoperability, or compatibility
with a physical device.

The crate has no Matter SDK, FFI, sidecar, network runtime, or external
reference implementation dependency.

## Versioned fixtures

The initial built-in fixtures are:

- `light-v1`: Matter On/Off light on endpoint 1, initially off;
- `door-lock-v1`: Matter Door Lock on endpoint 1, initially locked.

Commissioning accepts only the deterministic non-secret setup tokens exported
by the crate. Fixture node IDs, endpoint numbers, descriptors, attributes, and
commands are fixed. Fabric and operation IDs remain caller-owned port inputs.
Controller event IDs derive from fabric ID plus event sequence, so identical
inputs do not introduce random trace data.

## Virtual execution

`SimulatorClock` advances explicit UTC time without sleeping. Every timestamp
in acknowledgements, reports, events, checkpoints, and traces comes from that
clock.

`SimulatorDispatchBarriers` exposes two independently controlled crossings:

- `before_invoke` pauses before simulated physical state changes;
- `after_acknowledgement` pauses after state mutation and acknowledgement but
  before report delivery.

Tests can therefore distinguish command supersession before dispatch from
post-dispatch observation and convergence without relying on scheduler timing.

## Fault script

The ordered `SimulatorFault` queue supports:

- exact structured failures for every controller port operation;
- dropped, duplicated, delayed, and out-of-order reports;
- subscription loss followed by explicit resubscription;
- unknown cancellation and partial removal outcomes; and
- restart at each supported lifecycle phase.

Restart faults capture simulator-only state after the exact progress fact.
`from_restart_checkpoint` reloads that state without replaying an unknown
remote action. Full explicit checkpoints also round-trip virtual fabric, node,
attribute, subscription, event, pending-report, and trace state.

## Export isolation

Simulator exports use `MatterFabricExportFormat::SimulatorV1`, a simulator-only
prefix, and a deterministic non-secret recovery placeholder. Production import
code must call `ensure_protected_format` before adapter byte access; the
application contract rejects `SimulatorV1` there. The simulator conversely
rejects `ProtectedV1`, malformed prefixes, wrong placeholders, and fabric-ID
mismatches.

This is structural isolation for tests. The simulator format is not a protected
backup and must never be presented as one.

## Normalized evidence

`tests/controller_contract.rs` covers:

- happy paths for every `MatterController` operation;
- light and lock read/invoke/report behavior;
- pre-dispatch and post-acknowledgement barriers;
- every report, subscription, cancellation, removal, restart, and injectable
  error category;
- simulator versus protected export rejection;
- checkpoint resume; and
- property-generated command orders repeated for byte identity.

`tests/fixtures/light-trace-v1.json` is the inspectable normalized trace for the
version-one fixture sequence. Its committed SHA-256 is:

`7451b5a74337e40a2312f5a5723308ad1e8a881714e19f94c9b0e538bff1f244`

The CI portability job verifies that exact JSON and hash on `ubuntu-latest`
with `x86_64` and `macos-14` with `arm64`, and fails if the runner architecture
does not match. GitHub currently documents `macos-14` as an ARM64 hosted runner:
[GitHub-hosted runners](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job).

Local evidence on 2026-07-12:

- host architecture: `arm64`;
- simulator contract tests: 9 passed;
- committed JSON/hash fixture: matched;
- strict Clippy: passed;
- complete workspace tests and warning-denied Rustdoc: passed;
- Matter dependency boundary and secret scan: passed.

The Linux x86_64 CI execution remains pending because this repository currently
has no configured remote and no local Linux container runtime. The matrix is
committed but must run before E4-004 is marked done.
