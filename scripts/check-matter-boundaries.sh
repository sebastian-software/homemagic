#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repository_root"

for package in homemagic-domain homemagic-application; do
  dependency_tree="$(cargo tree --locked --edges normal --prefix none -p "$package")"
  if printf '%s\n' "$dependency_tree" \
    | rg --ignore-case --quiet '^(homemagic-matter|rs-matter|matter-rs|matterjs|connectedhomeip)( |$)'; then
    printf 'forbidden Matter integration dependency found in %s\n' "$package" >&2
    exit 1
  fi
done

if rg --quiet 'homemagic-matter|rs-matter|matter-rs|matterjs|connectedhomeip' \
  crates/homemagic-domain/Cargo.toml crates/homemagic-application/Cargo.toml; then
  printf 'forbidden Matter integration dependency found in core manifest\n' >&2
  exit 1
fi

if rg --quiet '(rs-matter|matter-rs|matterjs|connectedhomeip|tom-code/rust-matc|^matc[[:space:]]*=)' \
  Cargo.toml crates/*/Cargo.toml Cargo.lock; then
  printf 'candidate or reference Matter dependency entered a production manifest\n' >&2
  exit 1
fi

printf 'Matter domain/application dependency boundaries are intact.\n'
