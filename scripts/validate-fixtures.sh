#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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
  local answers_file="$1"
  local key="$2"

  python3 - "$answers_file" "$key" <<'PY'
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
}

answers_set() {
  local answers_file="$1"
  local key="$2"
  local value="$3"

  python3 - "$answers_file" "$key" "$value" <<'PY'
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

validate_jig_mcp_smoke() {
  local repo_dir="$1"
  local expect_schema_dump="$2"
  local expect_sqlx="$3"

  REPO_DIR="$repo_dir" EXPECT_SCHEMA_DUMP="$expect_schema_dump" EXPECT_SQLX="$expect_sqlx" python3 <<'PY'
import json
import os
import pathlib
import subprocess
import sys

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
assert "jig.session_start" in tool_names, tool_names

send({
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {"name": "jig.session_start", "arguments": {}},
})
response = recv()
content = response["result"]["structuredContent"]
assert content["session_id"], response

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
PY

    local session_json
    local session_id
    local plan_json
    local plan_id
    local receipts_json
    local receipt_count
    local expected_receipt_count

    rm -rf .git/jig-tools .agent/.cache
    env -u JIG_DEV_BIN scripts/install-jig.sh >/dev/null
    validate_jig_mcp_smoke "$repo_dir" "$expect_schema_dump" "$expect_sqlx"

    session_json="$(scripts/jig session-start)"
    session_id="$(printf '%s' "$session_json" | json_get session_id)"

    plan_json="$(scripts/jig plans-open --title "Fixture runtime plan" --body "## Fixture\nRuntime validation.")"
    plan_id="$(printf '%s' "$plan_json" | json_get plan_id)"

    scripts/jig fmt-check --plan-id "$plan_id" >/dev/null
    scripts/jig contract-check --plan-id "$plan_id" >/dev/null
    scripts/jig test --plan-id "$plan_id" >/dev/null
    if [[ "$expect_sqlx" == "1" ]]; then
      scripts/jig schema-check --plan-id "$plan_id" >/dev/null
    fi

    if [[ "$expect_schema_dump" == "1" ]]; then
      scripts/jig schema-dump --plan-id "$plan_id" >/dev/null
    fi

    if [[ "$expect_sqlx" == "1" ]]; then
      scripts/jig migration-add "$migration_name" --plan-id "$plan_id" >/dev/null
    fi
    scripts/jig decisions-add \
      --title "Fixture decision" \
      --selected-option "Use jig" \
      --rationale "Runtime contract is wired and validated." \
      --plan-id "$plan_id" \
      --alternatives "Plain make" \
      >/dev/null

    receipts_json="$(scripts/jig receipts-list --plan-id "$plan_id" --limit 20)"
    receipt_count="$(printf '%s' "$receipts_json" | json_get receipts | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')"
    expected_receipt_count=3
    if [[ "$expect_sqlx" == "1" ]]; then
      expected_receipt_count=$((expected_receipt_count + 1))
    fi
    if [[ "$expect_schema_dump" == "1" ]]; then
      expected_receipt_count=$((expected_receipt_count + 1))
    fi
    if [[ "$receipt_count" -lt "$expected_receipt_count" ]]; then
      echo "Expected at least $expected_receipt_count receipts for plan $plan_id, found $receipt_count." >&2
      exit 1
    fi

    scripts/jig plans-close --plan-id "$plan_id" --resolution "fixture complete" >/dev/null
    scripts/jig session-end --session-id "$session_id" --outcome success >/dev/null

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

write_backend_stub_repo() {
  local repo_dir="$1"

  mkdir -p "$repo_dir/crates/demo/src"
  mkdir -p "$repo_dir/crates/acme-db/migrations"
  mkdir -p "$repo_dir/docs/schema"
  mkdir -p "$repo_dir/.sqlx"

  cat > "$repo_dir/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/demo"]
resolver = "2"
EOF

  cat > "$repo_dir/crates/demo/Cargo.toml" <<'EOF'
[package]
name = "demo"
version = "0.1.0"
edition = "2024"
EOF

  cat > "$repo_dir/crates/demo/src/lib.rs" <<'EOF'
pub fn meaning() -> i32 {
    42
}
EOF

  cat > "$repo_dir/crates/demo/AGENTS.md" <<'EOF'
# demo agent guide

## Purpose
Demo crate used to validate the extracted agent guide checks.

## Key entrypoints
- `src/lib.rs`: demo entrypoint.

## Edit here for X
- Change the demo function: `src/lib.rs`.

## Invariants
- Keep this crate small and deterministic.

## Common commands
- `cargo check -p demo`
EOF

  cat > "$repo_dir/crates/acme-db/AGENTS.md" <<'EOF'
# acme-db agent guide

## Purpose
Demo persistence crate used to validate crate-guide coverage.

## Key entrypoints
- `src/lib.rs`: demo DB entrypoint.

## Edit here for X
- Change DB helpers: `src/lib.rs`.

## Invariants
- Keep migration history forward-only.

## Common commands
- `cargo check -p acme-db`
EOF

  mkdir -p "$repo_dir/crates/acme-db/src"
  cat > "$repo_dir/crates/acme-db/src/lib.rs" <<'EOF'
pub fn marker() -> &'static str {
    "db"
}
EOF

  cat > "$repo_dir/crates/acme-db/migrations/20260405120000_init.up.sql" <<'EOF'
CREATE TABLE demo_items (id integer PRIMARY KEY);
EOF

  cat > "$repo_dir/crates/acme-db/migrations/20260405120000_init.down.sql" <<'EOF'
DROP TABLE demo_items;
EOF
}

