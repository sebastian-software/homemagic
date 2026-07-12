# ADR-0037: Own Matter fabrics and export them only through protected envelopes

- Status: Accepted
- Date: 2026-07-12
- Deciders: HomeMagic maintainers
- Related: ADR-0005, ADR-0007, ADR-0008, EPIC-004, E4-001

## Context

A Matter controller owns fabric identity, operational credentials, trust roots,
node relations, and protocol storage that must survive restart. Losing the fabric
can make commissioned devices unreachable, while copying it insecurely would
turn an ordinary database backup into an access credential.

HomeMagic must run as a macOS user process and a headless Linux service without
creating a second secret-store policy or silently weakening ADR-0008.

## Decision

The first production version owns one active HomeMagic fabric per installation.
The application owns stable fabric/node metadata and lifecycle transactions;
the selected adapter performs protocol operations through `MatterController`.

Live secret material is split into individually addressable values behind the
existing async `SecretStore`. SQLite stores only opaque `SecretRef` values.
macOS Keychain, Linux Secret Service, and the explicitly configured headless
encrypted vault remain exactly as defined by ADR-0008. There is no automatic
backend change or plaintext fallback.

Secret creation precedes metadata attachment. A durable operation records the
pending secret references, then atomically attaches them to the fabric. Failed
commissioning, rotation, node removal, or restore records cleanup work as
`repair_required`; it never silently drops metadata while remote or secret-store
outcomes are unknown.

### Portable export

An ordinary SQLite/data-directory backup excludes usable fabric secrets. Fabric
portability is a separate explicit authenticated operation.

HomeMagic creates a versioned canonical export payload containing only the
minimum fabric credentials, trust material, fabric/node identity relations, and
format metadata needed for restore. It generates a random 256-bit recovery key
from the operating-system CSPRNG and encrypts the payload with
XChaCha20-Poly1305 using a fresh 192-bit nonce. The immutable envelope header,
format version, cipher identifier, installation/fabric identity, and payload
length are authenticated as associated data.

The recovery key is returned once through an explicitly sensitive response and
is never logged, placed in events, persisted automatically, included beside the
export, or copied into a normal backup. HomeMagic does not support a weaker
passphrase-derived export mode in the first format.

Restore requires the envelope and recovery key as sensitive input. It verifies
format, bounds, authenticated header, payload integrity, secret references, and
ownership conflicts before changing active state. Restore stages new secrets,
verifies the controller can load the fabric, and then atomically activates the
metadata. Failure leaves the prior active fabric unchanged or exposes explicit
repair state for staged resources.

A fabric already active under another identity is never merged automatically.
A clean-data-directory restore preserves exported stable identities and rejects
duplicate active ownership.

Simulator credentials and exports carry a distinct format/type marker and are
unconditionally rejected by production restore.

## Consequences

- Database backups do not silently become Matter controller credentials.
- Users must separately protect a high-entropy recovery key and export artifact.
- Losing both live secret-store entries and the recovery material is not
  recoverable by HomeMagic.
- Export/restore requires careful crash, corruption, duplicate, and redaction
  testing.
- SDK persistence callbacks must adapt to HomeMagic ownership rather than own an
  opaque untracked store.
