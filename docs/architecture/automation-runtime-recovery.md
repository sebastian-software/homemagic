# Automation Runtime Recovery Keys

E3-006 derives durable work identities from source facts instead of process-local
randomness:

- occurrence ID = automation ID + immutable version + normalized source key;
- run ID = accepted occurrence ID;
- timer ID = run ID + plan node ID + absolute ready instant;
- trace ID = run ID + contiguous sequence.

Reprocessing the same event cursor, schedule instant, accepted occurrence, or
timer after restart therefore addresses the existing row. Repository create
operations remain idempotent only when the complete payload matches, so a key
collision cannot silently replace different work.

The runtime repository also exposes stable-order bounded queries for active
identity/version pairs and direct run/timer lookup. Startup recovery first loads
active immutable plans, then pending work, and confirms individual run/timer
state before attempting continuation. This is the base contract for the durable
step coordinator; it does not itself authorize or dispatch commands.
