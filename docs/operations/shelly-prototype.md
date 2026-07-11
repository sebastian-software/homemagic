# HomeMagic Device Foundation Operations

## Supported hosts

- macOS on Apple Silicon;
- Linux on x86_64;
- Rust 1.85 or newer for source builds.

The host and Shelly Gen2+ devices must share an mDNS-reachable network. macOS
may request Local Network permission. The runtime is read-only through EPIC-001.

## Database and migrations

The default database is `homemagic.sqlite3` relative to the process working
directory. Production deployments should set an absolute path:

```sh
export HOMEMAGIC_DATABASE=/var/lib/homemagic/homemagic.sqlite3
```

HomeMagic enables foreign keys, WAL mode, and a bounded busy timeout whenever it
opens the database. Forward-only migrations run before the API starts. A schema
newer than the binary or a changed historical migration checksum stops startup;
the database is not silently rewritten.

Authenticated `system.health` exposes the schema version, integrity result, WAL
state, and retained event cursor bounds. Unauthenticated `GET /health` exposes
only process liveness and package version.

## Backup and restore

Create a consistent online backup. The destination is validated before it is
atomically replaced:

```sh
cargo run --locked -- backup \
  --database /var/lib/homemagic/homemagic.sqlite3 \
  /secure-backups/homemagic.sqlite3
```

Restore into an inactive path. The source remains unchanged; the copy is
migrated, integrity-checked, and atomically installed:

```sh
cargo run --locked -- restore \
  /secure-backups/homemagic.sqlite3 \
  /var/lib/homemagic/homemagic-restored.sqlite3
```

Stop the daemon, retain the old database, then point `HOMEMAGIC_DATABASE` at the
restored file and verify `system.health` before removing the old copy. Never copy
only the live SQLite main file while the daemon is running; use `backup` so WAL
state is included consistently.

## Credential provisioning and recovery

Shelly uses the fixed digest username `admin`; HomeMagic stores only the password
in a secret backend. The database contains an opaque `SecretRef`, never plaintext.
The provisioning command reads the password only from stdin.

### macOS Keychain or Linux Secret Service

The platform backend is the default:

```sh
read -rsp 'Shelly password: ' SHELLY_PASSWORD
printf '%s' "$SHELLY_PASSWORD" | cargo run --locked -- credential-set-shelly \
  --database /var/lib/homemagic/homemagic.sqlite3 \
  --secret-store platform
unset SHELLY_PASSWORD
```

The shell variable is not exported. A local secret manager can instead pipe its
value directly to the command. Do not pass the password as a command-line
argument or environment variable. On
macOS, the item is stored in Keychain. On desktop Linux, it uses Secret Service.
Re-run the command to rotate or repair a rejected credential.

### Explicit encrypted-file mode

Headless Linux without Secret Service must opt into the encrypted file vault.
Create one owner-only 32-byte master key and keep it separate from the vault and
database backups:

```sh
umask 077
openssl rand -out /etc/homemagic/master.key 32
```

```sh
read -rsp 'Shelly password: ' SHELLY_PASSWORD
printf '%s' "$SHELLY_PASSWORD" | cargo run --locked -- credential-set-shelly \
  --database /var/lib/homemagic/homemagic.sqlite3 \
  --secret-store file \
  --master-key-file /etc/homemagic/master.key \
  --secret-vault /var/lib/homemagic/secrets
unset SHELLY_PASSWORD
```

Start `serve` with the same backend, key, and vault options. Losing the master key
makes the vault unrecoverable; restore the key from its separate protected backup
or provision the Shelly password again into a new vault.

## Server and RPC diagnostics

```sh
RUST_LOG=info cargo run --locked -- serve
```

The default bind address is loopback only. The daemon loads durable state before
network discovery, then runs periodic reconciliation, freshness evaluation,
managed WebSocket sessions, and bounded recovery.

On a new database, let the daemon create its installation, then bootstrap the
first actor from another terminal. The 256-bit bearer token is printed once;
store it in a local secret manager. SQLite retains only an Argon2id hash.

```sh
cargo run --locked -- actor-bootstrap \
  --database /var/lib/homemagic/homemagic.sqlite3 \
  --name local-agent
```

Rotate or disable an actor without deleting its audit identity:

```sh
cargo run --locked -- actor-rotate \
  --database /var/lib/homemagic/homemagic.sqlite3 ACTOR_ID

cargo run --locked -- actor-disable \
  --database /var/lib/homemagic/homemagic.sqlite3 ACTOR_ID
```

Set the one-time token in a non-exported shell variable for local diagnostics:

```sh
HOMEMAGIC_TOKEN='hm1.ACTOR_ID.RANDOM_SECRET'

curl -s http://127.0.0.1:8787/health

curl -s http://127.0.0.1:8787/rpc \
  -H "authorization: Bearer $HOMEMAGIC_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"devices.list","params":{}}'

curl -s http://127.0.0.1:8787/rpc \
  -H "authorization: Bearer $HOMEMAGIC_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"repairs.list","params":{}}'
```

See [the JSON-RPC contract](../api/json-rpc.md) for filters, device details,
metadata operations, repairs, and cursor-based event subscriptions.

## Redacted hardware smoke test

The command reads device identity, configuration, and status but sends no
state-changing RPC. It omits device/native IDs, addresses, aliases, spaces, and
vendor payloads.

```sh
cargo run --locked -- hardware-smoke \
  --discovery-seconds 8 \
  --output docs/evidence/hardware/YYYY-MM-DD-macos-arm64-shelly.json
```

The committed 2026-07-11 macOS ARM report observed 43 devices on an `aarch64`
host, all with firmware `1.7.5`, including:

| Coverage | Observed model | Normalized evidence |
| --- | --- | --- |
| switch | `S3SW-001P8EU` | `on_off.v1`, power, energy |
| dimmer | `S3DM-0A101WWL` | `on_off.v1`, `level.v1`, power, energy |
| cover | `S3SW-002P16EU`, `SNSW-102P16EU` | `position.v1`, power, energy |

Evidence: [redacted macOS ARM report](../evidence/hardware/2026-07-11-macos-arm64-shelly.json)
and [report schema](hardware-smoke-report.md). This is read-path compatibility
evidence, not physical command or safety validation.

## Validation and secret scan

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test -p homemagic-storage --test migration_fixtures --locked
./scripts/scan-secrets.sh
```

Linux x86_64 CI is configured to run all five gates after installing the native
Secret Service build dependencies. Its live result is tracked separately from
the locally verified macOS ARM report and quality gate.

## Remaining limitations

- no API authentication or authorization before EPIC-002;
- no state-changing commands before EPIC-002;
- no Shelly Gen1/CoIoT adapter;
- no cloud relay or remote access;
- event history is operational, not analytical time-series storage.
