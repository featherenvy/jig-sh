#!/usr/bin/env bash

if ! declare -F json_get >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi

validate_jig_mcp_smoke() {
  local repo_dir="$1"
  local expect_schema_dump="$2"
  local expect_sqlx="$3"

  REPO_DIR="$repo_dir" EXPECT_SCHEMA_DUMP="$expect_schema_dump" EXPECT_SQLX="$expect_sqlx" python3 <<'PY'
import json
import os
import pathlib
import subprocess

repo_dir = pathlib.Path(os.environ["REPO_DIR"])
expect_schema_dump = os.environ["EXPECT_SCHEMA_DUMP"] == "1"
expect_sqlx = os.environ["EXPECT_SQLX"] == "1"
proc = subprocess.Popen(
    [str(repo_dir / "scripts" / "jig"), "mcp"],
    cwd=repo_dir,
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)

def send(message):
    body = json.dumps(message).encode()
    proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    proc.stdin.flush()

def recv():
    headers = {}
    while True:
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("MCP server closed stdout unexpectedly")
        if line == b"\r\n":
            break
        name, value = line.decode().split(":", 1)
        headers[name.lower()] = value.strip()

    body = proc.stdout.read(int(headers["content-length"]))
    return json.loads(body)

send({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "fixture", "version": "1"},
    },
})
response = recv()
assert response["result"]["serverInfo"]["name"] == "jig", response

send({"jsonrpc": "2.0", "method": "notifications/initialized"})
send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
response = recv()
tool_names = {tool["name"] for tool in response["result"]["tools"]}
assert "jig.fmt_check" in tool_names, tool_names
assert ("jig.schema_check" in tool_names) == expect_sqlx, tool_names
assert ("jig.schema_dump" in tool_names) == expect_schema_dump, tool_names
assert ("jig.sqlx_check" in tool_names) == expect_sqlx, tool_names
assert ("jig.migration_add" in tool_names) == expect_sqlx, tool_names
assert "jig.agent_doctor" in tool_names, tool_names
assert "jig.work_start" in tool_names, tool_names
assert "jig.session_start" not in tool_names, tool_names

send({
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
        "name": "jig.work_status",
        "arguments": {},
    },
})
response = recv()
content = response["result"]["structuredContent"]
assert content["ok"] is True, response
assert "counts" in content, response

proc.terminate()
proc.wait(timeout=5)
PY
}

validate_jig_runtime() {
  local repo_dir="$1"
  local expect_schema_dump="$2"
  local expect_sqlx="$3"
  local migration_name="${4:-}"

  (
    cd "$repo_dir"
    [[ -f .mcp.json ]]
    [[ -f .agent/jig-contract.json ]]
    make contract-check >/dev/null

    EXPECT_SCHEMA_DUMP="$expect_schema_dump" EXPECT_SQLX="$expect_sqlx" python3 <<'PY'
import json
import os
import pathlib

manifest = json.loads(pathlib.Path(".agent/jig-contract.json").read_text())
expect_schema_dump = os.environ["EXPECT_SCHEMA_DUMP"] == "1"
expect_sqlx = os.environ["EXPECT_SQLX"] == "1"
targets = set(manifest["required_make_targets"])
tools = {tool["name"] for tool in manifest["tools"]}

assert ("schema-dump" in targets) == expect_schema_dump, manifest
assert ("schema-check" in targets) == expect_sqlx, manifest
assert ("jig.schema_dump" in tools) == expect_schema_dump, manifest
assert ("jig.schema_check" in tools) == expect_sqlx, manifest
assert ("sqlx-check" in targets) == expect_sqlx, manifest
assert ("migration-add" in targets) == expect_sqlx, manifest
assert ("jig.sqlx_check" in tools) == expect_sqlx, manifest
assert ("jig.migration_add" in tools) == expect_sqlx, manifest
assert "jig.session_start" not in tools, manifest
PY

    local work_json
    local plan_id
    local receipts_json

    rm -rf .git/jig-tools .agent/.cache
    env -u JIG_DEV_BIN scripts/install-jig.sh >/dev/null
    validate_jig_mcp_smoke "$repo_dir" "$expect_schema_dump" "$expect_sqlx"

    work_json="$(scripts/jig work start --title "Fixture runtime plan" --body "## Fixture\nRuntime validation.")"
    plan_id="$(printf '%s' "$work_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["plan"]["plan_id"])')"

    if [[ "$expect_sqlx" == "1" ]]; then
      scripts/jig migration-add "$migration_name" --plan-id "$plan_id" >/dev/null
    fi

    scripts/jig work check --plan-id "$plan_id" >/dev/null
    scripts/jig work gates --plan-id "$plan_id" >/dev/null

    scripts/jig work decide \
      --title "Fixture decision" \
      --selected-option "Use jig" \
      --rationale "Runtime contract is wired and validated." \
      --plan-id "$plan_id" \
      --alternatives "Plain make" \
      >/dev/null

    receipts_json="$(scripts/jig work receipts --plan-id "$plan_id" --limit 20)"
    RECEIPTS_JSON="$receipts_json" EXPECT_SQLX="$expect_sqlx" EXPECT_SCHEMA_DUMP="$expect_schema_dump" python3 <<'PY'
import json
import os

payload = json.loads(os.environ["RECEIPTS_JSON"])
tools = {receipt["tool_name"] for receipt in payload["receipts"]}
required = {
    "jig.plans_open",
    "jig.contract_check",
    "jig.test",
    "jig.decisions_add",
}
if os.environ["EXPECT_SQLX"] == "1":
    required.update({"jig.sqlx_check", "jig.schema_check", "jig.migration_add"})
if os.environ["EXPECT_SCHEMA_DUMP"] == "1":
    required.add("jig.schema_dump")

missing = sorted(required - tools)
if missing:
    raise SystemExit(f"Missing expected runtime receipts: {', '.join(missing)}")
PY

    scripts/jig work finish --plan-id "$plan_id" --resolution "fixture complete" --outcome success >/dev/null

    [[ -f ".agent/plans/${plan_id}.md" ]]
    rg -q "Runtime validation" ".agent/plans/${plan_id}.md"
    [[ -f .agent/state/receipts.jsonl ]]
    [[ -f .agent/state/decisions.jsonl ]]
    [[ -f ".git/jig-tools/0.1.0/bin/jig" ]]
    if [[ "$expect_sqlx" == "1" ]]; then
      find crates/acme-db/migrations -name "*_${migration_name}.up.sql" | grep -q .
    fi
  )
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  if [[ "$#" -lt 3 || "$#" -gt 4 ]]; then
    echo "Usage: $0 REPO_DIR EXPECT_SCHEMA_DUMP EXPECT_SQLX [MIGRATION_NAME]" >&2
    exit 2
  fi

  validate_jig_runtime "$@"
fi
