---
id: E1-005
epic: EPIC-001
title: Add secret-safe Shelly digest authentication
status: ready
priority: high
depends_on: [E1-001, E1-002]
adrs: [ADR-0008]
created: 2026-07-11
updated: 2026-07-11
---

# E1-005: Shelly Authentication

## Outcome

Authenticated Shelly Gen2+ devices can be enrolled and refreshed using opaque
credential references, while plaintext and digest material stay outside durable
domain snapshots and observable diagnostics.

## Tasks

- [x] Define the secret-store application port and credential reference type.
  Evidence: `SecretStore`, zeroizing `SecretValue`, and domain-owned `SecretRef`.
- [x] Implement configured platform and headless secret adapters. Evidence:
  `homemagic-secrets` isolates macOS Keychain, Linux Secret Service, and the
  explicit XChaCha20-Poly1305 file vault.
- [ ] Implement Shelly RPC digest challenge parsing and response generation.
- [ ] Support nonce refresh and bounded reauthentication.
- [ ] Represent missing, rejected, and unavailable credentials as repair states.
- [ ] Redact passwords, nonces, headers, and digest response material.
- [ ] Add sanitized authentication fixtures across firmware variations.

## Acceptance criteria

- [ ] Authenticated info, config, and status RPC calls succeed in fixtures.
- [x] Device persistence contains only a credential reference. Evidence:
  `IntegrationInstance::credential_ref` serializes only an opaque `SecretRef`.
- [ ] Rejected credentials create a diagnostic and actionable repair.
- [ ] Credential material cannot be found in logs, RPC, fixtures, or snapshots.

## Verification

- [ ] Digest algorithm vector tests.
- [ ] Challenge, retry, stale nonce, and rejection integration tests.
- [ ] Repository-wide credential leak test over captured output.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Added the secret-store port, platform adapters, explicit headless
  vault, zeroizing secret values, permission/ownership validation, associated
  data binding, and secret-canary tests. Full locked Clippy and workspace tests
  pass; Shelly digest transport remains in progress.
