# Command Recovery and Physical Safety

## Before enabling commands

Keep the API on loopback. Bootstrap a dedicated actor, store its one-time token
in an operator-controlled secret manager, and add only the grants required for
the test or agent. A query must resolve exactly one device in the actor's
installation:

```sh
cargo run --locked -- actor-grant-device-execute ACTOR_ID \
  --device-query 'Kitchen light' --maximum-risk comfort
```

Mechanical tests require `--maximum-risk mechanical`. The CLI deliberately
cannot create a device-wide security grant; future lock operations require an
exact-capability workflow and a separate review.

## Normal recovery

On startup, HomeMagic loads every non-terminal command. `received` and
`validated` work is re-evaluated; `dispatched` and `acknowledged` work performs
observation-only confirmation. It never treats a process restart as permission
to repeat a physical command.

Use `commands.get` for the current aggregate and `commands.audit` for the ordered
transition evidence. A timeout or mismatched observation is a durable outcome.
Do not retry with a new key until an operator has inspected current physical and
observed state. Retrying the identical request with the same key is safe and
returns the original command.

## Compensation and rollback

Physical actions cannot be transactionally rolled back. Capture original state
before a test and restore it with a new governed command and new idempotency key.
If restoration fails, leave an explicit failed cleanup record and restore the
device manually; never report the scenario as passed.

For switches, restore original on/off state. For dimmers, restore both level and
on/off state. For calibrated covers, issue `stop` first, keep the physical stop
control within reach, and restore the original position. Do not run cover motion
tests when the area is not visible and clear.

## Emergency handling

For unexpected cover movement:

1. Use the physical wall switch or device stop control immediately.
2. If safe and reachable, issue a governed `position.v1` `stop` command.
3. Isolate actuator power if movement continues and the installation permits it.
4. Disable the actor token to prevent further remote commands.
5. Preserve command/audit and device event cursors before diagnosis.

Network RPC is never the sole emergency-stop mechanism. Fire, shock, crushing,
security, and building-safety procedures take priority over preserving software
state or test evidence.

## Incident evidence

Retain the redacted hardware report, command IDs, actor ID, policy reasons,
transition sequences, timestamps, correlation ID, device model/firmware, and
whether cleanup completed. Do not retain bearer tokens, Digest material,
addresses, native IDs, aliases, or raw vendor payloads.
