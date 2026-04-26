#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/jig-yml.sh get <answers-file> <key>
  scripts/jig-yml.sh set <answers-file> <key> <value>
EOF
  exit 2
}

if [[ $# -lt 1 ]]; then
  usage
fi

command="$1"
shift

case "$command" in
  get)
    if [[ $# -ne 2 ]]; then
      usage
    fi
    python3 - "$1" "$2" <<'PY'
import pathlib
import re
import sys

answers_path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
pattern = re.compile(rf"^{re.escape(key)}:\s*(.*)$")

for line in answers_path.read_text().splitlines():
    match = pattern.match(line)
    if not match:
        continue
    raw = match.group(1).strip()
    if raw in {"''", '""'}:
        print("")
    elif len(raw) >= 2 and raw[0] == "'" and raw[-1] == "'":
        print(raw[1:-1].replace("''", "'"))
    elif len(raw) >= 2 and raw[0] == '"' and raw[-1] == '"':
        print(raw[1:-1])
    else:
        print(raw)
    break
PY
    ;;
  set)
    if [[ $# -ne 3 ]]; then
      usage
    fi
    python3 - "$1" "$2" "$3" <<'PY'
import pathlib
import sys

answers_path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
value = sys.argv[3]
lines = answers_path.read_text().splitlines()
escaped = value.replace("'", "''")

for index, line in enumerate(lines):
    if line.startswith(f"{key}: "):
        lines[index] = f"{key}: '{escaped}'"
        break
else:
    lines.append(f"{key}: '{escaped}'")

answers_path.write_text("\n".join(lines) + "\n")
PY
    ;;
  *)
    usage
    ;;
esac
