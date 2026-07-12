# EPIC-005: MCP and Intent-Driven Interaction

- Milestone: M5
- Status: Planned
- Depends on: EPIC-003; EPIC-004 for Matter coverage
- Followed by: generated end-user interaction surfaces

## Objective

Expose HomeMagic's governed device and automation capabilities to agents through
MCP, with safe identity resolution and token-efficient context, while preserving
the same application, policy, command, and audit boundaries as every other client.

## User outcome

An agent can understand the installation, resolve phrases such as “the dining
room blinds,” inspect relevant capabilities, propose an automation, validate and
simulate it, explain the result, and request activation. The agent never needs to
construct entity IDs or receive an unrestricted administrative API.

## Scope

- MCP server using the official Rust SDK;
- Streamable HTTP and local stdio deployment modes as supported by the SDK;
- authentication and actor mapping for remote/local sessions;
- curated device, capability, event, command, policy, and automation resources;
- narrow tools for queries, commands, and automation lifecycle;
- semantic identity resolution using names, aliases, spaces, and capabilities;
- ambiguity handling and confirmation policy;
- token budgets, pagination, filtering, and schema discovery;
- audit, rate limits, diagnostics, and conformance tests;
- presentation descriptors that can later drive generated UIs.

## Non-goals

- giving an LLM direct database, filesystem, adapter, or raw RPC access;
- arbitrary tool generation at runtime;
- silently resolving ambiguous or security-sensitive targets;
- embedding a specific model provider in the core runtime;
- building the final graphical UI;
- allowing MCP to activate behavior that policy would reject over RPC.

## Finalized EPIC-003 contracts

- MCP delegates automation mutations to `AutomationLifecycleService` with the
  session's authenticated `Actor`; tools never accept an alternate actor ID.
- New drafts use the server-generated envelope contract from
  `automations.drafts.create`. Agents author behavior and provenance text while
  HomeMagic owns schema, automation ID, version, author, and timestamp.
- Authored `automation.document.v1` and normalized `automation.plan.v1` are
  data-only, bounded, and versioned independently. MCP must not add code,
  template, raw-adapter, or arbitrary JSON execution paths.
- Validation, simulation, approval, activation, rollback, disable, retirement,
  operational get/list, run/trace/cancel, and exact catch-up semantics are the
  JSON-RPC reference contract. MCP tools may narrow but must not weaken them.
- Simulation accepts synthetic data but no plan, run identity, dispatcher, or
  physical command path. Sensitive exact versions require user approval.
- Operational mutations use the revision returned by the latest get/list
  result. Rollback changes only the active pointer; cancellation and physical
  compensation remain separate.
- Missed schedules are never replayed automatically. Catch-up represents one
  explicit, authenticated, idempotent missed instant.
- Automation transition resources use the existing durable global event cursor,
  actor-owner filtering, and bounded pages. Cursor gaps may represent hidden
  events and must not be treated as data loss.
- Every runtime command still crosses EPIC-002 `CommandService` with current
  grants, policy, idempotency, deadlines, audit, and causation.

## Required decisions

- [ ] E5.D1: Add an ADR for MCP transport, authentication, session, and actor
  mapping.
- [ ] E5.D2: Add an ADR for tool/resource design, disclosure boundaries, and token
  budgets.
- [ ] E5.D3: Define identity-resolution scoring, ambiguity thresholds, and
  confirmation requirements.
- [ ] E5.D4: Define the first versioned presentation descriptor contract without
  committing to a frontend framework.

## Workstream E5.1: MCP runtime boundary

- [ ] Add an MCP adapter crate that depends only on application services and
  public contract types.
- [ ] Integrate the official Rust MCP SDK without changing core domain semantics.
- [ ] Support stdio for local development and Streamable HTTP for managed use.
- [ ] Negotiate and report only implemented capabilities.
- [ ] Map each session to an authenticated HomeMagic actor.
- [ ] Apply cancellation, deadlines, progress, and structured errors.
- [ ] Ensure MCP startup/shutdown does not affect device sessions or automations.

## Workstream E5.2: Resource design

- [ ] Expose a compact installation summary resource.
- [ ] Expose filtered spaces, devices, endpoints, and capability descriptors.
- [ ] Expose current observations with freshness and availability.
- [ ] Expose relevant policy summaries without secret or internal rule leakage.
- [ ] Expose automation drafts, versions, validation, simulation, and run traces
  according to actor authorization.
- [ ] Use pagination and filters for every potentially unbounded collection.
- [ ] Provide list-changed or resource-update notifications where useful.
- [ ] Separate stable machine data from human-oriented summaries.

## Workstream E5.3: Tool design

- [ ] Add a target-resolution tool that returns candidates and confidence, not an
  implicit mutation.
- [ ] Add read-only device/capability inspection tools.
- [ ] Add a command validation tool.
- [ ] Add a command execution tool that requires an unambiguous resolved target
  and passes through EPIC-002.
