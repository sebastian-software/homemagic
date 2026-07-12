# Automation Engine Threat Model

## Scope and assets

This model covers authenticated automation authoring, validation, deterministic
simulation, approval, activation, scheduling, event consumption, runtime
execution, run traces, explicit catch-up, operational recovery, and durable
event streaming. Protected assets are physical safety, household access and
privacy, actor identity, immutable authored intent, approval evidence, command
policy, causation history, availability, and credentials held outside the
automation model.

## Trust boundaries

1. An RPC or future MCP client crosses an authenticated transport boundary.
2. Transport code passes the authenticated `Actor` into one
   `AutomationLifecycleService`; JSON cannot supply or replace that actor.
3. Authored data crosses the compiler boundary and becomes an immutable,
   bounded execution plan resolved at one registry revision.
4. Synthetic history crosses into the simulator, which owns no dispatcher.
5. Active plans cross into the durable scheduler and step interpreter.
6. Every physical action crosses the existing `CommandService` boundary with
   current actor grants, policy, idempotency, deadlines, and audit.
7. Version, operational, and run facts cross into the global durable event
   cursor; WebSocket delivery applies actor ownership before serialization.

The engine never accepts source code, templates, raw vendor calls, dynamic
plugins, database queries, or a dispatcher supplied by the authoring client.

## Threats and controls

| Threat | Control | Residual risk |
| --- | --- | --- |
| Actor spoofing | Actor derives only from the bearer context; document ownership is enforced below transport; catch-up also crosses this boundary | A stolen live bearer retains its grants until disabled |
| Agent-authored arbitrary execution | Versioned data-only IR, fixed node enum, no loops/recursion/code/template/FFI node, hard document and plan bounds | Future IR extensions require security review |
| Unsafe target resolution | Exact schema validation at one registry revision; missing, stale, ambiguous, and incompatible references fail with paths | Device state can change after validation and is rechecked by command policy |
| Approval bypass | Compiler derives Safety Profiles; sensitive versions require approval bound to exact document and plan hashes; activation rechecks evidence atomically | The same household actor may author and approve by design |
| Simulation causes physical work | Simulator constructs internal IDs and has no dispatcher dependency; command outcomes are caller-declared data | Simulation quality depends on supplied history |
| Runtime bypasses command policy | Runtime owns only `CommandService` and common capability payloads; no adapter handle or raw RPC path exists | A defect inside CommandService remains in the trusted base |
| Duplicate action after crash | Deterministic occurrence/run/timer/command keys, pre-dispatch command persistence, durable attempt state, no redispatch after dispatch boundary | Physical device ambiguity still requires observation |
| Command storm or feedback loop | Same-segment last-state reduction, self-causation suppression, single/restart/queue/parallel bounds, retry and duration limits | An explicitly allowed external event cycle can still consume bounded capacity |
| Silent missed-run replay | Scheduler records expired occurrences as skipped; only one exact authenticated `catch_up` request can materialize a missed instant | Operator can deliberately request an undesirable catch-up |
| Cancellation mistaken for physical rollback | Run cancellation stops eligible undispatched work and timers only; dispatched commands remain facts | Physical compensation is a separate governed command |
| Rollback mistaken for device compensation | Rollback changes only the active immutable version pointer | Existing runs and prior physical effects require explicit handling |
| Cross-actor history disclosure | Draft/version/run queries enforce ownership; automation WebSocket events are owner-filtered; errors omit internals | Global cursor gaps reveal only that some hidden event existed |
| Secret or vendor-data disclosure | Automation documents, plans, traces, events, and stable errors have no credential field; events exclude authored text, plans, traces, and vendor payloads | User-authored rationale may itself contain sensitive prose and remains visible to its owner |
| Resource exhaustion | Bounded documents, nodes, depth, branches, queue, traces, run duration, query pages, event pages, and one-step engine passes | Many authorized bounded automations consume aggregate host capacity |
| Local database tampering | Forward-only checksummed migrations, immutable versions/approvals, optimistic revisions, transaction invariants | The local database owner can replace files; signed remote audit is not provided |

## Security invariants

- Only a validated and successfully simulated exact version can become ready.
- Sensitive versions cannot activate without exact user approval.
- Every active pointer mutation is optimistic, atomic, and durably evented.
- Every run identifies its immutable version, occurrence, actor, correlation,
  and optional causation event.
- Every physical action uses the shared command policy and audit path.
- Restart and ordinary scheduler passes never infer permission to replay missed
  schedule instants.
- Cancellation, disable, rollback, retirement, and physical compensation remain
  distinct operations.
- No token, credential, raw vendor payload, or unauthenticated actor input has a
  serialization path into automation events or RPC errors.

## Review triggers

Revisit this model before adding locks, valves, cameras, Matter fabrics, remote
listeners, cloud relays, new Safety Profiles, code-like IR nodes, dynamic
plugins, automatic catch-up, cross-installation actors, event sharing, new FFI,
or any adapter path reachable without `CommandService`.
