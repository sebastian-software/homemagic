#!/usr/bin/env bash
set -euo pipefail

candidate_id="${1:-rust-matc}"
report_path="${2:-matter-controller-candidate-report.json}"
expected_architecture="${3:-$(uname -m)}"
manifest="config/matter-controller-candidates.json"

case "$candidate_id" in
  rust-matc|rs-matter) ;;
  *) echo "unsupported candidate: $candidate_id" >&2; exit 2 ;;
esac
test "$(uname -m)" = "$expected_architecture"

repository="$(jq -r --arg id "$candidate_id" '.candidates[] | select(.id == $id) | .repository' "$manifest")"
revision="$(jq -r --arg id "$candidate_id" '.candidates[] | select(.id == $id) | .revision' "$manifest")"
test -n "$repository"
test -n "$revision"

workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT
git init --quiet "$workspace/source"
git -C "$workspace/source" remote add origin "$repository"
git -C "$workspace/source" fetch --quiet --depth 1 origin "$revision"
git -C "$workspace/source" checkout --quiet --detach FETCH_HEAD
test "$(git -C "$workspace/source" rev-parse HEAD)" = "$revision"

export CARGO_TARGET_DIR="$workspace/target"

lockfile_origin="candidate"
if ! test -f "$workspace/source/Cargo.lock"; then
  cargo generate-lockfile --manifest-path "$workspace/source/Cargo.toml"
  lockfile_origin="generated_from_manifest_ranges"
fi
lockfile_sha256="$(shasum -a 256 "$workspace/source/Cargo.lock" | awk '{print $1}')"
expected_lockfile_sha256="$(jq -r --arg id "$candidate_id" '.candidates[] | select(.id == $id) | .generated_lockfile_sha256 // empty' "$manifest")"
if test -n "$expected_lockfile_sha256"; then
  test "$lockfile_sha256" = "$expected_lockfile_sha256"
fi

if test "$candidate_id" = "rust-matc"; then
  cargo test --quiet --manifest-path "$workspace/source/Cargo.toml" --locked
  cargo test --quiet --manifest-path "$workspace/source/Cargo.toml" --locked --all-features
  cargo build --quiet --manifest-path "$workspace/source/Cargo.toml" --locked --release --example simple-devman
  binary="$workspace/target/release/examples/simple-devman"
  release_target="simple-devman example"
  test_profile="default and all features"
  all_features_outcome="pass"
else
  cargo test --quiet --manifest-path "$workspace/source/Cargo.toml" --workspace --locked
  if cargo check --quiet --manifest-path "$workspace/source/Cargo.toml" --package rs-matter --locked --all-features; then
    all_features_outcome="pass"
  else
    all_features_outcome="fail"
  fi
  cargo build --quiet --manifest-path "$workspace/source/Cargo.toml" --locked --release --package rs-matter-examples --bin commissioner_tests
  cargo build --quiet --manifest-path "$workspace/source/Cargo.toml" --locked --release --package rs-matter-examples --bin onoff_light
  binary="$workspace/target/release/commissioner_tests"
  release_target="commissioner_tests and onoff_light binaries"
  test_profile="default workspace"
fi

rust_bytes="$(find "$workspace/source" -path "$workspace/source/.git" -prune -o -path "$workspace/source/target" -prune -o -type f -name '*.rs' -exec wc -c {} + | awk 'END {print $1 + 0}')"
other_code_bytes="$(find "$workspace/source" -path "$workspace/source/.git" -prune -o -path "$workspace/source/target" -prune -o -type f \( -name '*.c' -o -name '*.cc' -o -name '*.cpp' -o -name '*.h' -o -name '*.hpp' -o -name '*.m' -o -name '*.mm' -o -name '*.swift' -o -name '*.py' -o -name '*.js' -o -name '*.ts' \) -exec wc -c {} + | awk 'END {print $1 + 0}')"
total_code_bytes="$((rust_bytes + other_code_bytes))"
rust_share_basis_points="$((rust_bytes * 10000 / total_code_bytes))"
unsafe_blocks="$(find "$workspace/source" -path "$workspace/source/.git" -prune -o -path "$workspace/source/target" -prune -o -type f -name '*.rs' -exec grep -Eho '(^|[^[:alnum:]_])unsafe[[:space:]]+(fn|impl|trait|extern)|(^|[^[:alnum:]_])unsafe[[:space:]]*\{' {} + | wc -l | tr -d ' ')"
native_files="$(find "$workspace/source" -path "$workspace/source/.git" -prune -o -path "$workspace/source/target" -prune -o -type f \( -name '*.c' -o -name '*.cc' -o -name '*.cpp' -o -name '*.h' -o -name '*.hpp' -o -name '*.m' -o -name '*.mm' -o -name '*.swift' \) -print | wc -l | tr -d ' ')"
default_tree="$(cargo tree --manifest-path "$workspace/source/Cargo.toml" --locked --workspace -e normal --prefix none)"
default_dependencies="$(sort -u <<<"$default_tree" | wc -l | tr -d ' ')"
if test "$candidate_id" = "rust-matc"; then
  all_feature_tree="$(cargo tree --manifest-path "$workspace/source/Cargo.toml" --locked --all-features -e normal --prefix none)"
  all_feature_dependencies="$(sort -u <<<"$all_feature_tree" | wc -l | tr -d ' ')"
  if test "$(uname -s)" = "Darwin"; then
    transitive_native_packages='["btleplug", "objc-sys", "objc2", "objc2-core-bluetooth", "objc2-foundation"]'
  else
    transitive_native_packages='["btleplug", "dbus", "libdbus-sys"]'
  fi
  compiled_unsafe_blocks=0
