# ADR-0039: Do not select a production Matter controller yet

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0005, ADR-0008, ADR-0033, ADR-0037, ADR-0038, EPIC-004, E4-008

## Context

ADR-0038 requires every production candidate to pass license, platform,
SDK-neutral boundary, independent lifecycle, secret persistence, failure
semantics, packaging, and ADR-0005 gates before weighted scoring. The evaluation
covered two Rust-native candidates, the official ConnectedHomeIP SDK boundary,
and an isolated matter.js sidecar fallback at exact revisions on macOS ARM64 and
Linux x86_64. Subsequent remediation also tested matter.js against the pinned
official ConnectedHomeIP light fixture.

Both Rust-native candidates have production attestation, cancellation, secret,
and lifecycle gaps. ConnectedHomeIP builds but provides neither a stable narrow
C ABI nor a production process boundary. matter.js completes the official
independent lifecycle on Linux x86_64, but times out at the first operational
CASE reconnect on macOS ARM64 against both rs-matter and ConnectedHomeIP. No
candidate passes the gate set.

## Decision

HomeMagic selects no production Matter controller implementation in this ADR.
Weighted scoring is not performed because all candidates fail at least one
mandatory gate.

The deterministic pure-Rust simulator remains available for HomeMagic contract,
RPC, policy, recovery, and UI/agent development. It is not a Matter protocol
implementation and must not be presented as device compatibility.

E4-009 is blocked. E4-008-05 must diagnose the commissioning failure against a
second independent implementation and may then prove either a corrected
Rust-native adapter or the specified isolated matter.js boundary. A future
selection requires a new ADR that supersedes this one and applies the unchanged
ADR-0038 gates. ConnectedHomeIP remains a reference/last-resort adapter source;
its current CLI and Python binding are not accepted runtime dependencies.

## Consequences

- The Rust-majority, secret-store, cancellation, partial-outcome, and packaging
  requirements remain intact.
- EPIC-004 cannot claim production Matter compatibility or unblock its later
  portability, physical-device, or Nuki work yet.
- Track A simulator-backed APIs remain useful and stable while protocol work is
  remediated behind the SDK-neutral port.
- No rejected candidate enters a HomeMagic production manifest.
- The next evidence slice costs more time but avoids committing the product to
  an unobservable commissioning failure or a broad non-Rust boundary.

## Evidence

- [Final controller matrix](../evidence/matter-candidates/final-controller-matrix.md)
- [Native controller contract analysis](../evidence/matter-candidates/native-controller-contract-analysis.md)
- [ConnectedHomeIP boundary audit](../evidence/matter-candidates/connectedhomeip-boundary-audit.md)
- [matter.js contingency audit](../evidence/matter-candidates/matter-js-contingency-audit.md)
- [Proposed matter.js private boundary](../evidence/matter-candidates/matter-js-sidecar-boundary.md)
