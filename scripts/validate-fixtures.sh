#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

source "$ROOT_DIR/scripts/fixtures/lib.sh"
source "$ROOT_DIR/scripts/fixtures/runtime-smoke.sh"
source "$ROOT_DIR/scripts/fixtures/stub-repos.sh"
source "$ROOT_DIR/scripts/fixtures/rendered-repos.sh"
source "$ROOT_DIR/scripts/fixtures/source-normalization.sh"

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
