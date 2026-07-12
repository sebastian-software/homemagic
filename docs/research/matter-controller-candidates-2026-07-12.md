# Matter Controller Candidate Discovery and Fixed Rubric

- Discovery date: 2026-07-12
- Source policy: current primary repositories, releases, manifests, and security files
- Target hosts: macOS ARM64 and Linux x86_64
- Decision boundary: ADR-0038 mandatory gates precede weighted scoring

## Discovery method

The snapshot was produced from GitHub repository search and repository APIs,
then verified against each candidate's checked-out README, manifest, license,
default branch, latest commit, and latest release. Candidate source is cloned
only into an isolated spike directory and never added to HomeMagic production
manifests.

Reproduction queries:

```sh
gh api -X GET search/repositories \
  -f q='matter controller language:Rust' -f sort=stars -f order=desc
gh api repos/OWNER/REPOSITORY
gh api repos/OWNER/REPOSITORY/commits
gh api repos/OWNER/REPOSITORY/releases/latest
```

## Current candidate screen

| Candidate | Pinned source | License | Current role | Screen result |
| --- | --- | --- | --- | --- |
| [`tom-code/rust-matc`](https://github.com/tom-code/rust-matc) | `c829d2a1b570b2f2433607a3f4731074b73fb367` | BSD-2-Clause | Native Rust controller library; README claims PASE, CASE, commissioning, read/write, invoke, and subscriptions | Advance to full native spike |
| [`project-chip/rs-matter`](https://github.com/project-chip/rs-matter) | `42d3c2211239f5f388ac7f7449c82bb3347912f5` | Apache-2.0 | Native Rust device stack plus a newly added controller/commissioner path | Advance to full native spike; retain its device role only as a reference for other candidates |
| [`estincelle/chip-tool-rs`](https://github.com/estincelle/chip-tool-rs) | `524a4de506d2ac6d3a3451c0c670775df10565b6` | No detected repository license | Rust chip-tool experiment for rs-matter controller testing | Reject mandatory license/provenance gate |
| [`matter-js/matter.js`](https://github.com/matter-js/matter.js) | `b539372ff41fea24344760d69172508e9df931a2` | Apache-2.0 | Maintained TypeScript controller/device stack; latest release `v0.17.4` | Contingency sidecar only under ADR-0005 |
| [`project-chip/connectedhomeip`](https://github.com/project-chip/connectedhomeip) | release `v1.5.1.0` | Apache-2.0 | Official C++ SDK and controller/reference tooling | Contingency narrow FFI or isolated sidecar only under ADR-0005 |
| [`matter-js/python-matter-server`](https://github.com/matter-js/python-matter-server) | `4c820ed12bac349dda372031e9757c67b9fd9048` | Apache-2.0 | Python server over the official C++ SDK; repository archived | Reject maintenance gate; reference architecture only |

No certification or conformance status is inferred from README claims, tests,
repository ownership, or functional interoperability. Certification evidence
must name the certified product and scope; a reusable library inherits none.

## Primary evidence links

| Candidate | Capability and provenance | Maintenance and security | Conformance statement |
| --- | --- | --- | --- |
| `rust-matc` | [README at pin](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/README.md), [manifest](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/Cargo.toml), [license](https://github.com/tom-code/rust-matc/blob/c829d2a1b570b2f2433607a3f4731074b73fb367/LICENSE) | [commit history](https://github.com/tom-code/rust-matc/commits/c829d2a1b570b2f2433607a3f4731074b73fb367); no repository security policy or GitHub release exists at the snapshot | No certification or conformance claim found; self-tests are candidate evidence only |
| `rs-matter` | [commissioner module at pin](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/rs-matter/src/onboard.rs), [controller example](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/examples/src/bin/commissioner_tests.rs), [Apache license](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/LICENSE) | [security policy](https://github.com/project-chip/rs-matter/blob/42d3c2211239f5f388ac7f7449c82bb3347912f5/SECURITY.md), [v0.2.0](https://crates.io/crates/rs-matter/0.2.0) | ConnectedHomeIP integration is test evidence, not inherited certification; the source explicitly lacks production device-attestation verification |
| `chip-tool-rs` | [repository at pin](https://github.com/estincelle/chip-tool-rs/tree/524a4de506d2ac6d3a3451c0c670775df10565b6) | No detected license, security policy, or release | Rejected before conformance scoring |
| `matter.js` | [controller-capable repository](https://github.com/matter-js/matter.js/tree/b539372ff41fea24344760d69172508e9df931a2), [v0.17.4](https://github.com/matter-js/matter.js/releases/tag/v0.17.4) | [security policy](https://github.com/matter-js/matter.js/security/policy), active project-chip organization | Project functional claims are not product certification |
| `python-matter-server` | [archived repository](https://github.com/matter-js/python-matter-server/tree/4c820ed12bac349dda372031e9757c67b9fd9048) | GitHub archive state at snapshot; latest release `8.1.2` | Rejected before conformance scoring |
| `connectedhomeip` | [official SDK release v1.5.1.0](https://github.com/project-chip/connectedhomeip/releases/tag/v1.5.1.0), [controller guide](https://project-chip.github.io/connectedhomeip-doc/development_controllers/chip-tool/chip_tool_guide.html) | [security policy](https://github.com/project-chip/connectedhomeip/security/policy), Connectivity Standards Alliance project | Official SDK/reference status does not certify HomeMagic or an adapter |

## Mandatory gates

ADR-0038 gates are binary. A failure excludes weighted ranking:

1. distribution-compatible license and traceable provenance;
2. macOS ARM64 and Linux x86_64 build/runtime evidence;
3. SDK-neutral `MatterController` implementation without public type leakage;
4. fabric create/load, on-network commissioning, inventory, bounded reads,
   invoke, subscriptions, restart persistence, and removal;
5. HomeMagic-owned secret-store callbacks and reference-only persistence;
6. normalized errors, cancellation, partial outcomes, and subscription loss;
7. reproducible packaging with pinned source and dependencies;
8. ADR-0005 evidence for every unsafe, FFI, native, process, or non-Rust exception.

## Frozen 0–5 rubrics

Scores are assigned only after all mandatory gates pass. Category points equal
`rating / 5 × weight`; no unpublished adjustment is allowed.

### Feature coverage and independent interoperability — weight 30

| Rating | Evidence threshold |
| ---: | --- |
| 0 | No usable controller lifecycle. |
| 1 | API shapes or self-authored examples only. |
| 2 | Candidate unit/integration tests cover commissioning plus read or invoke. |
| 3 | Independent virtual device proves commission, inventory, read, invoke, and removal on one host; subscriptions or restart remain incomplete. |
| 4 | Full fixed lifecycle, subscriptions, restart, and removal pass against an independent reference on both hosts. |
| 5 | Rating 4 plus separately authorized physical-device and applicable published conformance evidence. |

### Persistence, restart, subscriptions, and recovery — weight 20

| Rating | Evidence threshold |
| ---: | --- |
| 0 | State is ephemeral or restart repeats unsafe work. |
| 1 | Fabric material can be written but recovery semantics are unspecified. |
| 2 | Fabric reload works; subscriptions, cancellation, or partial outcomes are incomplete. |
| 3 | Restart and subscription recreation are bounded with visible failures. |
| 4 | Every fixed dispatch barrier maps to explicit HomeMagic recovery without duplicate physical work. |
| 5 | Rating 4 plus independent crash/fault campaigns across both hosts. |

### Rust, unsafe, FFI, and native footprint — weight 20

| Rating | Evidence threshold |
| ---: | --- |
| 0 | Majority non-Rust or broad unbounded FFI/process API. |
| 1 | Large native SDK or sidecar; replacement boundary is incomplete. |
| 2 | Isolated native component meets ADR-0005 but materially threatens the 95% target. |
| 3 | Rust-majority with bounded audited unsafe/FFI or optional native transport. |
| 4 | 100% first-party Rust, no first-party unsafe, native code only in optional replaceable platform adapters. |
| 5 | Rating 4 plus dependency-policy enforcement and clean default transitive graph on both hosts. |

### Cross-platform runtime and packaging — weight 15

| Rating | Evidence threshold |
| ---: | --- |
| 0 | A required host cannot build. |
| 1 | Both compile only with undocumented/manual host mutation. |
| 2 | Pinned builds pass; runtime or packaging remains host-specific and fragile. |
| 3 | Clean scripted builds/tests pass on both hosts with documented native prerequisites. |
| 4 | Reproducible release artifacts and startup/runtime smoke pass on both hosts. |
| 5 | Rating 4 plus automated packaging, upgrade, and rollback evidence. |

### Maintenance, security, license, and releases — weight 10

| Rating | Evidence threshold |
| ---: | --- |
| 0 | License/provenance failure, archived source, or known unaddressed critical issue. |
| 1 | Single-maintainer experiment without releases or security process. |
| 2 | Active source and compatible license, but limited maintainers/releases/security documentation. |
| 3 | Multiple active contributors, tagged releases, dependency updates, and issue response. |
| 4 | Published security policy/advisories, disciplined releases, and sustained maintenance. |
| 5 | Rating 4 plus audited disclosure/conformance process and documented long-term support. |

### Diagnostics, contract fit, and replacement cost — weight 5

| Rating | Evidence threshold |
| ---: | --- |
| 0 | Raw SDK types/errors must escape the adapter. |
| 1 | Major port semantics require public-contract changes. |
| 2 | Adapter is possible but cancellation, errors, or lifecycle need substantial forks. |
| 3 | Port maps cleanly with a maintained normalization layer and bounded fork surface. |
| 4 | SDK types remain entirely private and replacement is isolated to `homemagic-matter`. |
| 5 | Rating 4 plus a second implementation passes the same suite without application changes. |

## Next evidence

Both `rust-matc` and `rs-matter` advance to native spikes. The initial screen
misclassified `rs-matter` from its device-oriented README; source inspection at
the same pin found the public `onboard::Commissioner`, generic Interaction Model
client transactions, and a controller example. That correction is deliberately
recorded rather than hidden. Each implementation's device fixture is independent
evidence only for the other candidate, never for itself. Non-Rust contingencies
are measured only if both native candidates fail a mandatory gate; they cannot
win by weighted score while bypassing ADR-0005.
