#!/usr/bin/env bash
set -eo pipefail

report_path="${1:-connectedhomeip-candidate-report.json}"
expected_architecture="${2:-$(uname -m)}"
manifest="config/matter-controller-candidates.json"

test "$(uname -m)" = "$expected_architecture"

repository="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .repository' "$manifest")"
revision="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .revision' "$manifest")"
release="$(jq -r '.candidates[] | select(.id == "connectedhomeip") | .release' "$manifest")"
test -n "$repository"
test -n "$revision"

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

target="${platform}-${target_architecture}-chip-tool-no-ble-no-wifi-no-thread"
workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT

git init --quiet "$workspace/source"
git -C "$workspace/source" remote add origin "$repository"
git -C "$workspace/source" fetch --quiet --depth 1 origin "$revision"
git -C "$workspace/source" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$workspace/source" rev-parse HEAD)" = "$revision"

python3 "$workspace/source/scripts/checkout_submodules.py" \
  --shallow --platform "$platform" --jobs 8

export GITHUB_ACTION=1
export PW_ENVIRONMENT_ROOT="$workspace/environment"
start_seconds="$(date +%s)"
cd "$workspace/source"
source scripts/bootstrap.sh -p "$platform"
python3 scripts/build/build_examples.py --target "$target" build
build_seconds="$(( $(date +%s) - start_seconds ))"

binary="$workspace/source/out/$target/chip-tool"
test -x "$binary"
set +e
"$binary" --help > "$workspace/chip-tool-help.txt" 2>&1
help_exit_code="$?"
set -e
# chip-tool prints the complete command catalog and exits with 1 when no
# subcommand was selected. Treat that documented CLI behavior as a smoke pass,
# but retain the exact status in the report so it cannot be mistaken for an
# embeddable health check.
test "$help_exit_code" -eq 0 -o "$help_exit_code" -eq 1
grep -q 'Command sets:' "$workspace/chip-tool-help.txt"

binary_bytes="$(wc -c < "$binary" | tr -d ' ')"
binary_format="$(file -b "$binary")"
source_kib="$(du -sk "$workspace/source" | awk '{print $1}')"
environment_kib="$(du -sk "$workspace/environment" | awk '{print $1}')"
submodule_count="$(git submodule status --recursive | wc -l | tr -d ' ')"
submodule_manifest_sha256="$(git submodule status --recursive | shasum -a 256 | awk '{print $1}')"
controller_cpp_lines="$(find src/controller examples/chip-tool -type f \( -name '*.c' -o -name '*.cc' -o -name '*.cpp' -o -name '*.h' -o -name '*.hpp' \) -exec wc -l {} + | awk 'END {print $1 + 0}')"
python_binding_exports="$(grep -Rho 'pychip_[A-Za-z0-9_]*' src/controller/python --include='*.cpp' --include='*.h' | sort -u | wc -l | tr -d ' ')"

jq -n \
  --arg schema "homemagic.matter.connectedhomeip-candidate-report.v1" \
  --arg repository "$repository" \
  --arg revision "$revision" \
  --arg release "$release" \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg target "$target" \
  --arg binary_format "$binary_format" \
  --arg submodule_manifest_sha256 "$submodule_manifest_sha256" \
  --argjson build_seconds "$build_seconds" \
  --argjson binary_bytes "$binary_bytes" \
  --argjson source_kib "$source_kib" \
  --argjson environment_kib "$environment_kib" \
  --argjson submodule_count "$submodule_count" \
  --argjson controller_cpp_lines "$controller_cpp_lines" \
  --argjson python_binding_exports "$python_binding_exports" \
  --argjson help_exit_code "$help_exit_code" \
  '{
    schema: $schema,
    candidate: "connectedhomeip",
    source: {repository: $repository, revision: $revision, release: $release},
    host: {captured_at: $captured_at, os: $os, architecture: $architecture},
    build: {
      target: $target,
      result: "pass",
      seconds: $build_seconds,
      binary_bytes: $binary_bytes,
      binary_format: $binary_format,
      source_checkout_kib: $source_kib,
      bootstrap_environment_kib: $environment_kib,
      submodule_count: $submodule_count,
      submodule_manifest_sha256: $submodule_manifest_sha256,
      cli_help_exit_code: $help_exit_code
    },
    boundary: {
      evaluated: "chip-tool process plus source-level controller ABI survey",
      controller_cpp_lines: $controller_cpp_lines,
      existing_python_binding_exports: $python_binding_exports,
      stable_narrow_c_abi: false,
      production_adapter: false
    }
  }' > "$report_path"

jq -e '.schema == "homemagic.matter.connectedhomeip-candidate-report.v1" and .build.result == "pass"' \
  "$report_path" >/dev/null
echo "ConnectedHomeIP candidate report written to $report_path"
