# ADR-0006: Use Shelly Gen2+ as the first device vertical slice

- Status: Accepted
- Date: 2026-07-11

## Context

The first implementation needs to prove local discovery, device identity,
capability projection, current state, and an RPC-first interface on real hardware.
Matter adds commissioning, fabric, Thread, certification, and SDK-selection risks
before the HomeMagic kernel itself has been proven.

Shelly Gen2+ devices advertise `_shelly._tcp` through mDNS and expose documented
local JSON-RPC methods. Their switch, light, cover, and energy components exercise
the most important initial capabilities.

## Decision

The first vertical slice supports unauthenticated Shelly Gen2+ devices:

1. discover `_shelly._tcp.local.` services, falling back to Shelly-filtered
   `_http._tcp.local.` advertisements;
2. resolve IP address and port;
3. fetch `Shelly.GetDeviceInfo` and `Shelly.GetStatus`;
4. project recognized components into HomeMagic capabilities;
5. expose the resulting device snapshot through the HomeMagic RPC API.

Authentication, persistent WebSocket notifications, Gen1 CoIoT, command dispatch,
and dynamic component pagination follow in later slices.

On macOS, the prototype may fall back to the operating system's `dns-sd` client
when the pure-Rust multicast socket cannot observe services already known to
mDNSResponder. This is a platform adapter, not part of the domain contract.

## Consequences

- The first executable can be tested against common local hardware.
- The initial scope remains safe and read-only.
- Matter stays an early roadmap item without blocking kernel validation.
- Authenticated devices are visible by identity but cannot initially provide full
  status.
