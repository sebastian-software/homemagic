# rust-matc controller contract spike

This isolated Cargo workspace pins `rust-matc` without adding it to any
HomeMagic production manifest. The executable attempts the on-wire subset of
the fixed `MatterController` contract against an independently maintained
`rs-matter` on/off-light process and emits partial outcomes when a phase fails.

| HomeMagic port operation | Spike mechanism | Evidence boundary |
| --- | --- | --- |
| `fabric_status` / `create_fabric` | `DeviceManager::create/load` plus adapter-owned metadata | Candidate file backend is test-only; production secrets require ADR-0008 callbacks |
| `commission` | `DeviceManager::commission` over IPv6 | Candidate currently omits Device Attestation verification |
| `cancel_commissioning` | No candidate cancellation handle | Mandatory gap; timeout can only classify the outcome as indeterminate |
| `nodes` / `node` | Persistent bounded device registry | Registry is candidate-owned JSON in this spike |
| `subscribe` | `Connection::subscribe_attrs` | Recreated explicitly after restart; candidate handle drop is local only |
| `read` | typed/raw bounded Interaction Model read | SDK types must be normalized inside the production adapter |
| `invoke` | typed/raw Interaction Model invoke | Response status must be normalized before crossing the port |
| `remove_node` | remote `RemoveFabric`, then local registry removal | Two-step partial outcome must remain visible |
| `export_fabric` / `restore_fabric` | HomeMagic wrapper over secret references | Not supplied by the candidate |
| `events_after` | HomeMagic adapter event journal | Not supplied by the candidate |

The current independent run reaches the reference device's `ArmFailSafe` step
but times out before commissioning completes from a fresh device state. The
spike therefore proves the mapping and captures an interoperability failure; it
does not prove a complete lifecycle. It is not a production adapter and cannot
waive missing attestation, cancellation, secret storage, error normalization,
or partial-outcome work.
