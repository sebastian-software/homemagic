# Proposed matter.js private sidecar boundary

## Status

This is the minimum boundary to test if the independent lifecycle passes. It is
not an accepted production protocol and does not authorize a non-Rust runtime
exception. Every unproven item below remains an ADR-0038 mandatory failure.

## Isolation model

The Rust modular monolith spawns one pinned sidecar process per installation.
The child receives two inherited anonymous pipes and no listening network
socket. Standard error is diagnostic output with mandatory redaction; standard
input/output carry protocol frames only. The child runs with a dedicated data
directory, no shell, no ambient credentials, no outbound Internet access except
an explicitly configured DCL proxy, and the smallest platform sandbox available.

Rust owns authorization, idempotency, durable operation state, deadlines,
secret-store access, public RPC projection, audit events, restart policy, and
the selected `MatterController` port. TypeScript owns only Matter protocol state
and interaction with commissioned nodes. No matter.js class name, JSON shape,
error, or numeric enum becomes a public HomeMagic contract.

## Framing and handshake

Frames use a four-byte unsigned big-endian payload length followed by UTF-8
JSON. The first implementation caps ordinary frames at 1 MiB and secret-import
frames at 8 MiB. Oversized, malformed, duplicate, or out-of-order frames close
the process and map to a stable protocol error.

The first child frame is `hello` and contains:

- private protocol major/minor version;
- exact matter.js revision and packaged Node version;
- supported controller methods and event kinds;
- maximum frame sizes and event window;
- a child nonce.

Rust answers with `accept` containing the selected minor version, installation
ID, a random session nonce, and policy capabilities. Both nonces are included in
every subsequent envelope. Version downgrade, unknown major versions, or a
mismatched revision fail closed before secrets are released. Inherited pipes
provide peer placement; the nonce handshake prevents accidental cross-wiring,
not a compromised same-user process.

## RPC envelope

Requests contain `request_id`, `method`, `deadline_ms`, `idempotency_key`, and a
method-specific SDK-neutral body. Responses contain the same request ID and one
of `result`, `partial`, or `error`. Stable methods are limited to:

- `fabric.load`, `fabric.create`, `fabric.export`, and `fabric.remove`;
- `node.commission`, `node.inventory`, and `node.remove`;
- `attribute.read` and `command.invoke`;
- `subscription.open`, `subscription.resume`, and `subscription.close`;
- `operation.cancel`, `health.check`, and `process.drain`.

The sidecar may issue reverse `secret.get`, `secret.put`, `secret.delete`, and
`secret.compare_and_swap` calls. Those calls address opaque HomeMagic secret
handles and never receive platform keychain identifiers. Rust serializes them
through ADR-0008 and ADR-0037 stores. The sidecar must use an in-memory custom
matter.js storage driver; default file storage is forbidden in production.

## Events and backpressure

Every event contains a monotonically increasing process-local sequence, a
durable HomeMagic subscription ID, the associated operation ID when present,
and an SDK-neutral event body. Rust grants an explicit event window and
acknowledges the highest contiguous sequence. The sidecar stops reading Matter
reports when the window is exhausted, coalesces state values by attribute path,
and never coalesces lifecycle, error, lock, or security events.

After process restart, Rust reopens subscriptions from its durable cursor. The
sidecar reports a `subscription_lost` gap when remote Matter state cannot be
replayed; Rust then invokes the existing reconciliation path. Sequence reuse,
silent gaps, or unbounded buffering are protocol violations.

## Cancellation and partial outcomes

`operation.cancel` is idempotent and acknowledges one of `cancelled`,
`too_late`, or `unsupported_at_phase`. A local deadline always triggers cancel,
but Rust does not report cancellation success until the sidecar confirms the
protocol operation stopped. Process termination is a last resort and produces
an indeterminate outcome for mutating operations.

Commissioning progress must identify discovery, PASE, attestation, fabric
installation, operational discovery, CASE, subscription initialization, and
completion. A crash or timeout after fabric installation is never flattened to
`commission_failed`; Rust persists the last acknowledged phase and reconciles
inventory on restart.

## Secrets and logs

Setup codes, Wi-Fi or Thread credentials, private keys, certificates containing
private material, imported fabric state, and secret callback payloads are
marked sensitive at the envelope type boundary. They are excluded from normal
logs, tracing fields, crash dumps, persisted request journals, and test
snapshots. Diagnostic errors may contain stable secret handles but never secret
values. JavaScript cannot guarantee deterministic memory zeroization, so the
exception ADR must treat process isolation and lifetime as risk controls, not as
equivalent to Rust-owned secret memory.

## Supervision and packaging

Rust applies bounded startup, request, drain, and kill deadlines with exponential
restart backoff and a circuit breaker. A child crash fails in-flight reads and
health calls as unavailable; mutating calls become indeterminate until durable
operation reconciliation completes. Crash loops disable Matter only, not the
HomeMagic core.

Production packaging must bundle an exact Node runtime and a pruned sidecar
artifact for both target hosts. Requiring a user-installed Node runtime is not
acceptable. The current monorepo build reports measure development cost only;
they do not prove the size, license closure, reproducibility, signing, or update
rollback of a production package.

## Required boundary tests

Acceptance requires tests for incompatible and downgraded handshakes, malformed
and oversized frames, secret redaction, storage callback failure, cancellation
at every commissioning phase, sidecar crash before and after mutation, hung
requests, event-window exhaustion, cursor gaps, restart persistence, missing
runtime, package rollback, and replacement by a fake SDK-neutral controller.

The exception must be removed when a Rust-native implementation passes all
ADR-0038 gates with equal device compatibility. A matter.js major/revision
change, Node end-of-support, an unresolved critical advisory, or failure to
reproduce either target package suspends upgrades until this boundary suite is
green again.
