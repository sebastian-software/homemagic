---
id: E1-005
epic: EPIC-001
title: Add secret-safe Shelly digest authentication
status: done
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
- [x] Implement Shelly RPC digest challenge parsing and response generation.
  Evidence: `homemagic-shelly/src/auth.rs` implements the documented SHA-256
  HA1/HA2 formula and strict challenge parsing.
- [x] Support nonce refresh and bounded reauthentication. Evidence: the HTTP
  transport performs one challenge response and at most one `stale=true` retry.
- [x] Represent missing, rejected, and unavailable credentials as repair states.
  Evidence: discovery candidates carry stable condition-derived repair records.
- [x] Redact passwords, nonces, headers, and digest response material. Evidence:
  secret and challenge debug output plus public adapter errors expose stable
  codes rather than protocol material.
- [x] Add sanitized authentication fixtures across firmware variations.
  Evidence: modern base64 and legacy numeric nonce challenge fixtures.

## Acceptance criteria

- [x] Authenticated info, config, and status RPC calls succeed in fixtures.
  Evidence: public device-info and authenticated RPC projection fixtures plus
  the local challenge-response transport integration test.
- [x] Device persistence contains only a credential reference. Evidence:
  `IntegrationInstance::credential_ref` serializes only an opaque `SecretRef`.
- [x] Rejected credentials create a diagnostic and actionable repair. Evidence:
  `authentication_failure_should_create_stable_actionable_repair`.
- [x] Credential material cannot be found in logs, RPC, fixtures, or snapshots.
  Evidence: redaction, serialization, and encrypted-envelope canary assertions.

## Verification

- [x] Digest algorithm vector tests. Evidence:
  `digest_should_match_independent_shelly_formula_vector`.
- [x] Challenge, retry, stale nonce, and rejection integration tests. Evidence:
  local HTTP tests assert two normal requests, three stale-nonce requests, and
  bounded rejection after two requests.
- [x] Repository-wide credential leak test over captured output. Evidence:
  secret/debug diagnostics, serialized candidates, fixtures, and encrypted
  vault envelopes are covered by credential-canary assertions.

## Progress log

- 2026-07-11: Issue created.
- 2026-07-11: Added the secret-store port, platform adapters, explicit headless
  vault, zeroizing secret values, permission/ownership validation, associated
  data binding, and secret-canary tests. Full locked Clippy and workspace tests
  pass; Shelly digest transport remains in progress.
- 2026-07-11: Completed SHA-256 digest auth, bounded stale-nonce renewal,
  runtime backend selection, stable authentication repairs, sanitized firmware
  fixtures, and diagnostic redaction. Full locked format, Clippy, workspace
  tests, and doctests pass.