write_full_stack_stub_repo() {
  local repo_dir="$1"

  write_backend_stub_repo "$repo_dir"

  mkdir -p "$repo_dir/frontend" "$repo_dir/admin-panel"
  cat > "$repo_dir/docs/schema/tables.sql" <<'EOF'
CREATE TABLE demo_items (id integer PRIMARY KEY);
EOF
  cat > "$repo_dir/scripts/dump-schema.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p docs/schema
cat > docs/schema/tables.sql <<'SQL'
CREATE TABLE demo_items (id integer PRIMARY KEY);
SQL
EOF
  chmod +x "$repo_dir/scripts/dump-schema.sh"
}

write_tooling_only_stub_repo() {
  local repo_dir="$1"

  mkdir -p "$repo_dir/crates/demo/src"

  cat > "$repo_dir/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/demo"]
resolver = "2"
EOF

  cat > "$repo_dir/crates/demo/Cargo.toml" <<'EOF'
[package]
name = "demo"
version = "0.1.0"
edition = "2024"
EOF

  cat > "$repo_dir/crates/demo/src/lib.rs" <<'EOF'
pub fn meaning() -> i32 {
    42
}
EOF

  cat > "$repo_dir/crates/demo/AGENTS.md" <<'EOF'
# demo agent guide

## Purpose
Demo crate used to validate the extracted agent guide checks.

## Key entrypoints
- `src/lib.rs`: demo entrypoint.

## Edit here for X
- Change the demo function: `src/lib.rs`.

## Invariants
- Keep this crate small and deterministic.

## Common commands
- `cargo check -p demo`
EOF
}

validate_backend_fixture() {
  local repo_dir="$1"

  write_backend_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .jig.yml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    scripts/generate-agent-map.sh
    git add .
    git commit -m "fixture" >/dev/null
    make help >/dev/null
    bash scripts/check-agent-map.sh
    bash scripts/check-agent-guides.sh
    bash scripts/check-rust-file-loc.sh --all >/dev/null
    bash scripts/check-migration-immutability.sh --changed-against HEAD
    bash scripts/check-sqlx-unchecked-non-test.sh >/dev/null
    coverage_dir="$(mktemp -d)"
    COVERAGE_DIR="$coverage_dir" COVERAGE_THRESHOLD=0 node scripts/enforce-coverage.js >/dev/null
    rm -rf "$coverage_dir"
    perl -0pi -e "s/default_branch: 'main'/default_branch: 'dev'/" .jig.yml
    git add .jig.yml
    git commit -m "change answers" >/dev/null
    uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml >/dev/null
    grep -q '^DEFAULT_BRANCH ?= dev$' Makefile
    grep -q '^JIG_VERSION ?= 0.1.0$' Makefile
    if [[ -f .github/workflows/webapp-checks.yml ]]; then
      rg -q "No web apps configured" .github/workflows/webapp-checks.yml
    fi
    validate_jig_runtime "$repo_dir" 0 1 "fixture_backend_runtime"
  )
}

validate_full_stack_fixture() {
  local repo_dir="$1"

  write_full_stack_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .jig.yml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    scripts/generate-agent-map.sh
    git add .
    git commit -m "fixture" >/dev/null
    make help >/dev/null
    bash scripts/check-agent-map.sh
    bash scripts/check-agent-guides.sh
    bash scripts/check-rust-file-loc.sh --all >/dev/null
    bash scripts/check-migration-immutability.sh --changed-against HEAD
    bash scripts/check-sqlx-unchecked-non-test.sh >/dev/null
    bash scripts/check-schema-dump.sh >/dev/null
    uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml >/dev/null
    rg -q "frontend" .github/workflows/webapp-checks.yml
    rg -q "admin-panel" .github/workflows/webapp-checks.yml
    rg -q "40" .github/workflows/webapp-checks.yml
    validate_jig_runtime "$repo_dir" 1 1 "fixture_full_stack_runtime"
  )
}

