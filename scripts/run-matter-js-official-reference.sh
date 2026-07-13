#!/usr/bin/env bash
set -euo pipefail

report_path="${1:-matter-js-official-reference-report.json}"
expected_architecture="${2:-$(uname -m)}"
operational_address_fallback="${3:-false}"
execution_mode="${4:-reference}"
candidate_manifest="config/matter-js-candidate.json"
controller_manifest="config/matter-controller-candidates.json"

case "$report_path" in
  /*) ;;
  *) report_path="$PWD/$report_path" ;;
esac

test "$(uname -m)" = "$expected_architecture"
test "$(node --version)" = "v$(jq -r '.node' "$candidate_manifest")"

case "$(uname -s)" in
  Darwin)
    platform="darwin"
    target_architecture="arm64"
    ;;
  Linux)
    platform="linux"
    target_architecture="x64"
    ;;
  *)
    echo "unsupported host" >&2
    exit 2
    ;;
esac

candidate_repository="$(jq -r '.repository' "$candidate_manifest")"
candidate_revision="$(jq -r '.revision' "$candidate_manifest")"
candidate_lock_sha256="$(jq -r '.package_lock_sha256' "$candidate_manifest")"
reference_repository="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .repository' "$controller_manifest")"
reference_revision="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .revision' "$controller_manifest")"
reference_release="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .release' "$controller_manifest")"
target="${platform}-${target_architecture}-light-no-ble-no-wifi-no-thread"

workspace="$(mktemp -d)"
homemagic_root="$PWD"
reference_pid=""
cleanup() {
  status="$?"
  trap - EXIT
  if test -n "$reference_pid"; then
    kill "$reference_pid" 2>/dev/null || true
    wait "$reference_pid" 2>/dev/null || true
  fi
  if test "$status" -ne 0; then
    test ! -f "$workspace/reference.log" || tail -200 "$workspace/reference.log" >&2
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
if test "$operational_address_fallback" = "1"; then
  git -C "$workspace/candidate" apply \
    "$homemagic_root/spikes/matter-controller-matter-js/direct-operational-address.patch"
fi
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
test "$(git -C "$workspace/reference-source" rev-parse HEAD)" = "$reference_revision"
python3 "$workspace/reference-source/scripts/checkout_submodules.py" \
  --shallow --platform "$platform" --jobs 8

export GITHUB_ACTION=1
export PW_ENVIRONMENT_ROOT="$workspace/environment"
(
  cd "$workspace/reference-source"
  set +u
  source scripts/bootstrap.sh -p "$platform"
  set -u
  python3 scripts/build/build_examples.py --target "$target" build
)

reference_binary="$workspace/reference-source/out/$target/chip-lighting-app"
test -x "$reference_binary"
mkdir -p "$workspace/reference-state" "$workspace/controller-state"
"$reference_binary" \
  --discriminator 3840 \
  --passcode 20202021 \
  --secured-device-port 55541 \
  --KVS "$workspace/reference-state/kvs" \
  >"$workspace/reference.log" 2>&1 &
reference_pid="$!"

ready=false
for _ in $(seq 1 45); do
  if ! kill -0 "$reference_pid" 2>/dev/null; then
    cat "$workspace/reference.log" >&2
    exit 1
  fi
  if grep -Eq 'SetupQRCode|QRCode|Commissioning window is now open' "$workspace/reference.log"; then
    ready=true
    break
  fi
  sleep 1
done
test "$ready" = true

if test "$execution_mode" = "sidecar"; then
  package_path="$workspace/sidecar-package"
  HOMEMAGIC_MATTER_OPERATIONAL_ADDRESS_FALLBACK="$operational_address_fallback" \
    "$homemagic_root/scripts/build-matter-js-sidecar.sh" \
    "$package_path" "$expected_architecture"
  HOMEMAGIC_MATTER_JS_NODE="$package_path/bin/node" \
    HOMEMAGIC_MATTER_JS_SIDECAR="$package_path/sidecar.mjs" \
    HOMEMAGIC_MATTER_FIXTURE_SETUP="34970112332" \
    HOMEMAGIC_MATTER_FIXTURE_ADDRESS="::1" \
    HOMEMAGIC_MATTER_FIXTURE_PORT="55541" \
    cargo test -p homemagic-matter --all-features \
      packaged_matter_js_should_match_the_rust_protocol_when_configured \
      -- --exact --nocapture
  jq -n \
    --slurpfile package "$package_path/manifest.json" \
    --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg os "$(uname -s)" \
    --arg architecture "$(uname -m)" \
    --arg reference_revision "$reference_revision" \
    --arg reference_release "$reference_release" \
    --arg reference_target "$target" \
    --arg operational_address_fallback "$operational_address_fallback" \
    '{
      schema: "homemagic.matter.matter-js-sidecar-official-reference.v1",
      host: {captured_at: $captured_at, os: $os, architecture: $architecture},
      reference: {id: "connectedhomeip-light", revision: $reference_revision, release: $reference_release, target: $reference_target},
      diagnostic: {operational_address_fallback: ($operational_address_fallback == "1")},
      package: $package[0],
      outcomes: {
        missing_fabric: "pass",
        fabric_create: "pass",
        setup_validation: "pass",
        commission: "pass",
        inventory: "pass",
        process_restart: "pass",
        fabric_load: "pass",
        remove: "pass",
        empty_inventory: "pass"
      }
    }' > "$report_path"
  echo "matter.js sidecar official reference report written to $report_path"
  exit 0
fi

(
  cd "$workspace/controller-state"
  HOMEMAGIC_MATTER_OPERATIONAL_ADDRESS_FALLBACK="$operational_address_fallback" \
    node "$workspace/candidate/homemagic-lifecycle.mjs" \
    commission "$workspace/commission.json" "::1" 55541
)

if jq -e '.outcomes.commission == "pass" and .outcomes.inventory == "pass" and .outcomes.read == "pass" and .outcomes.invoke == "pass" and .outcomes.subscribe == "pass"' "$workspace/commission.json" >/dev/null; then
  (
    cd "$workspace/controller-state"
    node "$workspace/candidate/homemagic-lifecycle.mjs" \
      restart "$workspace/restart.json" "::1" 55541
  )
else
  jq -n '{schema: "homemagic.matter.matter-js-independent-reference.v1", mode: "restart", outcomes: {restart: "not_run", remove: "not_run"}, error: {phase: "prerequisite", message: "Commission lifecycle did not pass"}}' > "$workspace/restart.json"
fi

jq -n \
  --slurpfile commission "$workspace/commission.json" \
  --slurpfile restart "$workspace/restart.json" \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg node "$(node --version)" \
  --arg candidate_revision "$candidate_revision" \
  --arg reference_revision "$reference_revision" \
  --arg reference_release "$reference_release" \
  --arg reference_target "$target" \
  --arg operational_address_fallback "$operational_address_fallback" \
  '{
    schema: "homemagic.matter.matter-js-official-reference-report.v1",
    candidate: {id: "matter-js", revision: $candidate_revision},
    reference: {id: "connectedhomeip-light", revision: $reference_revision, release: $reference_release, target: $reference_target},
    diagnostic: {operational_address_fallback: ($operational_address_fallback == "1")},
    host: {captured_at: $captured_at, os: $os, architecture: $architecture, node: $node},
    commission_process: $commission[0],
    restart_process: $restart[0]
  }' > "$report_path"

jq -e '.schema == "homemagic.matter.matter-js-official-reference-report.v1"' "$report_path" >/dev/null
echo "matter.js official reference report written to $report_path"
