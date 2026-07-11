# ADR-0013: Authenticate RPC clients as durable actors

- Status: Accepted
- Date: 2026-07-11

## Context

EPIC-002 introduces physical mutations. Client-supplied actor strings are useful
causation hints for read-only metadata, but cannot authorize commands. macOS ARM
and Linux x86_64 need one headless-compatible local mechanism before remote access
or federated identity is considered.

## Decision

HomeMagic uses durable actor records and random 256-bit bearer tokens for the
first command control plane. A bootstrap CLI creates an actor, generates a token
from the operating-system RNG, prints it once, and stores only an Argon2id hash
with a unique salt. Tokens never enter SQLite, logs, events, command records, or
diagnostics.

`POST /rpc` and `/rpc/ws` require `Authorization: Bearer`. Authentication derives
the actor used by application services; request parameters cannot override it.
The minimal HTTP liveness endpoint remains unauthenticated and returns no device,
repository, or security details. `system.health` is authenticated.

Actors can be disabled without deleting audit history. Verification is bounded
and performed away from async executor threads. Authentication failures use one
generic response and do not reveal whether an actor or token exists.

## Consequences

- Every mutation has a non-spoofable local actor.
- Token distribution and TLS for untrusted networks remain operator concerns;
  the server stays loopback-first.
- Future mTLS, passkeys, or OS peer credentials may map to the same actor model.
