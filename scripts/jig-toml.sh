#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/jig-toml.sh get <answers-file> <key>
  scripts/jig-toml.sh set <answers-file> <key> <value>
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
import tomllib

answers_path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
data = tomllib.loads(answers_path.read_text())
value = data.get(key, "")
if value is None:
    value = ""
print(value)
PY
    ;;
  set)
    if [[ $# -ne 3 ]]; then
      usage
    fi
    python3 - "$1" "$2" "$3" <<'PY'
import pathlib
import re
import sys

answers_path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
value = sys.argv[3]
lines = answers_path.read_text().splitlines()
escaped = value.replace("\\", "\\\\").replace('"', '\\"')
replacement = f'{key} = "{escaped}"'
pattern = re.compile(rf"^{re.escape(key)}\s*=")

for index, line in enumerate(lines):
    if pattern.match(line):
        lines[index] = replacement
        break
else:
    lines.append(replacement)

answers_path.write_text("\n".join(lines) + "\n")
PY
    ;;
  *)
    usage
    ;;
esac
