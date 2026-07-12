#!/usr/bin/env bash
set -euo pipefail

report_path="${1:-matter-independent-reference-report.json}"
expected_architecture="${2:-$(uname -m)}"
manifest="config/matter-controller-candidates.json"

test "$(uname -m)" = "$expected_architecture"

reference_repository="$(jq -r '.candidates[] | select(.id == "rs-matter") | .repository' "$manifest")"
reference_revision="$(jq -r '.candidates[] | select(.id == "rs-matter") | .revision' "$manifest")"
reference_lockfile_sha256="$(jq -r '.candidates[] | select(.id == "rs-matter") | .generated_lockfile_sha256' "$manifest")"
test -n "$reference_repository"
test -n "$reference_revision"

workspace="$(mktemp -d)"
reference_pid=""
cleanup() {
  status="$?"
  trap - EXIT
  if test -n "$reference_pid"; then
    kill "$reference_pid" 2>/dev/null || true
    wait "$reference_pid" 2>/dev/null || true
  fi
  if test "$status" -ne 0; then
    test ! -f "$workspace/reference.log" || cat "$workspace/reference.log" >&2
    test ! -f "$workspace/lifecycle.json" || cat "$workspace/lifecycle.json" >&2
  fi
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT

git init --quiet "$workspace/reference-source"
git -C "$workspace/reference-source" remote add origin "$reference_repository"
git -C "$workspace/reference-source" fetch --quiet --depth 1 origin "$reference_revision"
git -C "$workspace/reference-source" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$workspace/reference-source" rev-parse HEAD)" = "$reference_revision"
git -C "$workspace/reference-source" apply \
  "$PWD/spikes/matter-controller-rust-matc/rs-matter-reference-no-mdns.patch"

export CARGO_TARGET_DIR="$workspace/reference-target"
cargo generate-lockfile --manifest-path "$workspace/reference-source/Cargo.toml"
test "$(shasum -a 256 "$workspace/reference-source/Cargo.lock" | awk '{print $1}')" = \
  "$reference_lockfile_sha256"
cargo build --quiet --locked --release \
  --manifest-path "$workspace/reference-source/Cargo.toml" \
  --package rs-matter-examples --bin onoff_light

mkdir -p "$workspace/reference-home" "$workspace/reference-state" "$workspace/reference-tmp"
(
  cd "$workspace/reference-state"
  HOME="$workspace/reference-home" \
    HOMEMAGIC_MATTER_TEST_PORT=55540 \
    RUST_LOG=info \
    TMPDIR="$workspace/reference-tmp" \
    "$workspace/reference-target/release/onoff_light" \
    >"$workspace/reference.log" 2>&1
) &
reference_pid="$!"

ready=false
for _ in $(seq 1 30); do
  if ! kill -0 "$reference_pid" 2>/dev/null; then
    cat "$workspace/reference.log" >&2
    exit 1
  fi
  if rg --quiet 'SetupQRCode:.*MT:' "$workspace/reference.log"; then
    ready=true
    break
  fi
  sleep 1
done
test "$ready" = true

export CARGO_TARGET_DIR="$workspace/spike-target"
cargo run --quiet --locked --release \
  --manifest-path spikes/matter-controller-rust-matc/Cargo.toml -- \
  "$workspace/controller-state" '[::1]:55540' > "$workspace/lifecycle.json"

last_reference_observation="commissionable_fixture_started"
if rg --quiet 'Got Arm Fail Safe Request' "$workspace/reference.log"; then
  last_reference_observation="arm_fail_safe_received"
fi

jq \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg rustc "$(rustc --version)" \
  --arg last_reference_observation "$last_reference_observation" \
  '. + {
    last_reference_observation: $last_reference_observation,
    host: {captured_at: $captured_at, os: $os, architecture: $architecture, rustc: $rustc}
  }' \
  "$workspace/lifecycle.json" > "$report_path"

jq -e '.schema == "homemagic.matter.independent-reference.v1" and .outcomes.fabric_create == "pass"' \
  "$report_path" >/dev/null
echo "Matter independent reference report written to $report_path"
