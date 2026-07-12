# Native Rust Controller Build and Footprint Audit

- Candidate: [`tom-code/rust-matc`](https://github.com/tom-code/rust-matc)
- Revision: `c829d2a1b570b2f2433607a3f4731074b73fb367`
- Package: `matc 0.1.3`
- License: BSD-2-Clause
- Public workflows: initial `rust-matc` run `29210089483`; corrected two-candidate run `29211681113`
- Evidence class: candidate self-tests and build/packaging evidence

## Reproduction

Both hosts checked out the same HomeMagic commit and ran:

```sh
./scripts/run-matter-controller-candidate.sh rust-matc report.json ARCHITECTURE
```

The script fetches only the manifest revision into a temporary repository, uses
stable Rust 1.93.0, runs default and all-feature tests, builds the
`simple-devman` release example, measures source/runtime and dependency
footprints, writes a versioned JSON report, and deletes the checkout. Candidate
code never enters HomeMagic production manifests.

## Cross-platform result

| Measurement | macOS ARM64 | Linux x86_64 |
| --- | ---: | ---: |
| Default unit tests | 69 pass | 69 pass |
| Default doc tests | 13 pass, 1 ignored | 13 pass, 1 ignored |
| All-feature unit tests | 73 pass | 73 pass |
| All-feature doc tests | 13 pass, 1 ignored | 13 pass, 1 ignored |
| Release example | Pass, Mach-O ARM64 | Pass, ELF x86_64 |
| Release example bytes | 3,814,864 | 4,610,504 |
| Default normal dependencies | 114 | 113 |
| All-feature normal dependencies | 150 | 153 |

Exact reports:

- `rust-matc-macos-arm64.json`
- `rust-matc-linux-x86_64.json`

## Rust and native footprint

The candidate repository contains 3,471,251 Rust bytes and 180,093 other code
bytes, or 95.06% Rust by this deliberately broad repository metric.
The non-Rust portion is Python cluster code-generation input/tooling; it is not
compiled into the controller. The compiled first-party runtime path is 100%
Rust, contains zero detected semantic `unsafe` blocks, and contains zero
first-party C, C++, Objective-C, Swift, or header files.

Default on-network builds have no identified native platform adapter. The
optional `ble` feature crosses a replaceable `btleplug` boundary:

- macOS: `objc-sys`, `objc2`, `objc2-core-bluetooth`, and `objc2-foundation`;
- Linux: `dbus` and `libdbus-sys`, plus the system `libdbus-1` package.

HomeMagic's initial ADR-0038 transport is on-network, so BLE is not required by
the first adapter. Enabling BLE later requires a separate ADR-0005 review and
does not inherit this default-path result.

## rs-matter cross-platform result

The same workflow audited `project-chip/rs-matter` revision
`42d3c2211239f5f388ac7f7449c82bb3347912f5` after source inspection found its
new commissioner/controller path.

| Measurement | macOS ARM64 | Linux x86_64 |
| --- | ---: | ---: |
| Default workspace tests | Pass | Pass |
| All-feature check | Fail: `defmt` and `log` conflict | Fail: `defmt` and `log` conflict |
| Release controller/device binaries | Pass | Pass |
| Commissioner binary bytes | 1,825,024 | 2,187,000 |
| Default normal dependencies | 241 | 242 |
| Generated lock SHA-256 | `2ef10d247e0c00d5d90e0f250bf804284d76b0ec4eae166e143deb98012e7e7a` | Same |

Exact reports:

- `rs-matter-macos-arm64.json`
- `rs-matter-linux-x86_64.json`

The repository is 98.16% Rust by measured code bytes and has no first-party
native source files. It contains 211 source lines matching semantic `unsafe`
constructs across all optional backends and configurations. The audit does not
claim those are all compiled by the default controller path; the exact compiled
default unsafe count remains unproven and is therefore `null` in the reports.
This is materially different from reporting zero.

`rs-matter` does not commit a root lockfile. HomeMagic records and enforces the
generated resolution hash so dependency drift fails the audit rather than
silently changing evidence. A production integration would need to own its
lockfile. All-feature failure is an invalid aggregate feature combination, not
a default controller build failure, but it remains explicit.

## Maintenance and packaging observations

- Compatible BSD-2-Clause license and a committed lockfile are present.
- The snapshot has 84 commits from six recorded author identities; one identity
  authored most history.
- No GitHub release/tag, declared `rust-version`, repository security policy, or
  published conformance/certification claim was found at the snapshot.
- Stable Rust 1.93.0 builds without a candidate-specific toolchain file.
- The default package is Cargo-native; all-feature Linux adds DBus development
  headers and macOS uses system CoreBluetooth through transitive Rust bindings.

## Gate interpretation

Cross-platform default build and packaging feasibility passes for both pins.
`rust-matc` proves zero first-party semantic unsafe blocks; `rs-matter` requires
a compiled-path unsafe audit before any production selection. Neither passes
the complete ADR-0038 candidate gate: E4-008-03 records independent lifecycle,
attestation, secret-store, cancellation, and partial-outcome failures. Candidate
self-tests cannot substitute for that evidence.
