# Native Controller Contract Analysis

- Candidate pins: `rust-matc c829d2a1`, `rs-matter 42d3c221`
- Contract: `homemagic_application::MatterController`
- Evidence rule: candidate self-tests and independent interoperability are
  separate; neither implementation may validate itself

## Port mapping

| Fixed operation | `rust-matc` | `rs-matter` | Current evidence |
| --- | --- | --- | --- |
| Fabric create/load | `CertManager`, `Controller`, `DeviceManager` | caller installs/loads `Matter` fabric state | Both self-build; `rust-matc` lifecycle spike |
| On-network commission | `Controller::commission` | `onboard::Commissioner::{commission, complete_via_case}` | Self-tests pass; fresh independent device receives `ArmFailSafe`, then `rust-matc` times out |
| Inventory | `DeviceManager` registry | caller-owned registry | Mapping compiles; not reached after independent commissioning failure |
| Bounded read | `Connection::read_request2` | `ImClient` read transaction | Mapping compiles; candidate self-tests only |
| Invoke | `Connection::invoke_request` | `ImClient` invoke transaction | Mapping compiles; candidate self-tests only |
| Subscribe | `Connection::subscribe_attrs/events` | `ImClient` subscribe transaction | Mapping compiles; candidate self-tests only |
| Restart | reload `DeviceManager`, establish CASE | caller reloads KV/fabric and establishes CASE | Mapping compiles; not reached independently |
| Remove | raw `RemoveFabric`, then registry cleanup | generic invoke, then caller registry cleanup | Mapping compiles; not reached independently |
| Export/restore | HomeMagic-owned wrapper | HomeMagic-owned wrapper | Candidate-neutral Track A contract only |
| Cursor events | HomeMagic-owned journal | HomeMagic-owned journal | Candidate-neutral Track A contract only |

The isolated spike README records the exact `rust-matc` mapping. No candidate
type enters the application/domain public API or a production Cargo manifest.

## Mandatory-gap findings

### Device attestation

Neither candidate currently verifies Device Attestation in its on-network
commissioning path:

- `rust-matc` has a commented-out `run_attestation` call in `commission.rs`;
- `rs-matter::CommissionOptions` states that real DCL/certificate-chain
  verification is deferred, and production-safe `allow_test_attestation=false`
  has no success path.

The independent fixture therefore proves protocol interoperability only. It
does not prove a production-safe commission operation, and the adapter must map
this absence to `Attestation/AttestationFailed` or reject startup. Enabling a
test bypass is never an accepted production configuration.

### Cancellation and partial outcomes

Neither API exposes a commissioning cancellation token or a durable phase
handle. Dropping an in-flight Rust future may stop local work, while remote
fail-safe expiry may later roll device state back; that is not equivalent to a
confirmed cancellation. The only safe adapter behavior is:

1. stop issuing new protocol steps;
2. return `Cancelled/Cancelled` only before the first remote mutation;
3. otherwise return `Persistence/OutcomeIndeterminate`;
4. reconcile fabric/node inventory after the fail-safe deadline;
5. expose explicit repair rather than automatically recommissioning.

This is a mandatory adapter gap, not hidden retry behavior.

### Secrets and persistence

`rust-matc::CertManager` is replaceable, but `DeviceManager::create/load`
hard-codes `FileCertManager`, whose plaintext PEM backend is spike-only. A
production adapter would need lower-level controller construction with an
ADR-0008 secret-reference implementation and a separate HomeMagic registry.

`rs-matter` accepts caller-owned KV persistence, but the current commissioner
example generates CA/controller keys in process and does not demonstrate the
ADR-0008 secret-store lifecycle. An opaque encrypted KV adapter remains to be
proven.

### Failure normalization

Candidate error text must never cross the port. The adapter owns a finite map:

| Candidate failure context | HomeMagic category/code | Retry rule |
| --- | --- | --- |
| address/mDNS not found before dispatch | `Discovery/DiscoveryTimeout` | after network repair |
| UDP, MRP, PASE, or CASE transport failure before remote mutation | `Transport/SessionUnavailable` | bounded safe retry |
| passcode/PASE rejection | `Authentication/AuthenticationFailed` | after recommission action |
| absent/failed attestation | `Attestation/AttestationFailed` | never without trust repair |
| read status or malformed report | `Protocol/ReadFailed` | bounded when idempotent |
| invoke status before acknowledgement | `Protocol/InvokeFailed` | never unless command policy proves safety |
| timeout after remote dispatch | `Persistence/OutcomeIndeterminate` | after reconciliation |
| lost report stream | `Protocol/SubscriptionLost` | after explicit repair |
| secret callback failure | `SecretStore/SecretUnavailable` | after secret-store repair |

Raw SDK errors remain private diagnostics with secret-safe structured fields.

## Gate state before selection

| ADR-0038 gate | `rust-matc` | `rs-matter` |
| --- | --- | --- |
| License/provenance | Pass | Pass |
| Both target hosts | Pending committed two-host report | Pending committed two-host report |
| SDK-neutral port | Spike compiles; full adapter pending | Mapping only; adapter pending |
| Fixed lifecycle | Fail locally: fresh `rs-matter` receives `ArmFailSafe`, commission then times out; both-host capture pending | Incomplete independent evidence |
| HomeMagic secret callbacks | Feasible lower-level trait; unproven | Caller-owned persistence; unproven |
| Errors/cancellation/partial outcomes | Fail: no cancellation handle; attestation absent | Fail: no cancellation handle; attestation absent |
| Reproducible packaging | Pinned spike lock | Upstream has no lock; generated resolution recorded |
| ADR-0005 exceptions | Default path has no native/FFI exception | Default path has no native/FFI exception |

No weighted winner may be declared while either mandatory failure remains.
E4-008-04 must select a fully evidenced remediation or record the scoped
blocker; it cannot reinterpret the independent green lifecycle as attestation
or cancellation evidence.

## Primary source anchors

- [`rust-matc` commission source](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/src/commission.rs)
- [`rust-matc` controller and subscription API](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/src/controller.rs)
- [`rust-matc` certificate manager](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/src/certmanager.rs)
- [`rs-matter` commissioner source](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/rs-matter/src/onboard.rs)
- [`rs-matter` Interaction Model client](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/rs-matter/src/im/client.rs)
