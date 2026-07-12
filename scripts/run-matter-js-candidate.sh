#!/usr/bin/env bash
set -eo pipefail

report_path="${1:-matter-js-candidate-report.json}"
expected_architecture="${2:-$(uname -m)}"
manifest="config/matter-js-candidate.json"

case "$report_path" in
  /*) ;;
  *) report_path="$PWD/$report_path" ;;
esac

test "$(uname -m)" = "$expected_architecture"

repository="$(jq -r '.repository' "$manifest")"
revision="$(jq -r '.revision' "$manifest")"
lock_sha256="$(jq -r '.package_lock_sha256' "$manifest")"
expected_node="v$(jq -r '.node' "$manifest")"
license="$(jq -r '.license' "$manifest")"

test "$(node --version)" = "$expected_node"

workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT

git init --quiet "$workspace/source"
git -C "$workspace/source" remote add origin "$repository"
git -C "$workspace/source" fetch --quiet --depth 1 origin "$revision"
git -C "$workspace/source" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$workspace/source" rev-parse HEAD)" = "$revision"
test "$(shasum -a 256 "$workspace/source/package-lock.json" | awk '{print $1}')" = "$lock_sha256"

source_checkout_kib="$(du -sk "$workspace/source" | awk '{print $1}')"
start_seconds="$(date +%s)"
cd "$workspace/source"
npm ci --ignore-scripts --no-audit --no-fund
npm run build-clean
build_seconds="$(( $(date +%s) - start_seconds ))"

node --input-type=module -e \
  "import { CommissioningController } from './packages/matter.js/dist/esm/export.js'; if (typeof CommissioningController !== 'function') process.exit(1)"

node_modules_kib="$(du -sk node_modules | awk '{print $1}')"
distribution_kib="$(du -sk packages/*/dist examples/controller/dist 2>/dev/null | awk '{sum += $1} END {print sum + 0}')"
package_manifest_count="$(find node_modules -type f -name package.json | wc -l | tr -d ' ')"
native_addon_count="$(find node_modules -type f -name '*.node' | wc -l | tr -d ' ')"
typescript_lines="$(git ls-files '*.ts' '*.mts' '*.cts' | xargs wc -l | awk '$2 != "total" {sum += $1} END {print sum + 0}')"
controller_typescript_lines="$(wc -l packages/matter.js/src/CommissioningController.ts | awk '{print $1}')"

jq -n \
  --arg schema "homemagic.matter.matter-js-candidate-report.v1" \
  --arg repository "$repository" \
  --arg revision "$revision" \
  --arg license "$license" \
  --arg package_lock_sha256 "$lock_sha256" \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg node "$(node --version)" \
  --arg npm "$(npm --version)" \
  --argjson build_seconds "$build_seconds" \
  --argjson source_checkout_kib "$source_checkout_kib" \
  --argjson node_modules_kib "$node_modules_kib" \
  --argjson distribution_kib "$distribution_kib" \
  --argjson package_manifest_count "$package_manifest_count" \
  --argjson native_addon_count "$native_addon_count" \
  --argjson typescript_lines "$typescript_lines" \
  --argjson controller_typescript_lines "$controller_typescript_lines" \
  '{
    schema: $schema,
    candidate: "matter-js",
    source: {
      repository: $repository,
      revision: $revision,
      license: $license,
      package_lock_sha256: $package_lock_sha256
    },
    host: {captured_at: $captured_at, os: $os, architecture: $architecture},
    toolchain: {node: $node, npm: $npm},
    build: {
      result: "pass",
      seconds: $build_seconds,
      source_checkout_kib: $source_checkout_kib,
      node_modules_kib: $node_modules_kib,
      built_distribution_kib: $distribution_kib,
      installed_package_manifests: $package_manifest_count,
      native_addons: $native_addon_count
    },
    source_inventory: {
      typescript_lines: $typescript_lines,
      commissioning_controller_lines: $controller_typescript_lines
    },
    boundary: {
      controller_import_smoke: "pass",
      production_sidecar: false,
      stable_private_protocol: false,
      lifecycle_interop: "not_run"
    }
  }' > "$report_path"

jq -e '.schema == "homemagic.matter.matter-js-candidate-report.v1" and .build.result == "pass"' \
  "$report_path" >/dev/null
echo "matter.js candidate report written to $report_path"
