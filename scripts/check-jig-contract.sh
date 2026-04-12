#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

python3 - "$ROOT_DIR" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
manifest_path = root / ".agent" / "jig-contract.json"
answers_path = root / ".jig.yml"
makefile_path = root / "Makefile"
mcp_path = root / ".mcp.json"
jig_script = root / "scripts" / "jig"
install_script = root / "scripts" / "install-jig.sh"

manifest = json.loads(manifest_path.read_text())
answers_text = answers_path.read_text()
makefile_text = makefile_path.read_text()

errors = []

match = re.search(r'^jig_version:\s*[\'"]?([^\'"\n]+)[\'"]?$', answers_text, re.MULTILINE)
if not match:
    errors.append("Missing jig_version in .jig.yml.")
else:
    answers_version = match.group(1)
    if answers_version != manifest["jig_version"]:
        errors.append(
            f"jig_version mismatch: .jig.yml has {answers_version}, manifest has {manifest['jig_version']}."
        )

targets = set(re.findall(r"^([A-Za-z0-9._-]+):", makefile_text, re.MULTILINE))
missing_targets = [target for target in manifest["required_make_targets"] if target not in targets]
if missing_targets:
    errors.append(f"Missing required Make targets: {', '.join(missing_targets)}.")

if not mcp_path.exists():
    errors.append("Missing .mcp.json.")
if not jig_script.exists():
    errors.append("Missing scripts/jig launcher.")
if not install_script.exists():
    errors.append("Missing scripts/install-jig.sh installer.")

tool_names = [tool["name"] for tool in manifest["tools"]]
required_tools = {
    "jig.fmt_check",
    "jig.clippy",
    "jig.test",
    "jig.contract_check",
    "jig.session_start",
    "jig.plans_open",
    "jig.receipts_list",
    "jig.decisions_add",
}
if "schema-check" in manifest["required_make_targets"]:
    required_tools.add("jig.schema_check")
if "schema-dump" in manifest["required_make_targets"]:
    required_tools.add("jig.schema_dump")
if "sqlx-check" in manifest["required_make_targets"]:
    required_tools.add("jig.sqlx_check")
if "migration-add" in manifest["required_make_targets"]:
    required_tools.add("jig.migration_add")
missing_tools = sorted(required_tools.difference(tool_names))
if missing_tools:
    errors.append(f"Missing required jig tool definitions: {', '.join(missing_tools)}.")

if errors:
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    sys.exit(1)

print("jig contract check passed.")
print(f"  - manifest: {manifest_path}")
print(f"  - jig version: {manifest['jig_version']}")
print(f"  - tool definitions: {len(tool_names)}")
PY
