---
id: E1-009
epic: EPIC-001
title: Ship operations, compatibility evidence, and exit audit
status: planned
priority: high
depends_on: [E1-008]
adrs: []
created: 2026-07-11
updated: 2026-07-11
---

# E1-009: Operations and Exit Audit

## Outcome

Operators can locate, back up, restore, diagnose, and recover a HomeMagic
installation, while repeatable hardware and CI evidence proves the EPIC-001 exit
gate on supported platforms.

## Tasks

- [ ] Document database location, migration startup, backup, and restore.
- [ ] Document platform and headless credential setup and recovery.
- [ ] Add a redacted hardware smoke-test command and report schema.
- [ ] Record tested Shelly model, firmware, host, capabilities, and result.
- [ ] Cover switch, dimmer, and cover hardware on macOS Apple Silicon.
- [ ] Ensure Linux x86_64 CI runs format, Clippy, tests, and migrations.
- [ ] Run a plaintext-secret scan over fixtures and captured diagnostics.
- [ ] Link evidence to every EPIC-001 acceptance criterion and exit item.
- [ ] Update EPIC-002 with finalized repository, event, and credential contracts.

## Acceptance criteria

- [ ] A clean-checkout operator can back up and restore an installation.
- [ ] Smoke reports are reproducible and contain no secrets.
- [ ] Required hardware evidence is committed or linked with exact versions.
- [ ] Supported-platform quality gates pass.
- [ ] Every EPIC-001 checklist item is either evidenced or explicitly unresolved.

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-features --locked`
- [ ] Documentation link and command smoke tests.
- [ ] Requirement-by-requirement EPIC-001 completion audit.

## Progress log

- 2026-07-11: Issue created.
