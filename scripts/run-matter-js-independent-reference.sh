#!/usr/bin/env bash
set -euo pipefail

report_path="${1:-matter-js-independent-reference-report.json}"
expected_architecture="${2:-$(uname -m)}"
candidate_manifest="config/matter-js-candidate.json"
controller_manifest="config/matter-controller-candidates.json"

case "$report_path" in
  /*) ;;
  *) report_path="$PWD/$report_path" ;;
esac

test "$(uname -m)" = "$expected_architecture"
test "$(node --version)" = "v$(jq -r '.node' "$candidate_manifest")"

candidate_repository="$(jq -r '.repository' "$candidate_manifest")"
candidate_revision="$(jq -r '.revision' "$candidate_manifest")"
candidate_lock_sha256="$(jq -r '.package_lock_sha256' "$candidate_manifest")"
reference_repository="$(jq -r '.candidates[] | select(.id == "rs-matter") | .repository' "$controller_manifest")"
reference_revision="$(jq -r '.candidates[] | select(.id == "rs-matter") | .revision' "$controller_manifest")"
reference_lock_sha256="$(jq -r '.candidates[] | select(.id == "rs-matter") | .generated_lockfile_sha256' "$controller_manifest")"

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
  fi
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT

git init --quiet "$workspace/candidate"
git -C "$workspace/candidate" remote add origin "$candidate_repository"
git -C "$workspace/candidate" fetch --quiet --depth 1 origin "$candidate_revision"
git -C "$workspace/candidate" checkout --quiet --detach FETCH_HEAD
test "$(shasum -a 256 "$workspace/candidate/package-lock.json" | awk '{print $1}')" = "$candidate_lock_sha256"
(
  cd "$workspace/candidate"
  npm ci --ignore-scripts --no-audit --no-fund
  npm run build-clean
)
cp spikes/matter-controller-matter-js/lifecycle.mjs "$workspace/candidate/homemagic-lifecycle.mjs"

git init --quiet "$workspace/reference-source"
git -C "$workspace/reference-source" remote add origin "$reference_repository"
git -C "$workspace/reference-source" fetch --quiet --depth 1 origin "$reference_revision"
git -C "$workspace/reference-source" checkout --quiet --detach FETCH_HEAD
git -C "$workspace/reference-source" apply \
  "$PWD/spikes/matter-controller-rust-matc/rs-matter-reference-no-mdns.patch"

export CARGO_TARGET_DIR="$workspace/reference-target"
cargo generate-lockfile --manifest-path "$workspace/reference-source/Cargo.toml"
test "$(shasum -a 256 "$workspace/reference-source/Cargo.lock" | awk '{print $1}')" = "$reference_lock_sha256"
cargo build --quiet --locked --release \
  --manifest-path "$workspace/reference-source/Cargo.toml" \
  --package rs-matter-examples --bin onoff_light

mkdir -p "$workspace/reference-home" "$workspace/reference-state" "$workspace/reference-tmp" "$workspace/controller-state"
(
  cd "$workspace/reference-state"
  HOME="$workspace/reference-home" \
    HOMEMAGIC_MATTER_TEST_PORT=55540 \
    RUST_LOG=info \
    TMPDIR="$workspace/reference-tmp" \
    "$workspace/reference-target/release/onoff_light" >"$workspace/reference.log" 2>&1
) &
reference_pid="$!"

ready=false
for _ in $(seq 1 30); do
  if ! kill -0 "$reference_pid" 2>/dev/null; then
    cat "$workspace/reference.log" >&2
    exit 1
  fi
  if grep -q 'SetupQRCode:.*MT:' "$workspace/reference.log"; then
    ready=true
    break
  fi
  sleep 1
done
test "$ready" = true

(
  cd "$workspace/controller-state"
  node "$workspace/candidate/homemagic-lifecycle.mjs" commission "$workspace/commission.json"
)

if jq -e '.outcomes.commission == "pass" and .outcomes.inventory == "pass" and .outcomes.read == "pass" and .outcomes.invoke == "pass" and .outcomes.subscribe == "pass"' "$workspace/commission.json" >/dev/null; then
  (
    cd "$workspace/controller-state"
    node "$workspace/candidate/homemagic-lifecycle.mjs" restart "$workspace/restart.json"
  )
else
  jq -n '{schema: "homemagic.matter.matter-js-independent-reference.v1", mode: "restart", outcomes: {restart: "not_run", remove: "not_run"}, error: {phase: "prerequisite", message: "Commission lifecycle did not pass"}}' > "$workspace/restart.json"
fi

last_reference_observation="commissionable_fixture_started"
if grep -q 'Got Arm Fail Safe Request' "$workspace/reference.log"; then
  last_reference_observation="arm_fail_safe_received"
fi

jq -n \
  --slurpfile commission "$workspace/commission.json" \
  --slurpfile restart "$workspace/restart.json" \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg node "$(node --version)" \
  --arg rustc "$(rustc --version)" \
  --arg candidate_revision "$candidate_revision" \
  --arg reference_revision "$reference_revision" \
  --arg last_reference_observation "$last_reference_observation" \
  '{
    schema: "homemagic.matter.matter-js-independent-reference-report.v1",
    candidate: {id: "matter-js", revision: $candidate_revision},
    reference: {id: "rs-matter", revision: $reference_revision},
    last_reference_observation: $last_reference_observation,
    host: {captured_at: $captured_at, os: $os, architecture: $architecture, node: $node, rustc: $rustc},
    commission_process: $commission[0],
    restart_process: $restart[0]
  }' > "$report_path"

jq -e '.schema == "homemagic.matter.matter-js-independent-reference-report.v1"' "$report_path" >/dev/null
echo "matter.js independent reference report written to $report_path"
