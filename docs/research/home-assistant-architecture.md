# Home Assistant Architecture Analysis

- Research date: 2026-07-11
- Source snapshot: `home-assistant/core` commit
  `9cd694008baf1222fc3815f9c0d9303dc583c46f`
- Purpose: extract compatibility lessons for HomeMagic, not reproduce Home
  Assistant internals or configuration.

## Executive summary

Home Assistant's strongest asset is not its UI or Python runtime. It is the large,
maintained body of device semantics, lifecycle behavior, discovery rules,
diagnostics, and compatibility tests. Its architecture has successfully absorbed
thousands of integrations, but that success also creates user-visible concepts
and internal registries whose complexity HomeMagic should not copy.

HomeMagic should reuse Home Assistant as:

- a compatibility catalogue and device-behavior reference;
- a source of mature entity semantics and edge cases;
- a quality checklist for adapters;
- a comparison oracle for selected hardware.

It should not reuse Home Assistant as:

- a configuration or custom-component compatibility target;
- an entity-ID-based user experience;
- an event bus where every subsystem shares unbounded dictionaries;
- a reason to make UI configuration the primary lifecycle.

## Repository structure

At the inspected commit, `homeassistant/components` contains 1,474 integration
manifests. The root package combines the runtime, helpers, and every built-in
integration. Tests mirror integration paths under `tests/components`.

Important core files are large because they encode mature lifecycle and backward
compatibility behavior:

| Concern | File | Lines at snapshot |
| --- | --- | ---: |
| Runtime, event bus, states, services | `homeassistant/core.py` | 3,011 |
| Configuration entries and flows | `homeassistant/config_entries.py` | 4,212 |
| Integration loading | `homeassistant/loader.py` | 1,800 |
| Entity base behavior | `homeassistant/helpers/entity.py` | 1,786 |
| Entity platform orchestration | `homeassistant/helpers/entity_platform.py` | 1,308 |
| Entity registry | `homeassistant/helpers/entity_registry.py` | 2,553 |
| Device registry | `homeassistant/helpers/device_registry.py` | 1,974 |

This is not evidence of poor implementation. It is evidence that compatibility,
migration, naming, lifecycle, and extension behavior become a substantial product
surface.

## Core runtime

The documented core has four primary pieces: Event Bus, State Machine, Service
Registry, and Timer. The source additionally composes configuration, integration
loading, job scheduling, authentication context, HTTP/WebSocket APIs, registries,
and persistence helpers around them.

### Event bus

Events are the general coordination mechanism. They carry context with IDs and
parent/user relationships, enabling causation across service calls, state
changes, and automations. This is valuable for audit and traces.

The trade-off is weak semantic isolation: generic event names and payloads make
cross-subsystem coupling easy. HomeMagic should retain causation metadata but use
typed domain events internally.

### State machine

Home Assistant stores the latest state by `domain.object_id` and publishes
`state_changed`. A state contains a primary string value, attributes, timestamps,
and context. The model is extremely flexible and makes integrations uniform.

That flexibility shifts semantics into domain conventions and attributes. The
REST API can set a state without communicating with the physical device, which
illustrates the separation between representation and actuation.

HomeMagic should explicitly distinguish reported observations, desired state,
commands, and command outcomes.

### Service registry

Integrations register named actions. Calls are asynchronous, schema-validated,
and carry context. Service actions are the real actuation path even when state is
presented as the primary UI concept.

HomeMagic can make this relationship clearer by making typed commands first-class
RPC/application objects instead of presenting state mutation as control.

## Integration and plugin model

An integration is a directory identified by an immutable domain. Its manifest
declares dependencies, optional after-dependencies, Python requirements,
discovery matchers, integration type, IoT class, owners, and config-flow support.
Entity platforms are conventionally separate files such as `light.py`,
`switch.py`, or `cover.py`.

Built-in integrations live with Core. Custom integrations can override a built-in
domain and install Python dependencies at runtime. The approach offers a very low
extension threshold, but dependency and compatibility risk remains in the main
process.