- [ ] Add draft, validate, simulate, approve-request, activate, rollback, and
  disable automation tools over EPIC-003.
- [ ] Return structured content plus concise human explanation.
- [ ] Keep security-sensitive operations out of the first tool set unless a
  dedicated approval flow is implemented and tested.

## Workstream E5.4: Identity and intent resolution

- [ ] Index device names, aliases, spaces, endpoint semantics, and capabilities.
- [ ] Preserve stable IDs behind resolution results without requiring users to
  speak them.
- [ ] Rank exact aliases, scoped names, space context, and capability fit.
- [ ] Detect ambiguity and return candidate distinctions.
- [ ] Require explicit confirmation below the accepted confidence threshold.
- [ ] Prevent stale or unavailable targets from being silently preferred.
- [ ] Audit the source phrase, candidates, selection, actor, and resulting command
  or automation.
- [ ] Support multilingual aliases without using localized text as identity.

## Workstream E5.5: Safety, efficiency, and operations

- [ ] Apply the same authorization and risk policy as RPC and automation runtime.
- [ ] Add per-session tool rate, concurrency, and payload limits.
- [ ] Add configurable resource and trace token budgets.
- [ ] Redact secrets, credentials, network internals, camera data, and lock data by
  default.
- [ ] Resist prompt/tool descriptions that encourage bypassing approval.
- [ ] Add structured MCP diagnostics without logging private prompt content by
  default.
- [ ] Add conformance tests for supported MCP protocol versions and transports.
- [ ] Document secure local and remote deployment.

## Workstream E5.6: Generated-surface foundation

- [ ] Define presentation descriptors for capability controls, state summaries,
  risk prompts, and automation traces.
- [ ] Generate descriptors from capability schemas and policy, not vendor type.
- [ ] Include accessibility label, unit, range, ordering, and confirmation hints.
- [ ] Keep descriptors versioned and transport-neutral.
- [ ] Produce one reference CLI/text renderer proving that descriptors are usable.
- [ ] Document the boundary for a later generated web/mobile UI epic.

## Test and verification checklist

- [ ] MCP SDK conformance tests pass for every advertised protocol capability.
- [ ] Tool contract tests prove application-service delegation and error mapping.
- [ ] Authorization tests compare equivalent RPC, automation, and MCP calls.
- [ ] Resolution fixtures cover exact, contextual, ambiguous, stale, unavailable,
  and multilingual names.
- [ ] Prompt-injection/adversarial tests prove tools cannot bypass target,
  authorization, policy, approval, or schema validation.
- [ ] Token-budget tests cover large installations, traces, and event histories.
- [ ] End-to-end agent fixture covers discover, resolve, draft, validate, simulate,
  request approval, activate, observe, and rollback.
- [ ] Matter and Shelly capabilities use identical MCP tool shapes.

## Acceptance criteria

- [ ] AC1: An MCP client can discover the supported resources and tools through
  protocol negotiation without private HomeMagic implementation knowledge.
- [ ] AC2: A natural household phrase resolves to either one authorized target or
  an explicit ambiguity response; it never silently selects a low-confidence
  target.
- [ ] AC3: An agent can complete the automation draft-to-activation lifecycle with
  validation, simulation, explanation, and required approval.
- [ ] AC4: Equivalent MCP and RPC mutations produce the same command, policy, and
  audit records.
- [ ] AC5: Security-sensitive data and tools are absent unless the actor and policy
  explicitly allow them.
- [ ] AC6: Large installations remain usable through filtering, pagination, and
  bounded responses.
- [ ] AC7: Shelly and Matter devices are addressed through common capabilities,
  not vendor-specific MCP tools.
- [ ] AC8: Presentation descriptors can render a useful reference control surface
  without reading adapter implementation code.

## Exit gate

- [ ] All acceptance criteria contain linked evidence.
- [ ] Required ADRs are accepted and indexed.
- [ ] MCP conformance, adversarial, authorization, and token-budget suites pass.
- [ ] Remote deployment documentation includes TLS, authentication, rate limits,
  and audit behavior.
- [ ] No raw RPC, database, filesystem, integration, or arbitrary-code tool is
  exposed.
- [ ] A follow-up generated-UI epic can depend on the presentation descriptor
  contract without changing the capability kernel.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Ambiguous language controls the wrong device | Candidate resolution, thresholds, and confirmation |
| MCP becomes a privileged side API | Delegate only to application services and compare policy tests |
| Context grows beyond model limits | Filtered resources, pagination, summaries, and token budgets |
| Tool descriptions invite unsafe behavior | Narrow tools, policy enforcement, and adversarial tests |

## Progress log

- 2026-07-11: Epic created; blocked on the automation lifecycle in EPIC-003.
- 2026-07-12: Consumed the finalized EPIC-003 lifecycle, schema, simulation,
  approval, operational revision, run-control, catch-up, and event contracts.
