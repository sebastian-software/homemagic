# ADR-0005: Keep first-party runtime code at least 95% Rust

- Status: Accepted
- Date: 2026-07-11

## Context

Rust is the only fixed implementation constraint. Some home-automation domains,
especially certified Matter controllers and video codecs, have mature reference
implementations outside Rust. Reimplementing complete protocol stacks solely to
meet language purity would delay compatibility and introduce safety risk.

## Decision

At least 95% of first-party runtime source code, measured by tracked non-generated
lines, must be Rust. External processes and FFI are allowed only when no credible
Rust alternative meets the required compatibility and maintenance bar.

Each exception requires an ADR that documents:

- the missing Rust capability;
- trust, memory-safety, and process-isolation boundaries;
- supported platforms and packaging impact;
- conformance and upgrade strategy;
- criteria for replacement by a Rust implementation.

Unsafe Rust is forbidden by default. Any unavoidable unsafe block must be small,
reviewed, documented with its safety invariant, and covered by boundary tests.

## Consequences

- The product retains a coherent Rust architecture.
- Matter and media work can use proven components selectively.
- Language composition and dependency provenance remain measurable rather than
  aspirational.

