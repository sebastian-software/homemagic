# ADR-0008: Store device credentials behind platform secret stores

- Status: Accepted
- Date: 2026-07-11

## Context

Authenticated device integrations require credentials, but device snapshots,
automation documents, diagnostics, backups, and RPC responses must not contain
plaintext secrets. macOS provides Keychain Services and desktop Linux commonly
provides the Secret Service API. Headless Linux may provide neither.

Silently writing plaintext when a platform service is unavailable would violate
the security boundary. Refusing all headless deployments would make normal local
server operation impractical.

## Decision

The application defines an async `SecretStore` port whose public operations use
an opaque `SecretRef`. Domain and persistence records store only the reference.
Only an integration adapter resolving a credential for an immediate protocol
operation receives secret bytes.

HomeMagic supports three explicit backends:

1. **macOS:** the user's Keychain through a Rust keyring adapter backed by
   Keychain Services.
2. **Linux desktop:** the freedesktop.org Secret Service API over D-Bus using a
   Rust D-Bus client.
3. **Headless Linux:** an encrypted file vault, enabled only by explicit
   configuration. The vault uses a versioned XChaCha20-Poly1305 envelope with a
   random nonce per write and binds the `SecretRef` as associated data.

There is no automatic downgrade from a platform secret store to the file vault.
If the selected backend is unavailable, authenticated integrations remain
inactive and expose a structured repair record.

### Headless key provisioning

Headless mode requires `HOMEMAGIC_SECRET_STORE=file` and
`HOMEMAGIC_MASTER_KEY_FILE`. The master-key file contains a randomly generated
256-bit key, must not be inside the HomeMagic data directory or its backups, and
must be owned by the runtime user with no group or world permissions. HomeMagic
refuses startup of the file backend if these requirements fail.

Deployments should provision the file from an operating-system, container, or
orchestrator secret mount. HomeMagic never accepts the master key as a command
line argument and does not generate or persist a recovery copy silently.

### Secret lifecycle and redaction

- A secret is created before its reference is attached transactionally to an
  integration or device.
- Rotation writes a new secret, swaps the durable reference, verifies access,
  and then deletes the old secret.
- Removing a device does not immediately delete a shared integration secret.
- Secret values are wrapped in zeroizing memory containers and are never
  serializable.
- Error types expose backend, operation, and stable error code but not secret
  values, nonces, authorization headers, or digest response material.
- Logging fields, RPC responses, diagnostics, fixtures, and database backups are
  covered by automated secret-canary tests.

## FFI and Rust policy

The Linux Secret Service and headless vault implementations use Rust protocol
and cryptography crates. The macOS implementation uses the operating-system
Keychain API through maintained Rust bindings. This is a selective FFI exception
under ADR-0005:

- macOS Keychain is the system security boundary and no pure-Rust replacement
  can provide equivalent user access control or keychain integration;
- the FFI boundary is isolated inside the secret-store adapter and first-party
  code introduces no unsafe block;
- macOS ARM64 keychain contract tests and locked/unavailable-keychain tests gate
  dependency upgrades;
- the adapter can be replaced if Apple exposes an equally capable memory-safe
  system interface or a maintained Rust implementation removes the binding.

## Consequences

- Device and database backups remain useful without becoming credential backups.
- Headless installations need a separately managed master key.
- Losing both the platform entry and the device password requires credential
  re-enrollment; HomeMagic cannot recover the plaintext.
- Secret backend failures are operational repair states, not generic device
  disappearance.

## References

- [Apple Keychain Services](https://developer.apple.com/documentation/security/keychain-services)
- [Secret Service API](https://specifications.freedesktop.org/secret-service/latest/)
