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
no_mod_script = root / "scripts" / "check-no-mod-rs.sh"
# These templates are byte-for-byte script mirrors by design. If a future
# template needs Jinja directives, replace this parity check with a render check.
script_template_pairs = [
    (jig_toml_script, jig_toml_template),
    (install_script, root / "templates" / "project" / "scripts" / "install-jig.sh.jinja"),
    (root / "scripts" / "check-jig-contract.sh", root / "templates" / "project" / "scripts" / "check-jig-contract.sh.jinja"),
]

manifest = json.loads(manifest_path.read_text())

errors = []

def load_answers():
    try:
        import tomllib
    except ModuleNotFoundError:
        tomllib = None

    if tomllib is not None:
        return tomllib.loads(answers_path.read_text())

    match = re.search(r"^rust_crate_roots\s*=\s*\[(.*?)\]\s*$", answers_path.read_text(), re.MULTILINE)
    if not match:
        return {}
    roots = []
    for item in re.finditer(r'"((?:\\.|[^"\\])*)"', match.group(1)):
        roots.append(bytes(item.group(1), "utf-8").decode("unicode_escape"))
    return {"rust_crate_roots": roots}


def shell_double_quote(value):
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def expected_no_mod_script(crate_roots):
    loop = ""
    if crate_roots:
        quoted_roots = "".join(f" {shell_double_quote(root)}" for root in crate_roots)
        loop = f"""for root in{quoted_roots}; do
  matches="$(git ls-files -- "$root" 2>/dev/null | awk '/(^|\\/)mod[.]rs$/' || true)"
  if [[ -n "$matches" ]]; then
    violations="$violations"$'\\n'"$matches"
  fi
done
"""
    return f"""#!/usr/bin/env bash
set -euo pipefail

violations=""
{loop}
if [[ -n "$violations" ]]; then
  echo "Disallowed Rust module file(s) found. Use named module files instead of mod.rs." >&2
  printf '%s\\n' "$violations" | sed '/^$/d' >&2
  exit 1
fi

echo "No disallowed mod.rs files found under configured crate roots."
"""


answers = load_answers()

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

if no_mod_script.exists():
    crate_roots = answers.get("rust_crate_roots", [])
    if not isinstance(crate_roots, list) or not all(isinstance(item, str) for item in crate_roots):
        errors.append("rust_crate_roots in .jig.toml must be a string array.")
    elif not crate_roots:
        errors.append("rust_crate_roots in .jig.toml must declare at least one crate root.")
    elif no_mod_script.read_text() != expected_no_mod_script(crate_roots):
        errors.append("scripts/check-no-mod-rs.sh does not match rust_crate_roots in .jig.toml.")

contract_version = manifest.get("contract_version")
supported_command_keys = {
    "bootstrap_command",
    "contract_check_command",
    "migration_add_command",
    "rust_clippy_command",
    "rust_fmt_check_command",
    "rust_test_command",
    "rust_test_locked_command",
    "schema_check_command",
    "schema_dump_command",
    "sqlx_check_command",
}
if contract_version == 1:
    if not makefile_path.exists():
        errors.append("Missing Makefile required by contract version 1.")
    else:
        makefile_text = makefile_path.read_text()
        targets = set(re.findall(r"^([A-Za-z0-9._-]+):", makefile_text, re.MULTILINE))
        missing_targets = [
            target for target in manifest.get("required_make_targets", []) if target not in targets
        ]
        if missing_targets:
            errors.append(f"Missing required Make targets: {', '.join(missing_targets)}.")
elif contract_version == 2:
    for command_key in manifest.get("required_commands", []):
        if command_key not in supported_command_keys:
            errors.append(f"Unsupported required command in jig contract: {command_key}.")
            continue
        try:
            command_result = subprocess.run(
                [str(jig_toml_script), "get", str(answers_path), command_key],
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            errors.append(f"Failed to read command key {command_key}: {error}")
            continue
        command_value = command_result.stdout.rstrip("\n")
        if command_result.returncode != 0:
            errors.append(f"Failed to read {command_key} from .jig.toml.")
        elif not command_value:
            errors.append(f"Missing required command in .jig.toml: {command_key}.")
else:
    errors.append(f"Unsupported contract_version: {contract_version}.")

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
if contract_version == 2:
    required_tools.add("jig.bootstrap")
required_make_targets = manifest.get("required_make_targets", [])
required_commands = manifest.get("required_commands", [])
if "schema-check" in required_make_targets or "schema_check_command" in required_commands:
    required_tools.add("jig.schema_check")
if "schema-dump" in required_make_targets or "schema_dump_command" in required_commands:
    required_tools.add("jig.schema_dump")
if "sqlx-check" in required_make_targets or "sqlx_check_command" in required_commands:
    required_tools.add("jig.sqlx_check")
if "migration-add" in required_make_targets or "migration_add_command" in required_commands:
    required_tools.add("jig.migration_add")
missing_tools = sorted(required_tools.difference(tool_names))
if missing_tools:
    errors.append(f"Missing required jig tool definitions: {', '.join(missing_tools)}.")

for tool in manifest["tools"]:
    kind = tool.get("kind")
    name = tool.get("name", "<unnamed>")
    if kind == "make":
        if not makefile_path.exists():
            errors.append(f"Make-backed tool {name} requires Makefile, but Makefile is missing.")
        if "target" not in tool:
            errors.append(f"Make-backed tool {name} is missing target.")
    elif kind == "command":
        command_key = tool.get("command")
        if contract_version != 2:
            errors.append(f"Command-backed tool {name} requires contract_version 2.")
        elif not command_key:
            errors.append(f"Command-backed tool {name} is missing command.")
        elif command_key not in required_commands:
            errors.append(f"Command-backed tool {name} references undeclared command {command_key}.")
    else:
        errors.append(f"Unsupported tool kind for {name}: {kind}.")

if errors:
    for error in errors:
        print(f"ERROR: {error}", file=sys.stderr)
    sys.exit(1)

print("jig contract check passed.")
print(f"  - manifest: {manifest_path}")
print(f"  - jig version: {manifest['jig_version']}")
print(f"  - tool definitions: {len(tool_names)}")
PY
