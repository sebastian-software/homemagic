#!/usr/bin/env bash
set -euo pipefail

output_path="${1:-matter-js-sidecar-package}"
expected_architecture="${2:-$(uname -m)}"
candidate_manifest="config/matter-js-candidate.json"

test "$(uname -m)" = "$expected_architecture"
test "$(node --version)" = "v$(jq -r '.node' "$candidate_manifest")"

case "$output_path" in
  /*) ;;
  *) output_path="$PWD/$output_path" ;;
esac

repository="$(jq -r '.repository' "$candidate_manifest")"
revision="$(jq -r '.revision' "$candidate_manifest")"
lock_sha256="$(jq -r '.package_lock_sha256' "$candidate_manifest")"
node_version="$(node --version)"
homemagic_root="$PWD"
workspace="$(mktemp -d)"
cleanup() {
  status="$?"
  trap - EXIT
  rm -rf "$workspace"
  exit "$status"
}
trap cleanup EXIT

git init --quiet "$workspace/source"
git -C "$workspace/source" remote add origin "$repository"
git -C "$workspace/source" fetch --quiet --depth 1 origin "$revision"
git -C "$workspace/source" checkout --quiet --detach FETCH_HEAD
test "$(shasum -a 256 "$workspace/source/package-lock.json" | awk '{print $1}')" = "$lock_sha256"
cp "$homemagic_root/sidecars/matter-js/src/main.mjs" "$workspace/source/homemagic-sidecar.mjs"
cp "$homemagic_root/sidecars/matter-js/src/bun-sqlite-stub.mjs" "$workspace/source/homemagic-bun-sqlite-stub.mjs"
cp "$homemagic_root/sidecars/matter-js/src/storage.mjs" "$workspace/source/storage.mjs"

(
  cd "$workspace/source"
  npm ci --ignore-scripts --no-audit --no-fund
  npm run build-clean
  ./node_modules/.bin/esbuild \
    "$workspace/source/homemagic-sidecar.mjs" \
    --bundle \
    --platform=node \
    --format=esm \
    --conditions=esbuild \
    --alias:bun:sqlite="$workspace/source/homemagic-bun-sqlite-stub.mjs" \
    --external:@stoprocent/noble \
    --external:@stoprocent/bluetooth-hci-socket \
    --minify \
    --keep-names \
    --outfile="$workspace/sidecar.mjs"
)

test ! -e "$output_path"
mkdir -p "$output_path/bin" "$output_path/lib" "$output_path/licenses"
cp "$workspace/sidecar.mjs" "$output_path/sidecar.mjs"
cp "$(command -v node)" "$output_path/bin/node"
node_root="$(cd "$(dirname "$(command -v node)")/.." && pwd)"
test -f "$node_root/LICENSE"
cp "$node_root/LICENSE" "$output_path/licenses/Node.js-LICENSE"
cp "$workspace/source/LICENSE" "$output_path/licenses/matter.js-LICENSE"

runtime_library_name=""
runtime_library_sha256=""
runtime_library_bytes=0
runtime_library_source="$(find "$node_root/lib" -maxdepth 1 -type f -name 'libnode*' -print -quit 2>/dev/null || true)"
if test -n "$runtime_library_source"; then
  runtime_library_name="$(basename "$runtime_library_source")"
  cp "$runtime_library_source" "$output_path/lib/$runtime_library_name"
  runtime_library_sha256="$(shasum -a 256 "$output_path/lib/$runtime_library_name" | awk '{print $1}')"
  runtime_library_bytes="$(wc -c < "$output_path/lib/$runtime_library_name" | tr -d ' ')"
fi

bundle_sha256="$(shasum -a 256 "$output_path/sidecar.mjs" | awk '{print $1}')"
node_sha256="$(shasum -a 256 "$output_path/bin/node" | awk '{print $1}')"
bundle_bytes="$(wc -c < "$output_path/sidecar.mjs" | tr -d ' ')"
node_bytes="$(wc -c < "$output_path/bin/node" | tr -d ' ')"

jq -n \
  --arg schema "homemagic.matter.matter-js-package.v1" \
  --arg revision "$revision" \
  --arg node_version "$node_version" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg bundle_sha256 "$bundle_sha256" \
  --arg node_sha256 "$node_sha256" \
  --arg runtime_library_name "$runtime_library_name" \
  --arg runtime_library_sha256 "$runtime_library_sha256" \
  --argjson bundle_bytes "$bundle_bytes" \
  --argjson node_bytes "$node_bytes" \
  --argjson runtime_library_bytes "$runtime_library_bytes" \
  '{
    schema: $schema,
    matter_js_revision: $revision,
    node_version: $node_version,
    host: {os: $os, architecture: $architecture},
    files: {
      "sidecar.mjs": {sha256: $bundle_sha256, bytes: $bundle_bytes},
      "bin/node": {sha256: $node_sha256, bytes: $node_bytes},
      runtime_library: (if $runtime_library_name == "" then null else {path: ("lib/" + $runtime_library_name), sha256: $runtime_library_sha256, bytes: $runtime_library_bytes} end),
      licenses: ["Node.js-LICENSE", "matter.js-LICENSE"]
    },
    advertised_methods: ["fabric_load", "fabric_create", "node_commission", "node_inventory", "node_remove", "health_check", "process_drain"],
    production_selected: false
  }' > "$output_path/manifest.json"

"$output_path/bin/node" "$output_path/sidecar.mjs" </dev/null >/dev/null
jq -e '.production_selected == false' "$output_path/manifest.json" >/dev/null
echo "matter.js sidecar package written to $output_path"
