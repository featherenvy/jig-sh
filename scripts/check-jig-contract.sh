#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

python3 - "$ROOT_DIR" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1])
manifest_path = root / ".agent" / "jig-contract.json"
answers_path = root / ".jig.toml"
makefile_path = root / "Makefile"
mcp_path = root / ".mcp.json"
jig_script = root / "scripts" / "jig"
install_script = root / "scripts" / "install-jig.sh"
jig_toml_script = root / "scripts" / "jig-toml.sh"
jig_toml_template = root / "templates" / "project" / "scripts" / "jig-toml.sh.jinja"
# These templates are byte-for-byte script mirrors by design. If a future
# template needs Jinja directives, replace this parity check with a render check.
script_template_pairs = [
    (jig_toml_script, jig_toml_template),
    (install_script, root / "templates" / "project" / "scripts" / "install-jig.sh.jinja"),
    (root / "scripts" / "check-jig-contract.sh", root / "templates" / "project" / "scripts" / "check-jig-contract.sh.jinja"),
]

manifest = json.loads(manifest_path.read_text())
makefile_text = makefile_path.read_text()

errors = []

if "memory_schema_version" in manifest:
    errors.append("Remove memory_schema_version; runtime-owned state is not versioned in .agent/jig-contract.json.")

if not jig_toml_script.exists():
    errors.append("Missing scripts/jig-toml.sh helper.")
else:
    try:
        version_result = subprocess.run(
            [str(jig_toml_script), "get", str(answers_path), "jig_version"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as error:
        errors.append(f"Failed to run scripts/jig-toml.sh helper: {error}")
    else:
        answers_version = version_result.stdout.rstrip("\n")
        if version_result.returncode != 0:
            errors.append("Failed to read jig_version from .jig.toml.")
        elif not answers_version:
            errors.append("Missing jig_version in .jig.toml.")
        elif answers_version != manifest["jig_version"]:
            errors.append(
                f"jig_version mismatch: .jig.toml has {answers_version}, manifest has {manifest['jig_version']}."
            )

for script_path, template_path in script_template_pairs:
    if template_path.exists() and script_path.read_text() != template_path.read_text():
        errors.append(f"{script_path.relative_to(root)} and {template_path.relative_to(root)} differ.")

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
memory_tools = [tool["name"] for tool in manifest["tools"] if tool.get("kind") == "memory"]
if memory_tools:
    errors.append(
        "Runtime state tools must not be declared in .agent/jig-contract.json: "
        + ", ".join(sorted(memory_tools))
    )
required_tools = {
    "jig.fmt_check",
    "jig.clippy",
    "jig.test",
    "jig.contract_check",
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
