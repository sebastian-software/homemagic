# ADR-0007: Own SQLite schemas and use forward-only migrations

- Status: Accepted
- Date: 2026-07-11

## Context

EPIC-001 requires stable device identity and current observations to survive
restarts. The embedded database must be recoverable, upgradeable from every
released schema, and explicit about what happens when a binary encounters an
older, damaged, or newer database.

SQLite is mature and available on both supported targets, but its C API crosses
the FFI boundary governed by ADR-0005. Migration and backup policy must therefore
cover both data compatibility and the native dependency boundary.

## Decision

The `homemagic-storage` crate exclusively owns the SQLite schema, migrations,
connections, transactions, and backup operations. Other crates depend on
application repository traits and never issue SQL.

The first implementation uses `rusqlite` with a bundled, pinned SQLite build.
Synchronous database work runs on a dedicated blocking executor; async runtime
workers never call SQLite directly.

Every connection enables:

- WAL journal mode;
- foreign-key enforcement;
- a five-second busy timeout;
- defensive application-level size and concurrency bounds.

### Migration policy

Migrations are immutable, monotonically numbered SQL resources embedded in the
binary. A migration ledger stores its number, name, content checksum, and
application timestamp. The current schema version is also exposed through
`system.health`.

Every released schema has a committed database fixture. CI opens each fixture,
applies all later migrations, runs `PRAGMA integrity_check`, and executes the
repository contract suite.

Schema changes are classified as follows:

- **Compatible** changes are additive or semantics-preserving. They can be
  applied automatically in one startup migration without making the previous
  persisted data unreadable during the migration transaction.
- **Destructive** changes remove, reinterpret, or rewrite persisted information.
  They require an expand/migrate/contract sequence, a pre-migration backup, and
  a release note naming the oldest supported source schema. Contract steps do
  not share a migration with the initial expansion.
- **Unsupported** inputs include a database created by a newer schema version,
  a migration checksum mismatch, a schema older than the declared migration
  floor, and a database that fails integrity validation. HomeMagic refuses to
  mutate these databases and returns a structured recovery error.

Migrations are forward-only. HomeMagic does not implement down migrations.
Rollback means restoring a validated backup with a binary that supports that
backup's schema.

### Backup and restore contract

Online backup uses the SQLite Online Backup API to copy a consistent snapshot
into a temporary file in the destination directory. HomeMagic then opens the
copy, runs `integrity_check`, verifies the migration ledger, and exercises a
read-only repository health query before atomically renaming it to the requested
backup path. A failed validation never replaces an existing backup.

Restore is offline with respect to repository writes. HomeMagic validates the
candidate in a separate location, migrates it when supported, validates it
again, and only then atomically replaces the inactive database. The previous
database is retained as a recovery backup until the restored runtime starts
successfully.

## FFI exception review

- **Missing Rust capability:** no production-proven, pure-Rust SQLite engine
  provides SQLite file, WAL, migration, and online-backup compatibility.
- **Boundary:** only `homemagic-storage` depends on `rusqlite`; first-party code
  contains no unsafe block for normal database operations.
- **Trust and memory safety:** `libsqlite3-sys` owns the unsafe bindings and the
  pinned SQLite amalgamation is built by Cargo. Values cross the boundary through
  `rusqlite`'s safe API.
- **Platforms and packaging:** the bundled build is tested on macOS ARM64 and
  Linux x86_64 and avoids reliance on an unknown system SQLite version.
- **Conformance and upgrades:** repository contracts, historical migrations,
  backup/restore tests, and integrity checks gate SQLite dependency upgrades.
- **Replacement criterion:** reconsider the exception when a maintained
  pure-Rust engine passes the same file-compatibility, WAL, transactional,
  integrity, and online-backup contract on both supported platforms.

## Consequences

- Application and integration code remain independent of SQL and SQLite APIs.
- Startup may fail deliberately rather than risk mutating an unknown schema.
- Bundling SQLite increases binary size and native build surface.
- A validated backup is the supported rollback mechanism.

## References

- [SQLite Online Backup API](https://www.sqlite.org/backup.html)
- [SQLite Online Backup API functions](https://www.sqlite.org/c3ref/backup_finish.html)
