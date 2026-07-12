# Automation Operations and Recovery

## Inspect before changing anything

Use the authenticated operational aggregate after every reconnect. It supplies
the current state, active version, and optimistic revision; never guess a
revision from an earlier response.

```json
{"jsonrpc":"2.0","id":1,"method":"automations.list","params":{"limit":50}}
```

```json
{"jsonrpc":"2.0","id":2,"method":"automations.get","params":{"automation_id":"AUTOMATION_ID"}}
```

Inspect recent runs and then page the selected run's trace. A trace cursor is
run-local and `after_sequence` is exclusive.

```json
{"jsonrpc":"2.0","id":3,"method":"automations.runs.list","params":{"automation_id":"AUTOMATION_ID","limit":50}}
```

```json
{"jsonrpc":"2.0","id":4,"method":"automations.runs.trace","params":{"run_id":"RUN_ID","after_sequence":12,"limit":50}}
```

Also inspect referenced commands with `commands.get` and `commands.audit`.
Transport acknowledgement is not proof that the intended physical state was
reached.

## Stuck or unexpected run

1. Query `automations.get` and record its active version and revision.
2. Disable new trigger admission with that exact revision.
3. List non-terminal runs and inspect each trace and command audit.
4. Cancel a run only after checking whether it already submitted commands.
5. Verify current physical and observed device state.
6. Decide whether to fix forward, reactivate a reviewed version, or retire.

```json
{"jsonrpc":"2.0","id":5,"method":"automations.disable","params":{"automation_id":"AUTOMATION_ID","expected_revision":7}}
```

```json
{"jsonrpc":"2.0","id":6,"method":"automations.runs.cancel","params":{"run_id":"RUN_ID"}}
```

Disable prevents new event and schedule admission. It does not cancel an
already accepted run. Run cancellation atomically marks the run cancelled,
cancels its pending or ready timers, and appends an outcome trace. It does not
undo a command that crossed the dispatch boundary. Any physical compensation is
a new governed command with a new idempotency key.

## Roll back a bad active version

Retrieve the older immutable version and confirm that it is still `ready` and
was validated/simulated against understood registry evidence. Then activate it
through rollback using the current operational revision:

```json
{"jsonrpc":"2.0","id":7,"method":"automations.rollback","params":{"automation_id":"AUTOMATION_ID","version":2,"expected_revision":8}}
```

Rollback atomically changes the active pointer. It does not cancel runs created
by the replaced version and does not reverse physical effects. Disable first
when diagnosing unexpected behavior, cancel eligible old runs separately, and
compensate physical state only through `CommandService`.

## Deliberate catch-up

HomeMagic never replays a missed schedule automatically. Catch-up accepts one
exact missed instant plus an actor-scoped idempotency key. It rejects an instant
whose normal occurrence window remains open or an inactive automation.

```json
{"jsonrpc":"2.0","id":8,"method":"automations.catch_up","params":{"automation_id":"AUTOMATION_ID","scheduled_for":"2026-07-12T16:00:00Z","idempotency_key":"operator-reviewed-2026-07-12T16:00:00Z"}}
```

Before requesting catch-up, inspect the version, simulation evidence, current
device state, and intervening command history. Reusing the same key and instant
returns the existing occurrence; use a different key only for a genuinely
different operator decision.

## Retirement and incident evidence

Retirement is permanent for the identity. It blocks later activation but does
not erase versions, approvals, runs, traces, commands, or transition events.

```json
{"jsonrpc":"2.0","id":9,"method":"automations.retire","params":{"automation_id":"AUTOMATION_ID","expected_revision":9}}
```

For an incident retain the automation ID/version, operational revisions, run
and occurrence IDs, trace sequences, command IDs/audit, correlation and
causation IDs, timestamps, policy outcomes, observed state, and event cursors.
Do not retain bearer credentials, device credentials, network addresses, raw
vendor payloads, or unrelated authored household text.

If motion, access, energy, or material flow is unsafe, use the physical control
or isolation path first. Network RPC is never the sole emergency control.