HomeMagic should initially prefer compile-time Rust crates and a reviewed adapter
catalogue. A public plugin ABI should wait until several adapters demonstrate the
right boundary.

## Configuration lifecycle

Configuration entries persist integration configuration and move through states
including not-loaded, setup-in-progress, loaded, retry, setup error, migration
error, unloading, and failed unload. Config flows, reauthentication, reconfigure
flows, options, and subentries provide a complete UI-driven lifecycle.

The lifecycle semantics are worth keeping. The assumption that a UI flow owns
them is not. HomeMagic should expose enrollment, credential, retry, and repair
workflows through RPC and allow an agent, CLI, or generated UI to drive them.

## Devices, entities, and registries

Home Assistant distinguishes:

- devices: physical or logical products, connections, identifiers, manufacturer,
  model, firmware, area, and hierarchy;
- entities: state/control surfaces within domains such as light or sensor;
- config entries: instances of an integration and their lifecycle;
- areas, floors, labels, categories, and issues: additional organization and
  operational metadata.

Entity and device registries preserve identity and user customizations even while
runtime entity objects come and go. Unique IDs, availability, disabled-by-default
diagnostics, device classes, and supported features provide valuable semantics.

The user cost is indirection. A physical multi-channel device becomes several
entities with generated IDs and composed names. Scripts and automations often
refer to these technical identifiers. Renaming or choosing the correct entity can
be harder than expressing intent.

HomeMagic replaces the public entity abstraction with stable device/endpoint IDs,
mutable aliases, semantic space relationships, and composable capabilities.

## Data acquisition

Home Assistant supports push and poll. `DataUpdateCoordinator` centralizes shared
polls, listener notification, setup, retry, availability, and request
parallelism. Integrations are encouraged to keep entity property access free from
I/O and cache protocol data before projecting it.

This separation is directly reusable:

- adapters own protocol sessions and refresh strategy;
- normalized observations are published after I/O;
- consumers never trigger hidden network operations by reading a property;
- backoff, concurrency limits, and stale/availability state are standard adapter
  requirements.

## Automation and scripts

Automations combine trigger configuration, conditions, variables, and a reusable
script action engine. They support validation, concurrency modes, limits,
restoration, enable/disable state, and execution traces. Traces preserve paths and
variables for debugging. Blueprints add parameterized templates.

This is powerful but configuration-shaped. HomeMagic should retain the separation
of trigger, condition, and action plus execution modes and traces, while defining
a compact typed intermediate representation specifically suited to agent
generation, validation, simulation, versioning, and policy.

Home Assistant already exposes scripts as LLM tools. HomeMagic moves the agent
boundary earlier: agents can author governed automation documents, not only call
pre-existing scripts.

## Shelly integration study

The Shelly integration is Platinum quality and local-push. Its manifest declares
Bluetooth and Zeroconf discovery and delegates the device protocol to the
`aioshelly` library. The integration supports both Gen1 block/CoIoT devices and
Gen2+ RPC devices, which creates parallel coordinator and entity paths.

For Gen2+, Home Assistant:

- discovers `_shelly._tcp.local.` and `_http._tcp.local.` records;
- creates an RPC device using the resolved host;
- coordinates connection, authentication failures, firmware checks, sleeping
  devices, polling, push updates, and reconnect;
- maps component descriptions into platform-specific entities;
- exposes repair issues when expected push updates do not arrive;
- registers MAC-based device identity and channel/subdevice metadata.

The platform files contain extensive mapping knowledge. HomeMagic should initially
support Gen2+ only, preserve the component key (`switch:0`, `light:0`, `cover:0`)
as adapter identity, and project behavior into capabilities. Gen1 is a separate
future adapter concern rather than a compatibility branch in every type.

## Matter integration study

Home Assistant does not embed the Matter controller in Core. It connects through
a WebSocket client to the Open Home Foundation Matter Server, verifies server
version, manages connection/listen lifecycle and BLE proxying, then uses an
intermediate adapter to track nodes and discover platform entities from endpoint
attributes.