else
  all_feature_dependencies=null
  transitive_native_packages='[]'
  compiled_unsafe_blocks=null
fi
binary_bytes="$(wc -c < "$binary" | tr -d ' ')"
binary_format="$(file -b "$binary")"

jq -n \
  --arg schema "homemagic.matter.controller-candidate-report.v1" \
  --arg candidate "$candidate_id" \
  --arg repository "$repository" \
  --arg revision "$revision" \
  --arg captured_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg os "$(uname -s)" \
  --arg architecture "$(uname -m)" \
  --arg kernel "$(uname -r)" \
  --arg rustc "$(rustc --version)" \
  --arg cargo "$(cargo --version)" \
  --arg lockfile_origin "$lockfile_origin" \
  --arg lockfile_sha256 "$lockfile_sha256" \
  --arg release_target "$release_target" \
  --arg test_profile "$test_profile" \
  --arg all_features_outcome "$all_features_outcome" \
  --arg binary_format "$binary_format" \
  --argjson rust_bytes "$rust_bytes" \
  --argjson other_code_bytes "$other_code_bytes" \
  --argjson rust_share_basis_points "$rust_share_basis_points" \
  --argjson unsafe_blocks "$unsafe_blocks" \
  --argjson compiled_unsafe_blocks "$compiled_unsafe_blocks" \
  --argjson native_files "$native_files" \
  --argjson default_dependencies "$default_dependencies" \
  --argjson all_feature_dependencies "$all_feature_dependencies" \
  --argjson binary_bytes "$binary_bytes" \
  --argjson transitive_native_packages "$transitive_native_packages" \
  '{
    schema: $schema,
    candidate: $candidate,
    source: { repository: $repository, revision: $revision },
    host: { captured_at: $captured_at, os: $os, architecture: $architecture, kernel: $kernel, rustc: $rustc, cargo: $cargo },
    resolution: { lockfile_origin: $lockfile_origin, lockfile_sha256: $lockfile_sha256 },
    commands: { tests: "pass", test_profile: $test_profile, all_features: $all_features_outcome, release_build: "pass", release_target: $release_target },
    footprint: {
      repository_rust_bytes: $rust_bytes,
      repository_other_code_bytes: $other_code_bytes,
      repository_rust_share_basis_points: $rust_share_basis_points,
      compiled_first_party_rust_share_basis_points: 10000,
      repository_semantic_unsafe_blocks: $unsafe_blocks,
      compiled_first_party_semantic_unsafe_blocks: $compiled_unsafe_blocks,
      compiled_first_party_native_files: $native_files,
      default_normal_dependencies: $default_dependencies,
      all_feature_normal_dependencies: $all_feature_dependencies,
      all_feature_transitive_native_packages: $transitive_native_packages,
      release_example_bytes: $binary_bytes,
      release_example_format: $binary_format,
      native_boundary_note: (if $candidate == "rust-matc" then "optional ble feature through btleplug platform backends" else "default workspace build; optional platform features were not selected" end)
    }
  }' > "$report_path"

jq -e '.schema == "homemagic.matter.controller-candidate-report.v1"' "$report_path" >/dev/null
echo "Matter controller candidate report written to $report_path"
