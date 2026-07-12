---
id: E1-009
epic: EPIC-001
title: Ship operations, compatibility evidence, and exit audit
status: done
priority: high
depends_on: [E1-008]
adrs: []
created: 2026-07-11
updated: 2026-07-12
---

# E1-009: Operations and Exit Audit

## Outcome

Operators can locate, back up, restore, diagnose, and recover a HomeMagic
installation, while repeatable hardware and CI evidence proves the EPIC-001 exit
gate on supported platforms.

## Tasks

- [x] Document database location, migration startup, backup, and restore.
- [x] Document platform and headless credential setup and recovery.
- [x] Add a redacted hardware smoke-test command and report schema.
- [x] Record tested Shelly model, firmware, host, capabilities, and result.
- [x] Cover switch, dimmer, and cover hardware on macOS Apple Silicon.
- [x] Ensure Linux x86_64 CI runs format, Clippy, tests, and migrations.
- [x] Run a plaintext-secret scan over fixtures and captured diagnostics.
- [x] Link evidence to every EPIC-001 acceptance criterion and exit item.
- [x] Update EPIC-002 with finalized repository, event, and credential contracts.

## Acceptance criteria

- [x] A clean-checkout operator can back up and restore an installation.
- [x] Smoke reports are reproducible and contain no secrets.
- [x] Required hardware evidence is committed or linked with exact versions.
- [x] Supported-platform quality gates pass.
- [x] Every EPIC-001 checklist item is either evidenced or explicitly unresolved.

## Verification

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [x] `cargo test --workspace --all-features --locked`
- [x] Documentation link and command smoke tests.
- [x] Requirement-by-requirement EPIC-001 completion audit.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Added validated backup/restore and stdin-only credential
  provisioning commands plus complete operator recovery documentation.
- 2026-07-11: Generated a redacted macOS ARM report covering switch, dimmer, and
  cover read paths across 43 observed devices on firmware 1.7.5.
- 2026-07-11: Added Linux x86_64 CI migration and secret-scan gates with required
  D-Bus build packages.
- 2026-07-11: Exit audit completed; live Linux CI remains the only unresolved
  supported-platform gate.
- 2026-07-12: Closed the supported-platform gate with a read-only official Rust
  container running on `x86_64-unknown-linux-gnu`. Format, strict all-target
  Clippy, the complete workspace/all-features suite, doc tests, and all five
  migration fixtures passed.
