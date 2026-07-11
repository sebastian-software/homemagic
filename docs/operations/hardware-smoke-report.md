# Hardware Smoke Report Schema

`homemagic hardware-smoke` emits `homemagic.hardware_smoke.v1` JSON.

## Top-level fields

| Field | Type | Meaning |
| --- | --- | --- |
| `schema` | string | Exact report contract identifier |
| `generated_at` | RFC 3339 timestamp | UTC report time |
| `host.operating_system` | string | Rust target operating system |
| `host.architecture` | string | Rust target architecture |
| `integration` | string | Adapter under test |
| `discovery_seconds` | integer | Configured bounded discovery window |
| `device_count` | integer | Number of redacted observations |
| `devices` | array | Compatibility results described below |
| `redaction` | string | Fields deliberately omitted |

## Device result fields

| Field | Type | Meaning |
| --- | --- | --- |
| `manufacturer` | string | Reported manufacturer |
| `model` | string | Reported hardware model |
| `firmware` | string or null | Reported firmware version |
| `capabilities` | sorted string array | Normalized versioned schemas observed |
| `result` | string | `state_observed` or `identity_observed_authentication_required` |
| `count` | integer | Number of identical redacted observations in this group |

Reports must not contain device IDs, native IDs, MAC addresses, IP addresses,
hostnames, aliases, space assignments, credentials, digest material, or raw
vendor payloads. `scripts/scan-secrets.sh` is a required CI gate for committed
reports and fixtures.
