#!/usr/bin/env bash
set -euo pipefail

pattern='(password|passwd|secret|authorization:|api[_-]?key|bearer[[:space:]])'

if rg -n -i "$pattern" crates/*/tests/fixtures docs/evidence/hardware; then
  echo "Potential plaintext secret found in a committed fixture or evidence report." >&2
  exit 1
fi

echo "No plaintext-secret patterns found in committed fixtures or evidence reports."