validate_tooling_only_fixture() {
  local repo_dir="$1"

  write_tooling_only_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .jig.yml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    scripts/generate-agent-map.sh
    git add .
    git commit -m "fixture" >/dev/null
    make help >/dev/null
    bash scripts/check-agent-map.sh
    bash scripts/check-agent-guides.sh
    bash scripts/check-rust-file-loc.sh --all >/dev/null
    coverage_dir="$(mktemp -d)"
    COVERAGE_DIR="$coverage_dir" COVERAGE_THRESHOLD=0 node scripts/enforce-coverage.js >/dev/null
    rm -rf "$coverage_dir"
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    ! rg -q '^sqlx-db-setup:' Makefile
    ! rg -q '^sqlx-check:' Makefile
    ! rg -q '^schema-check:' Makefile
    ! rg -q '^schema-dump:' Makefile
    ! rg -q '^migration-add:' Makefile
    ! rg -q '^check-sqlx-unchecked-non-test:' Makefile
    ! rg -q '"jig\\.sqlx_check"' .agent/jig-contract.json
    ! rg -q '"jig\\.schema_check"' .agent/jig-contract.json
    ! rg -q '"jig\\.schema_dump"' .agent/jig-contract.json
    ! rg -q '"jig\\.migration_add"' .agent/jig-contract.json
    ! rg -q 'sqlx-unchecked-queries:' .github/workflows/repo-policy.yml
    ! rg -q 'migration-immutability:' .github/workflows/repo-policy.yml
    perl -0pi -e "s/default_branch: 'main'/default_branch: 'dev'/" .jig.yml
    git add .jig.yml
    git commit -m "change answers" >/dev/null
    uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml >/dev/null
    grep -q '^DEFAULT_BRANCH ?= dev$' Makefile
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    validate_jig_runtime "$repo_dir" 0 0
  )
}