The Matter integration has platform mappings for lights, covers, locks, vacuums,
climate, valves, sensors, updates, and more. This mapping layer is useful reference
material for translating clusters/device types into HomeMagic capabilities.

The architecture also exposes a practical constraint: the production-grade
controller stack and its host networking requirements are a product of their own.
The Open Home Foundation is moving its server from Python bindings over the
official C++ SDK to a Matter.js implementation. HomeMagic should therefore decide
its controller boundary through a dedicated experiment and ADR rather than assume
that a complete Rust controller is immediately viable.

## Quality model worth adopting

Home Assistant's Integration Quality Scale encodes operational lessons that are
easy to miss in a prototype. HomeMagic adapters should eventually satisfy an
equivalent checklist:

- unique identity and duplicate prevention;
- connection test before enrollment and startup;
- clean unload and shutdown;
- availability and recovery without log spam;
- reauthentication and reconfiguration;
- bounded parallel requests and appropriate polling;
- discovery and dynamic device handling;
- diagnostics with secret redaction;
- known supported/unsupported device documentation;
- stale-device cleanup and actionable repair issues;
- high test coverage for lifecycle and protocol mapping.

## Decisions for HomeMagic

| Home Assistant lesson | HomeMagic decision |
| --- | --- |
| Uniform domain entities enable broad compatibility | Use smaller composable capabilities |
| Device/entity/config registries preserve identity and customization | Use one domain registry with explicit projections |
| Event context enables causation | Preserve typed correlation and causation metadata |
| Services perform real actuation | Make commands first-class RPC/application objects |
| Push/poll coordinators isolate I/O | Standardize adapter session and observation ports |
| Config flows cover complex lifecycle | Expose the lifecycle through RPC, not only UI |
| Automation traces are essential | Make simulation and trace part of automation governance |
| Matter is a distinct controller subsystem | Keep Matter behind an adapter boundary |
| Integration quality requires more than protocol calls | Define conformance gates per adapter |

## Primary sources

All sources were accessed on 2026-07-11.

- [Home Assistant Core architecture](https://developers.home-assistant.io/docs/architecture/core/)
- [Entities: integrating devices and services](https://developers.home-assistant.io/docs/architecture/devices-and-services/)
- [Integration file structure](https://developers.home-assistant.io/docs/creating_integration_file_structure/)
- [Integration manifest](https://developers.home-assistant.io/docs/creating_integration_manifest/)
- [Config entries](https://developers.home-assistant.io/docs/config_entries_index/)
- [Fetching data](https://developers.home-assistant.io/docs/integration_fetching_data/)
- [Entity model](https://developers.home-assistant.io/docs/core/entity/)
- [Integration Quality Scale rules](https://developers.home-assistant.io/docs/core/integration-quality-scale/rules/)
- [REST API](https://developers.home-assistant.io/docs/api/rest/)
- [WebSocket API](https://developers.home-assistant.io/docs/api/websocket/)
- [Home Assistant Shelly integration](https://www.home-assistant.io/integrations/shelly/)
- [Home Assistant Matter integration](https://www.home-assistant.io/integrations/matter/)
- [Home Assistant Core source](https://github.com/home-assistant/core/tree/9cd694008baf1222fc3815f9c0d9303dc583c46f)
- [Open Home Foundation Matter Server](https://github.com/matter-js/matterjs-server)
- [Matter SDK architecture](https://project-chip.github.io/connectedhomeip-doc/getting_started/SDKBasics.html)
- [Shelly Gen2+ mDNS](https://shelly-api-docs.shelly.cloud/gen2/General/mDNS/)
- [Shelly Gen2+ RPC protocol](https://shelly-api-docs.shelly.cloud/gen2/General/RPCProtocol/)
- [Shelly service](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Shelly/)
- [Model Context Protocol architecture](https://modelcontextprotocol.io/docs/learn/architecture)
- [Official MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
