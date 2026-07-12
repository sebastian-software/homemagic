---
id: E4-008-02
epic: EPIC-004
parent: E4-008
title: Build and audit native Rust controller candidates
status: done
priority: critical
depends_on: [E4-008-01]
adrs: [ADR-0005, ADR-0038]
created: 2026-07-12
updated: 2026-07-12
---

# E4-008-02: Native Build and Footprint Audit

## Outcome

Every credible native Rust controller pin has reproducible macOS ARM64 and Linux
x86_64 build/test evidence plus Rust, unsafe, FFI, dependency, binary, license,
and packaging measurements.

## Verification

- [x] Default and all-feature outcomes are reproducible on both hosts.
- [x] First-party and transitive native footprints are reported separately.
- [x] Production manifests remain free of candidate dependencies.

## Progress log

- 2026-07-12: A pinned host-neutral audit script and separate macOS ARM64/Linux
  x86_64 workflow now produce the same versioned JSON report. Manual macOS ARM64
  evidence already passes 69 default and 73 all-feature tests plus a 3.9 MiB
  release example on stable Rust 1.93; workflow evidence remains pending.
- 2026-07-12: Public candidate workflow run `29210089483` passed the identical
  pinned audit on macOS ARM64 and Linux x86_64 and uploaded both reports. The
  compiled first-party path is 100% Rust with zero semantic unsafe blocks;
  repository-wide source bytes are 94.83% Rust because code-generation tooling
  is Python. Optional BLE native dependencies are reported separately. Final
  HomeMagic CI for the committed reports remains pending.
- 2026-07-12: Public repository CI run `29210237059` passed the committed
  reports, production-manifest guard, full workspace, migrations, disclosure,
  and both simulator architectures. This issue is done; E4-008-03 is ready.
- 2026-07-12: Reopened after source inspection identified the pinned
  `rs-matter` commissioner as a second credible native controller candidate.
  Its default workspace, commissioner, and device builds pass locally; the
  two-host workflow and exact footprint reports remain pending.
- 2026-07-12: Corrected public run `29211681113` passed all four host/candidate
  jobs with portable measurements. `rs-matter` default tests and release
  controller/device builds pass both hosts; its all-feature aggregate fails the
  explicit `defmt`/`log` conflict. Reports record 98.16% repository Rust, 211
  repository unsafe lines, no native source, and leave compiled-default unsafe
  unknown instead of claiming zero. This evidence issue is done.