validate_unpushed_commit_stays_local() {
  local bare_remote="$TMP_DIR/template-remote.git"
  local template_snapshot="$TMP_DIR/template-snapshot"
  local template_clone="$TMP_DIR/template-clone"
  local answers_file="$TMP_DIR/template-backend.yaml"
  local rendered_dir="$TMP_DIR/rendered-from-clone"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare "$template_snapshot" "$bare_remote" >/dev/null 2>&1
  git clone "$bare_remote" "$template_clone" >/dev/null 2>&1
  git -C "$template_clone" config user.name "Fixture"
  git -C "$template_clone" config user.email "fixture@example.com"

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url ""

  cat > "$template_clone/UNPUSHED_MARKER.md" <<'EOF'
marker
EOF
  git -C "$template_clone" add UNPUSHED_MARKER.md
  git -C "$template_clone" commit -m "unpushed template change" >/dev/null

  render_fixture_from_template "$template_clone" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.yml" _src_path)"
  expected_src_path="$template_clone"
  if [[ "$actual_src_path" != "$expected_src_path" ]]; then
    echo "Expected _src_path to stay local for an unpushed commit." >&2
    echo "Expected: $expected_src_path" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

validate_invalid_template_source_url_fails() {
  local invalid_source="$TMP_DIR/does-not-exist.git"
  local answers_file="$TMP_DIR/backend-invalid-remote.yaml"
  local rendered_dir="$TMP_DIR/render-invalid-remote"
  local log_file="$TMP_DIR/render-invalid-remote.log"

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url "$invalid_source"

  if render_fixture "$answers_file" "$rendered_dir" >"$log_file" 2>&1; then
    echo "Expected invalid template_source_url to fail fixture rendering." >&2
    exit 1
  fi

  rg -q "template_source_url .* is not usable" "$log_file"
}

validate_explicit_template_source_url_requires_reachable_commit() {
  local bare_remote="$TMP_DIR/template-explicit-remote.git"
  local template_snapshot="$TMP_DIR/template-explicit-snapshot"
  local template_clone="$TMP_DIR/template-explicit-clone"
  local answers_file="$TMP_DIR/backend-explicit-remote.yaml"
  local rendered_dir="$TMP_DIR/render-explicit-remote"
  local log_file="$TMP_DIR/render-explicit-remote.log"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare "$template_snapshot" "$bare_remote" >/dev/null 2>&1
  git clone "$bare_remote" "$template_clone" >/dev/null 2>&1
  git -C "$template_clone" config user.name "Fixture"
  git -C "$template_clone" config user.email "fixture@example.com"

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url "$bare_remote"

  cat > "$template_clone/UNPUSHED_EXPLICIT_REMOTE.md" <<'EOF'
marker
EOF
  git -C "$template_clone" add UNPUSHED_EXPLICIT_REMOTE.md
  git -C "$template_clone" commit -m "unpushed template change for explicit remote" >/dev/null

  if render_fixture_from_template "$template_clone" "$answers_file" "$rendered_dir" >"$log_file" 2>&1; then
    echo "Expected explicit template_source_url to fail when _commit is not on the remote branch." >&2
    exit 1
  fi

  rg -q "_commit '.*' is not reachable from refs/heads/main" "$log_file"
}

validate_explicit_template_source_url_rewrites_src_path() {
  local bare_remote="$TMP_DIR/template-explicit-ok.git"
  local template_snapshot="$TMP_DIR/template-explicit-ok-snapshot"
  local answers_file="$TMP_DIR/backend-explicit-ok.yaml"
  local rendered_dir="$TMP_DIR/render-explicit-ok"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url "$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.yml" _src_path)"
  if [[ "$actual_src_path" != "$bare_remote" ]]; then
    echo "Expected explicit template_source_url to replace _src_path after validation." >&2
    echo "Expected: $bare_remote" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

validate_quoted_local_src_path_installs_jig() {
  local template_snapshot="$TMP_DIR/template-quoted-local'source"
  local answers_file="$TMP_DIR/backend-quoted-local.yaml"
  local rendered_dir="$TMP_DIR/render-quoted-local"

  create_template_snapshot_repo "$template_snapshot"

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url ""

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.yml" _src_path)"
  if [[ "$actual_src_path" != "$template_snapshot" ]]; then
    echo "Expected quoted local _src_path to round-trip through rendering." >&2
    echo "Expected: $template_snapshot" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi

  (
    cd "$rendered_dir"
    rm -rf .git/jig-tools .agent/.cache
    env -u JIG_DEV_BIN scripts/install-jig.sh >/dev/null
    [[ -x .agent/.cache/jig/0.1.0/bin/jig ]]
  )
}

validate_quoted_template_source_url_rewrites_src_path() {
  local bare_remote="$TMP_DIR/template-quoted-remote'.git"
  local template_snapshot="$TMP_DIR/template-quoted-remote-snapshot"
  local answers_file="$TMP_DIR/backend-quoted-remote.yaml"
  local rendered_dir="$TMP_DIR/render-quoted-remote"

  create_template_snapshot_repo "$template_snapshot"
  git clone --bare "$template_snapshot" "$bare_remote" >/dev/null 2>&1

  cp "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$answers_file"
  answers_set "$answers_file" template_source_url "$bare_remote"

  render_fixture_from_template "$template_snapshot" "$answers_file" "$rendered_dir"

  actual_src_path="$(answers_get "$rendered_dir/.jig.yml" _src_path)"
  if [[ "$actual_src_path" != "$bare_remote" ]]; then
    echo "Expected quoted template_source_url to replace _src_path after validation." >&2
    echo "Expected: $bare_remote" >&2
    echo "Actual:   $actual_src_path" >&2
    exit 1
  fi
}

BACKEND_DIR="$TMP_DIR/backend-only"
FULL_STACK_DIR="$TMP_DIR/full-stack"
TOOLING_ONLY_DIR="$TMP_DIR/tooling-only"

render_fixture "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$BACKEND_DIR"
render_fixture "$ROOT_DIR/tests/fixtures/full-stack.yaml" "$FULL_STACK_DIR"
render_fixture "$ROOT_DIR/tests/fixtures/tooling-only.yaml" "$TOOLING_ONLY_DIR"

validate_backend_fixture "$BACKEND_DIR"
validate_full_stack_fixture "$FULL_STACK_DIR"
validate_tooling_only_fixture "$TOOLING_ONLY_DIR"
validate_unpushed_commit_stays_local
validate_invalid_template_source_url_fails
validate_explicit_template_source_url_requires_reachable_commit
validate_explicit_template_source_url_rewrites_src_path
validate_quoted_local_src_path_installs_jig
validate_quoted_template_source_url_rewrites_src_path

echo "Fixture validation passed."
