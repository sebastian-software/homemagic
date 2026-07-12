#!/usr/bin/env bash
set -euo pipefail

manifest="config/matter-controller-candidates.json"
workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT

jq -e '
  .schema == "homemagic.matter.controller-candidates.v1" and
  (.candidates | length > 0) and
  ([.candidates[].id] | length == (unique | length)) and
  (all(.candidates[];
    (.id | type == "string" and length > 0) and
    (.repository | startswith("https://github.com/") and endswith(".git")) and
    (.revision | test("^[0-9a-f]{40}$")) and
    (.role | type == "string" and length > 0) and
    (.disposition | type == "string" and length > 0)))
' "$manifest" >/dev/null

while IFS=$'\t' read -r id repository revision; do
  candidate="$workspace/$id"
  git init --quiet "$candidate"
  git -C "$candidate" remote add origin "$repository"
  git -C "$candidate" fetch --quiet --depth 1 origin "$revision"
  resolved="$(git -C "$candidate" rev-parse FETCH_HEAD^{commit})"
  test "$resolved" = "$revision"
done < <(jq -r '.candidates[] | [.id, .repository, .revision] | @tsv' "$manifest")

echo "Matter controller candidate pins are valid and fetchable."
