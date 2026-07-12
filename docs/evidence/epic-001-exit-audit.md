# EPIC-001 Exit Audit

- Audit date: 2026-07-12
- Local host: macOS `aarch64`
- Epic status: Done

## Acceptance criteria

| Criterion | Status | Evidence |
| --- | --- | --- |
| AC1 stable identities after restart | Pass | `repository_should_preserve_stable_device_id_across_reopen`, `bootstrap_should_reuse_identities_after_reopen`, and normalized endpoint/capability persistence tests |
| AC2 push state reaches registry and event stream | Pass | authenticated local Shelly WebSocket fixtures, durable `RepositoryLiveObservationSink`, and ordered `/rpc/ws` integration test |
| AC3 authenticated observation without password exposure | Pass | digest challenge/reauthentication tests, opaque `SecretRef`, zeroizing secret values, encrypted/platform backends, and captured-error canaries |
| AC4 discovery misses preserve known devices | Pass | `discovery_miss_should_not_change_known_device` |
| AC5 disconnect, backoff, stale, reconnect, and recovery are structured | Pass | deterministic backoff/reconnect tests, freshness offline/recovery test, `DeviceDetails`, repairs, and cursor event stream |
| AC6 clean migration and backup/restore | Pass on macOS ARM | all historical migration fixtures, storage backup/restore tests, and successful CLI backup/restore smoke with schema 1 and integrity `ok` |
| AC7 exact hardware compatibility evidence | Pass for read path | [redacted report](hardware/2026-07-11-macos-arm64-shelly.json) records macOS ARM, firmware 1.7.5, model, normalized capabilities, result, and grouped count |

## Exit items

| Exit item | Status | Evidence or unresolved condition |
| --- | --- | --- |
| Every acceptance criterion is linked | Pass | Table above |
| Required ADRs accepted and indexed | Pass | ADR-0007 through ADR-0012 and `docs/adr/README.md` |
| Operator and API documentation match behavior | Pass | `docs/operations/shelly-prototype.md`, hardware report schema, and `docs/api/json-rpc.md` |
| No plaintext secret in fixtures or diagnostics | Pass | `scripts/scan-secrets.sh` completed without matches after the hardware report was generated |
| macOS ARM full workspace quality gate | Pass | format, strict Clippy, workspace tests, migration tests, and doc tests completed locally |
| Linux x86_64 full workspace quality gate | Pass | Official `rust:1-bookworm` image under `x86_64-unknown-linux-gnu` Rust 1.97.0: format, strict all-target Clippy, complete workspace tests/all features, doc tests, and all five migration fixtures passed |
| EPIC-002 references finalized contracts | Pass | `docs/epics/002-safe-command-control-plane.md` foundation-contract section |

## Hardware evidence summary

The read-only smoke command observed 43 devices and grouped them without stable
or native IDs, addresses, aliases, spaces, or vendor payloads:

- switch: `S3SW-001P8EU`, firmware 1.7.5, `on_off.v1` plus power and energy;
- dimmer: `S3DM-0A101WWL`, firmware 1.7.5, `level.v1` and `on_off.v1` plus power and energy;
- cover: `S3SW-002P16EU` and `SNSW-102P16EU`, firmware 1.7.5, `position.v1` plus power and energy.

This proves discovery, identity/configuration reads, state reads, and normalized
projection. State-changing hardware validation belongs to EPIC-002 and must
restore every device to its original state.

## Local command evidence

The following commands completed successfully on macOS ARM:

```sh
cargo run --locked -- backup --database /tmp/homemagic-e1-009-source.sqlite3 /tmp/homemagic-e1-009-backup.sqlite3
cargo run --locked -- restore /tmp/homemagic-e1-009-backup.sqlite3 /tmp/homemagic-e1-009-restored.sqlite3
cargo run --locked -- credential-set-shelly --database /tmp/homemagic-e1-009-credentials.sqlite3 --secret-store file --master-key-file /tmp/homemagic-e1-009-master.key --secret-vault /tmp/homemagic-e1-009-vault
cargo run --locked -- hardware-smoke --discovery-seconds 8 --output docs/evidence/hardware/2026-07-11-macos-arm64-shelly.json
./scripts/scan-secrets.sh
```

The original direct cross-target attempt could not supply a Linux C compiler or
D-Bus sysroot from macOS. On 2026-07-12 the unchanged read-only checkout was
instead validated under an isolated official Rust Linux x86_64 container with
`libdbus-1-dev` and `pkg-config`, matching the CI dependency contract.
