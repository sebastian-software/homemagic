# Delivery Epics

These epics turn the HomeMagic roadmap into checkable delivery contracts. They
cover the next five milestones after the M0 discovery prototype.

## Sequence

| Epic | Milestone | Status | Depends on | Outcome |
| --- | --- | --- | --- | --- |
| [EPIC-001](001-reliable-device-foundation.md) | M1 | Done | M0 | Durable, continuously updated Shelly registry |
| [EPIC-002](002-safe-command-control-plane.md) | M2 | In progress | EPIC-001 | Authorized and audited device control |
| [EPIC-003](003-agent-authored-automation-engine.md) | M3 | Done | EPIC-002 | Governed automation authoring and execution |
| [EPIC-004](004-matter-controller-integration.md) | M4 | Planned; feasibility may start early | EPIC-001 and EPIC-002 | Production-capable Matter controller integration |
| [EPIC-005](005-mcp-intent-driven-interaction.md) | M5 | Planned | EPIC-003; EPIC-004 for Matter coverage | Agent-first control and automation lifecycle |

The critical delivery path is:

```text
M0 prototype
  -> EPIC-001 reliable device foundation
  -> EPIC-002 safe command control plane
  -> EPIC-003 automation engine
  -> EPIC-005 MCP and intent-driven interaction

EPIC-004 Matter feasibility can run during EPIC-001.
Full Matter delivery depends on EPIC-002 command and policy contracts.
```

## Status rules

- `Planned`: scoped but a dependency or start decision is outstanding.
- `Ready`: dependencies are satisfied and implementation may begin.
- `In progress`: at least one workstream is actively being delivered.
- `Blocked`: progress requires a named decision or external change.
- `Done`: every acceptance criterion and exit-gate item has evidence.

## Checklist rules

1. Check an item only when its implementation and required verification are both
   complete.
2. Append an evidence link or command result to completed acceptance items.
3. Keep partial work unchecked; explain partial state in the epic's progress log.
4. Add or supersede an ADR before implementing a decision marked `ADR required`.
5. If scope moves between epics, update both documents in the same commit.
6. An epic is not done merely because all implementation tasks are checked. Its
   acceptance criteria and exit gate must also be checked.

## Cross-epic invariants

Every epic must preserve these project constraints:

- first-party runtime code remains at least 95% Rust;
- unsafe Rust and FFI follow ADR-0005;
- stable identity never depends on a mutable display name;
- integrations map vendor behavior into versioned capabilities;
- all mutations pass through application commands and policy;
- MCP, future UIs, and other transports receive no privileged bypass;
- macOS Apple Silicon and Linux x86_64 remain supported targets;
- documentation, migrations, protocol fixtures, and operational diagnostics ship
  with the behavior they describe.

## Progress log format

Add dated entries to the relevant epic rather than maintaining a separate status
document:

```markdown
## Progress log

- 2026-07-11: Epic created; no implementation started.
- YYYY-MM-DD: Completed E1.2. Evidence: `path/to/test` and `cargo test ...`.
```
