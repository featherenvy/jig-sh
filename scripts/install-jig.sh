#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.yml"
JIG_YML="$ROOT_DIR/scripts/jig-yml.sh"

read_field() {
  "$JIG_YML" get "$ANSWERS_FILE" "$1"
}

JIG_VERSION="$(read_field jig_version)"
SRC_PATH="$(read_field _src_path)"
TEMPLATE_MODE="$(read_field _template_mode)"
TEMPLATE_LOCAL_PATH="$(read_field _template_local_path)"
TEMPLATE_SOURCE_URL="$(read_field template_source_url)"

if [[ -z "$JIG_VERSION" ]]; then
  echo "Failed to read jig_version from $ANSWERS_FILE." >&2
  exit 1
fi

if [[ -z "$SRC_PATH" ]]; then
  echo "Failed to read _src_path from $ANSWERS_FILE." >&2
  exit 1
fi

is_remote_source() {
  local source="$1"
  [[ "$source" == *"://"* || "$source" == git@*:* ]]
}

if [[ -d "$ROOT_DIR/.git" ]]; then
  DEFAULT_INSTALL_ROOT="$ROOT_DIR/.git/jig-tools/$JIG_VERSION"
else
  DEFAULT_INSTALL_ROOT="$ROOT_DIR/.agent/.cache/jig/$JIG_VERSION"
fi

INSTALL_ROOT="${1:-$DEFAULT_INSTALL_ROOT}"
BIN_PATH="$INSTALL_ROOT/bin/jig"

binary_version() {
  local bin_path="$1"
  "$bin_path" --version 2>/dev/null | awk '{print $2}'
}

assert_exact_version() {
  local bin_path="$1"
  local actual_version
  actual_version="$(binary_version "$bin_path" || true)"
  if [[ "$actual_version" != "$JIG_VERSION" ]]; then
    echo "Expected jig version $JIG_VERSION, found ${actual_version:-<missing>} at $bin_path." >&2
    return 1
  fi
}

install_from_dev_bin() {
  if [[ ! -x "${JIG_DEV_BIN:-}" ]]; then
    return 1
  fi

  mkdir -p "$(dirname "$BIN_PATH")"
  cp "$JIG_DEV_BIN" "$BIN_PATH"
  chmod +x "$BIN_PATH"
  assert_exact_version "$BIN_PATH"
}

install_from_local_source() {
  local source_root="$1"
  local crate_path="$source_root/crates/jig"
  if [[ ! -d "$crate_path" ]]; then
    echo "Expected local jig source at $crate_path." >&2
    return 1
  fi

  cargo install \
    --path "$crate_path" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force

  assert_exact_version "$BIN_PATH"
}

install_from_git_source() {
  cargo install \
    --git "$SRC_PATH" \
    --tag "v$JIG_VERSION" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    jig

  assert_exact_version "$BIN_PATH"
}

if install_from_dev_bin; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
  install_from_local_source "$SRC_PATH"
elif [[ "$TEMPLATE_MODE" == "working-tree" && -n "$TEMPLATE_LOCAL_PATH" && -d "$TEMPLATE_LOCAL_PATH/crates/jig" ]]; then
  install_from_local_source "$TEMPLATE_LOCAL_PATH"
elif [[ -n "$TEMPLATE_SOURCE_URL" ]]; then
  SRC_PATH="$TEMPLATE_SOURCE_URL"
  install_from_git_source
elif is_remote_source "$SRC_PATH"; then
  install_from_git_source
else
  echo "Cannot resolve jig source from _src_path='$SRC_PATH'." >&2
  if [[ "$TEMPLATE_MODE" == "working-tree" ]]; then
    echo "The working-tree template snapshot is unavailable and _template_local_path='$TEMPLATE_LOCAL_PATH' is not usable." >&2
  else
    echo "Re-render from an absolute template path or set template_source_url." >&2
  fi
  exit 1
fi

printf '%s\n' "$BIN_PATH"
