#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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

validate_backend_fixture() {
  local repo_dir="$1"

  write_backend_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .agentic-kit.yaml ]]
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
    perl -0pi -e "s/default_branch: 'main'/default_branch: 'dev'/" .agentic-kit.yaml
    git add .agentic-kit.yaml
    git commit -m "change answers" >/dev/null
    uvx --from copier copier recopy --trust --defaults --answers-file .agentic-kit.yaml >/dev/null
    grep -q '^DEFAULT_BRANCH ?= dev$' Makefile
    if [[ -f .github/workflows/webapp-checks.yml ]]; then
      rg -q "No web apps configured" .github/workflows/webapp-checks.yml
    fi
  )
}

validate_full_stack_fixture() {
  local repo_dir="$1"

  write_full_stack_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .agentic-kit.yaml ]]
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
    uvx --from copier copier update --trust --defaults --answers-file .agentic-kit.yaml >/dev/null
    rg -q "frontend" .github/workflows/webapp-checks.yml
    rg -q "admin-panel" .github/workflows/webapp-checks.yml
    rg -q "40" .github/workflows/webapp-checks.yml
  )
}

BACKEND_DIR="$TMP_DIR/backend-only"
FULL_STACK_DIR="$TMP_DIR/full-stack"

render_fixture "$ROOT_DIR/tests/fixtures/backend-only.yaml" "$BACKEND_DIR"
render_fixture "$ROOT_DIR/tests/fixtures/full-stack.yaml" "$FULL_STACK_DIR"

validate_backend_fixture "$BACKEND_DIR"
validate_full_stack_fixture "$FULL_STACK_DIR"

echo "Fixture validation passed."
