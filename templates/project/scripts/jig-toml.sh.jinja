#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/jig-toml.sh get <answers-file> <top-level-string-key>
  scripts/jig-toml.sh set <answers-file> <top-level-string-key> <value>
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
import ast
import pathlib
import re
import sys

answers_path = pathlib.Path(sys.argv[1])
key = sys.argv[2]
pattern = re.compile(rf"^\s*{re.escape(key)}\s*=\s*(.*?)\s*$")

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None

# Prefer tomllib when the local Python has it. The fallback keeps generated repos
# working on older system Python versions and intentionally supports only the
# top-level string answers used by shell scripts. set writes strings. The Rust
# runtime owns full TOML parsing.
if tomllib is not None:
    value = tomllib.loads(answers_path.read_text()).get(key, "")
    if value is None:
        value = ""
    if not isinstance(value, str):
        raise SystemExit(f"Unsupported non-string value for {key}.")
    if "\n" in value:
        raise SystemExit(f"Unsupported multiline value for {key}.")
    print(value)
    raise SystemExit(0)

def strip_inline_comment(value):
    quote = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if quote == '"' and char == "\\":
            escaped = True
            continue
        if char in {"'", '"'}:
            if quote == char:
                quote = None
            elif quote is None:
                quote = char
            continue
        if char == "#" and quote is None:
            return value[:index].strip()
    return value.strip()

found = False
for line in answers_path.read_text().splitlines():
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        continue
    if stripped.startswith("["):
        break
    match = pattern.match(line)
    if not match:
        continue
    raw = strip_inline_comment(match.group(1))
    if not raw:
        print("")
    elif raw.startswith("'''") or raw.startswith('"""'):
        raise SystemExit(f"Unsupported multiline string for {key}.")
    elif raw[0] == "'":
        if not raw.endswith("'"):
            raise SystemExit(f"Malformed literal string for {key}.")
        print(raw[1:-1])
    elif raw[0] == '"':
        # The fallback is intentionally limited to generated simple strings.
        # tomllib is authoritative when available.
        value = ast.literal_eval(raw)
        if "\n" in value:
            raise SystemExit(f"Unsupported multiline value for {key}.")
        print(value)
    elif raw[0] in {"[", "{"}:
        raise SystemExit(f"Unsupported non-scalar value for {key}.")
    else:
        raise SystemExit(f"Unsupported non-string value for {key}.")
    found = True
    break

if not found:
    print("")
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
pattern = re.compile(rf"^(\s*){re.escape(key)}\s*=")
insert_at = len(lines)
found = False

for index, line in enumerate(lines):
    if line.strip().startswith("["):
        insert_at = index
        break
    match = pattern.match(line)
    if match:
        indent = match.group(1)
        lines[index] = f"{indent}{replacement}"
        found = True
        break

if not found:
    if insert_at > 0 and lines[insert_at - 1].strip():
        lines.insert(insert_at, "")
        insert_at += 1
    lines.insert(insert_at, replacement)
    if insert_at + 1 < len(lines) and lines[insert_at + 1].strip():
        lines.insert(insert_at + 1, "")

answers_path.write_text("\n".join(lines) + "\n")
PY
    ;;
  *)
    usage
    ;;
esac
