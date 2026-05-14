#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"
JIG_TOML="$ROOT_DIR/scripts/jig-toml.sh"

read_field() {
  "$JIG_TOML" get "$ANSWERS_FILE" "$1"
}

JIG_VERSION="$(read_field jig_version)"
SRC_PATH="$(read_field _src_path)"
TEMPLATE_COMMIT="$(read_field _commit)"
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
INSTALL_LOCK_DIR="$INSTALL_ROOT.lock"
INSTALL_LOCK_ATTEMPTS=30
STALE_INSTALL_LOCK_SECONDS=300

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
  local dev_bin
  dev_bin="$(resolve_executable_path "$JIG_DEV_BIN")" || {
    echo "Failed to resolve JIG_DEV_BIN: $JIG_DEV_BIN" >&2
    exit 1
  }
  if [[ ! -x "$dev_bin" ]]; then
    echo "JIG_DEV_BIN is set but is not executable: $dev_bin" >&2
    exit 1
  fi

  if ! assert_exact_version "$dev_bin"; then
    echo "JIG_DEV_BIN must match jig version $JIG_VERSION; refusing to install a fallback binary." >&2
    echo "Rebuild from the jig source checkout with: cargo build -p jig-sh --bin jig" >&2
    echo "Then set JIG_DEV_BIN=target/debug/jig, unset JIG_DEV_BIN, or run scripts/jig so the normal cached installer path can select a compatible runtime." >&2
    exit 1
  fi
  # scripts/jig captures stdout from this installer and execs the printed path.
  printf '%s\n' "$dev_bin"
}

resolve_executable_path() {
  local input="$1"
  if command -v realpath >/dev/null 2>&1; then
    realpath "$input"
    return
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$input" <<'PY'
import os
import sys

print(os.path.realpath(sys.argv[1]))
PY
    return
  fi

  local input_dir
  input_dir="$(cd "$(dirname "$input")" && pwd -P)" || return 1
  local resolved="$input_dir/$(basename "$input")"
  case "$resolved" in
    /*)
      printf '%s\n' "$resolved"
      ;;
    *)
      echo "Resolved executable path is not absolute: $resolved" >&2
      return 1
      ;;
  esac
}

acquire_install_lock() {
  mkdir -p "$(dirname "$INSTALL_ROOT")"
  local attempt
  attempt=1
  while [[ "$attempt" -le "$INSTALL_LOCK_ATTEMPTS" ]]; do
    if mkdir "$INSTALL_LOCK_DIR" 2>/dev/null; then
      trap release_install_lock EXIT
      return 0
    fi
    if install_lock_is_stale; then
      rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
      continue
    fi
    sleep 1
    attempt=$((attempt + 1))
  done
  echo "Timed out waiting for jig installer lock: $INSTALL_LOCK_DIR" >&2
  exit 1
}

install_lock_is_stale() {
  [[ -d "$INSTALL_LOCK_DIR" ]] || return 1
  local now mtime
  now="$(date +%s)"
  # macOS/BSD stat uses -f, GNU stat uses -c.
  if mtime="$(stat -f %m "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  elif mtime="$(stat -c %Y "$INSTALL_LOCK_DIR" 2>/dev/null)"; then
    :
  else
    return 1
  fi
  [[ $((now - mtime)) -gt $STALE_INSTALL_LOCK_SECONDS ]]
}

release_install_lock() {
  if [[ -d "$INSTALL_LOCK_DIR" ]]; then
    rmdir "$INSTALL_LOCK_DIR" 2>/dev/null || true
  fi
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
  local git_ref_args=(--tag "v$JIG_VERSION")
  if [[ "$TEMPLATE_COMMIT" =~ ^[0-9a-fA-F]{7,40}$ ]]; then
    # Adopted repos pin the exact template revision in .jig.toml. Treat that
    # commit as trusted repo configuration: a hex value intentionally overrides
    # the release tag so updates install the same source revision that rendered
    # the repo-local harness.
    git_ref_args=(--rev "$TEMPLATE_COMMIT")
  fi

  cargo install \
    --git "$SRC_PATH" \
    "${git_ref_args[@]}" \
    --root "$INSTALL_ROOT" \
    --locked \
    --force \
    jig-sh

  assert_exact_version "$BIN_PATH"
}

if [[ -n "${JIG_DEV_BIN:-}" ]]; then
  install_from_dev_bin
  exit 0
fi

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

acquire_install_lock

if [[ -x "$BIN_PATH" ]] && assert_exact_version "$BIN_PATH"; then
  printf '%s\n' "$BIN_PATH"
  exit 0
fi

if [[ -d "$SRC_PATH/crates/jig" ]] || [[ "$SRC_PATH" == /* && -d "$SRC_PATH" ]]; then
  install_from_local_source "$SRC_PATH"
elif [[ -n "$TEMPLATE_SOURCE_URL" ]]; then
  SRC_PATH="$TEMPLATE_SOURCE_URL"
  install_from_git_source
elif is_remote_source "$SRC_PATH"; then
  install_from_git_source
else
  echo "Cannot resolve jig source from _src_path='$SRC_PATH'." >&2
  echo "Re-render from an absolute committed template path or set template_source_url." >&2
  exit 1
fi

printf '%s\n' "$BIN_PATH"
