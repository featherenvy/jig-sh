#!/usr/bin/env bash

if ! declare -F render_fixture >/dev/null; then
  source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"
fi
if ! declare -F validate_jig_runtime >/dev/null; then
  source "$ROOT_DIR/scripts/fixtures/runtime-smoke.sh"
fi
if ! declare -F write_backend_stub_repo >/dev/null; then
  source "$ROOT_DIR/scripts/fixtures/stub-repos.sh"
fi

settle_fixture_cargo_workspace() {
  # Keep the first structured work check from being invalidated by Cargo settling the repo.
  cargo generate-lockfile >/dev/null
}

validate_backend_fixture() {
  local repo_dir="$1"

  write_backend_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    scripts/jig check migration-immutability --changed-against HEAD >/dev/null
    scripts/jig check sqlx-unchecked-non-test >/dev/null
    coverage_dir="$(mktemp -d)"
    COVERAGE_DIR="$coverage_dir" COVERAGE_THRESHOLD=0 node scripts/enforce-coverage.js >/dev/null
    rm -rf "$coverage_dir"
    perl -0pi -e 's/default_branch = "main"/default_branch = "dev"/' .jig.toml
    git add .jig.toml
    git commit -m "change answers" >/dev/null
    scripts/jig update --recopy --force >/dev/null
    [[ ! -f Makefile ]]
    grep -q '^default_branch = "dev"$' .jig.toml
    grep -q '^jig_version = "0.2.0-beta.1"$' .jig.toml
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
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    scripts/jig check migration-immutability --changed-against HEAD >/dev/null
    scripts/jig check sqlx-unchecked-non-test >/dev/null
    scripts/jig check schema >/dev/null
    scripts/jig update --recopy --force >/dev/null
    rg -q "frontend" .github/workflows/webapp-checks.yml
    rg -q "admin-panel" .github/workflows/webapp-checks.yml
    rg -q "40" .github/workflows/webapp-checks.yml
    validate_jig_runtime "$repo_dir" 1 1 "fixture_full_stack_runtime" 1
  )
}

validate_tooling_only_fixture() {
  local repo_dir="$1"

  write_tooling_only_stub_repo "$repo_dir"
  (
    cd "$repo_dir"
    [[ -f .jig.toml ]]
    git init -b main >/dev/null
    git config user.name "Fixture"
    git config user.email "fixture@example.com"
    settle_fixture_cargo_workspace
    scripts/jig agent-map generate >/dev/null
    git add .
    git commit -m "fixture" >/dev/null
    [[ ! -f Makefile ]]
    scripts/jig check agent-map >/dev/null
    scripts/jig check agent-guides >/dev/null
    scripts/jig check rust-file-loc --all >/dev/null
    coverage_dir="$(mktemp -d)"
    COVERAGE_DIR="$coverage_dir" COVERAGE_THRESHOLD=0 node scripts/enforce-coverage.js >/dev/null
    rm -rf "$coverage_dir"
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    [[ ! -f Makefile ]]
    ! rg -q '"jig\\.sqlx_check"' .agent/jig-contract.json
    ! rg -q '"jig\\.schema_check"' .agent/jig-contract.json
    ! rg -q '"jig\\.schema_dump"' .agent/jig-contract.json
    ! rg -q '"jig\\.migration_add"' .agent/jig-contract.json
    ! rg -q 'sqlx-unchecked-queries:' .github/workflows/repo-policy.yml
    ! rg -q 'migration-immutability:' .github/workflows/repo-policy.yml
    perl -0pi -e 's/default_branch = "main"/default_branch = "dev"/' .jig.toml
    git add .jig.toml
    git commit -m "change answers" >/dev/null
    scripts/jig update --recopy --force >/dev/null
    [[ ! -f Makefile ]]
    grep -q '^default_branch = "dev"$' .jig.toml
    [[ ! -f scripts/add-migration.sh ]]
    [[ ! -f scripts/check-migration-immutability.sh ]]
    [[ ! -f scripts/check-schema-dump.sh ]]
    [[ ! -f scripts/check-sqlx-unchecked-non-test.sh ]]
    [[ ! -f scripts/generate-sqlx-unchecked-queries-todo.sh ]]
    validate_jig_runtime "$repo_dir" 0 0
  )
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  fixture_create_tmp_dir_if_needed

  backend_dir="$TMP_DIR/backend-only"
  full_stack_dir="$TMP_DIR/full-stack"
  tooling_only_dir="$TMP_DIR/tooling-only"
  template_snapshot="$TMP_DIR/template-snapshot"

  create_template_snapshot_repo "$template_snapshot"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/backend-only.toml" "$backend_dir"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/full-stack.toml" "$full_stack_dir"
  render_fixture_from_template "$template_snapshot" "$ROOT_DIR/tests/fixtures/tooling-only.toml" "$tooling_only_dir"

  validate_backend_fixture "$backend_dir"
  validate_full_stack_fixture "$full_stack_dir"
  validate_tooling_only_fixture "$tooling_only_dir"

  echo "Rendered fixture validation passed."
fi
