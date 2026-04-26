#!/usr/bin/env bash

FIXTURE_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT_DIR="${ROOT_DIR:-$(cd "$FIXTURE_SCRIPT_DIR/../.." && pwd -P)}"
JIG_YML="${JIG_YML:-$ROOT_DIR/scripts/jig-yml.sh}"

json_get() {
  local expression="$1"
  python3 -c '
import json
import sys

expr = sys.argv[1]
data = json.load(sys.stdin)

value = data
for segment in expr.split("."):
    if not segment:
        continue
    if segment.isdigit():
        value = value[int(segment)]
    else:
        value = value[segment]

if isinstance(value, (dict, list)):
    print(json.dumps(value))
else:
    print(value)
' "$expression"
}

answers_get() {
  "$JIG_YML" get "$1" "$2"
}

answers_set() {
  "$JIG_YML" set "$1" "$2" "$3"
}

render_fixture() {
  local answers_file="$1"
  local dest_dir="$2"

  uvx --from copier copier copy \
    --trust \
    --defaults \
    --data-file "$answers_file" \
    "$ROOT_DIR" \
    "$dest_dir"
}

render_fixture_from_template() {
  local template_root="$1"
  local answers_file="$2"
  local dest_dir="$3"

  uvx --from copier copier copy \
    --trust \
    --defaults \
    --data-file "$answers_file" \
    "$template_root" \
    "$dest_dir"
}

create_template_snapshot_repo() {
  local snapshot_dir="$1"

  mkdir -p "$snapshot_dir"
  (
    cd "$ROOT_DIR"
    tar cf - --exclude='.git' .
  ) | (
    cd "$snapshot_dir"
    tar xf -
  )

  (
    cd "$snapshot_dir"
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    git add .
    git commit -m "template snapshot" >/dev/null
  )
}

fixture_create_tmp_dir_if_needed() {
  if [[ -z "${TMP_DIR:-}" ]]; then
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT
  fi
}
