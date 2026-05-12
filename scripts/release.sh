#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PACKAGE_NAME="jig-sh"
BIN_NAME="jig"

print_usage() {
  cat <<'EOF'
Usage:
  scripts/release.sh check [VERSION]
  scripts/release.sh tag [VERSION]
  scripts/release.sh publish [VERSION]
  scripts/release.sh version

Commands:
  check     Run the full local release validation and cargo publish dry run.
  tag       Run release validation, then create annotated tag vVERSION.
  publish   Run release validation, ensure vVERSION is on origin at HEAD, then cargo publish.
  version   Print the jig-sh package version from Cargo metadata.

VERSION defaults to the package version from Cargo metadata.

Set ALLOW_DIRTY=1 only with `check` to validate working-tree release tooling before committing it.
EOF
}

usage() {
  print_usage >&2
  exit 2
}

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  "$@"
}

manifest_version() {
  cargo metadata --locked --format-version 1 --no-deps |
    python3 -c '
import json
import sys

package_name = sys.argv[1]
metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    if package["name"] == package_name:
        print(package["version"])
        break
else:
    raise SystemExit(f"Package {package_name!r} not found in Cargo metadata.")
' "$PACKAGE_NAME"
}

normalize_version() {
  local version="$1"
  version="${version#v}"
  if [[ -z "$version" ]]; then
    echo "Version must not be empty." >&2
    exit 2
  fi
  printf '%s\n' "$version"
}

release_version() {
  if [[ $# -gt 1 ]]; then
    usage
  fi
  if [[ $# -eq 1 ]]; then
    normalize_version "$1"
  else
    manifest_version
  fi
}

require_clean_tree() {
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 set; skipping clean working tree requirement." >&2
    return 0
  fi

  if [[ -n "$(git status --short --untracked-files=all)" ]]; then
    echo "Working tree is not clean. Commit or discard changes before releasing." >&2
    git status --short --untracked-files=all >&2
    exit 1
  fi
}

require_version_consistency() {
  local version="$1"
  local cargo_version
  local make_version
  local contract_version
  local launcher_version

  cargo_version="$(manifest_version)"
  if [[ "$cargo_version" != "$version" ]]; then
    echo "Cargo package version is $cargo_version, expected $version." >&2
    exit 1
  fi

  make_version="$(sed -n 's/^JIG_VERSION[[:space:]]*[:?]\{0,1\}=[[:space:]]*//p' "$ROOT_DIR/Makefile" | sed 's/[[:space:]]*#.*$//; s/[[:space:]]*$//')"
  if [[ -z "$make_version" ]]; then
    echo "Could not read JIG_VERSION from Makefile." >&2
    exit 1
  fi
  if [[ "$make_version" != "$version" ]]; then
    echo "Makefile JIG_VERSION is $make_version, expected $version." >&2
    exit 1
  fi

  contract_version="$(python3 - "$ROOT_DIR/.agent/jig-contract.json" <<'PY'
import json
import pathlib
import sys

print(json.loads(pathlib.Path(sys.argv[1]).read_text())["jig_version"])
PY
)"
  if [[ "$contract_version" != "$version" ]]; then
    echo ".agent/jig-contract.json jig_version is $contract_version, expected $version." >&2
    exit 1
  fi

  launcher_version="$(python3 - "$ROOT_DIR/scripts/jig" <<'PY'
import pathlib
import re
import sys

pattern = re.compile(r"""^JIG_VERSION\s*=\s*["']([^"']+)["']\s*$""")
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    match = pattern.match(line)
    if match:
        print(match.group(1))
        break
PY
)"
  if [[ -z "$launcher_version" ]]; then
    echo "Could not read JIG_VERSION from scripts/jig." >&2
    exit 1
  fi
  if [[ "$launcher_version" != "$version" ]]; then
    echo "scripts/jig JIG_VERSION is $launcher_version, expected $version." >&2
    exit 1
  fi

  local answer_files
  local fixture_files
  mapfile -t answer_files < <(git ls-files -- '.jig.yml' '**/.jig.yml')
  if [[ "${#answer_files[@]}" -eq 0 ]]; then
    echo "No tracked jig answer files found." >&2
    exit 1
  fi

  mapfile -t fixture_files < <(git ls-files -- 'tests/fixtures/*.yaml')
  if [[ "${#fixture_files[@]}" -eq 0 ]]; then
    echo "No tracked fixture answer files found." >&2
    exit 1
  fi

  local version_files=("${answer_files[@]}" "${fixture_files[@]}")

  local version_file
  for version_file in "${version_files[@]}"; do
    if ! git ls-files --error-unmatch "$version_file" >/dev/null 2>&1; then
      echo "Release-pinned jig answer file is not tracked: $version_file" >&2
      exit 1
    fi

    local file_version
    file_version="$("$ROOT_DIR/scripts/jig-yml.sh" get "$ROOT_DIR/$version_file" jig_version)"
    if [[ -z "$file_version" ]]; then
      echo "$version_file is missing jig_version." >&2
      exit 1
    fi
    if [[ "$file_version" != "$version" ]]; then
      echo "$version_file jig_version is $file_version, expected $version." >&2
      exit 1
    fi
  done
}

require_expected_binary_name() {
  local metadata_bin_names
  metadata_bin_names="$(
    cargo metadata --locked --format-version 1 --no-deps |
      python3 -c '
import json
import sys

package_name = sys.argv[1]
metadata = json.load(sys.stdin)
for package in metadata["packages"]:
    if package["name"] != package_name:
        continue
    names = sorted(
        target["name"]
        for target in package["targets"]
        if "bin" in target.get("kind", [])
    )
    print("\n".join(names))
    break
' "$PACKAGE_NAME"
  )"

  if ! grep -qx "$BIN_NAME" <<<"$metadata_bin_names"; then
    echo "Expected package $PACKAGE_NAME to expose binary $BIN_NAME." >&2
    echo "Found binaries:" >&2
    printf '%s\n' "$metadata_bin_names" >&2
    exit 1
  fi
}

cargo_dirty_flag=()
cargo_dirty_flags() {
  cargo_dirty_flag=()
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    cargo_dirty_flag=(--allow-dirty)
  fi
}

release_check() {
  local version="$1"
  cargo_dirty_flags

  require_clean_tree
  require_version_consistency "$version"
  require_expected_binary_name

  run make ci
  run bash scripts/validate-fixtures.sh
  run cargo publish -p "$PACKAGE_NAME" --locked --dry-run "${cargo_dirty_flag[@]}"

  echo "Release check passed for $PACKAGE_NAME v$version."
}

release_tag() {
  local version="$1"
  local tag="v$version"

  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 is only supported for release checks, not tagging." >&2
    exit 1
  fi

  if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    echo "Tag $tag already exists." >&2
    exit 1
  fi

  release_check "$version"

  run git tag -a "$tag" -m "$PACKAGE_NAME $tag"
  echo "Created tag $tag. release-publish will push it to origin before publishing."
}

remote_tag_commit() {
  local tag="$1"
  local output

  output="$(git ls-remote origin "refs/tags/$tag^{}" 2>/dev/null || true)"
  if [[ -z "$output" ]]; then
    output="$(git ls-remote origin "refs/tags/$tag" 2>/dev/null || true)"
  fi

  awk 'NR == 1 { print $1 }' <<<"$output"
}

ensure_remote_tag_at_head() {
  local tag="$1"
  local head_commit="$2"
  local remote_commit

  remote_commit="$(remote_tag_commit "$tag")"
  if [[ -z "$remote_commit" ]]; then
    run git push origin "refs/tags/$tag"
    remote_commit="$(remote_tag_commit "$tag")"
  fi

  if [[ -z "$remote_commit" ]]; then
    echo "Tag $tag is not present on origin after push." >&2
    exit 1
  fi

  if [[ "$remote_commit" != "$head_commit" ]]; then
    echo "Remote tag $tag points at $remote_commit, but HEAD is $head_commit." >&2
    exit 1
  fi
}

release_publish() {
  local version="$1"
  local tag="v$version"
  local tag_commit
  local head_commit

  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    echo "ALLOW_DIRTY=1 is only supported for release checks, not publishing." >&2
    exit 1
  fi

  if ! git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    echo "Missing release tag $tag." >&2
    exit 1
  fi

  tag_commit="$(git rev-list -n 1 "$tag")"
  head_commit="$(git rev-parse HEAD)"
  if [[ "$tag_commit" != "$head_commit" ]]; then
    echo "Tag $tag points at $tag_commit, but HEAD is $head_commit." >&2
    exit 1
  fi

  release_check "$version"
  ensure_remote_tag_at_head "$tag" "$head_commit"
  run cargo publish -p "$PACKAGE_NAME" --locked
}

if [[ $# -lt 1 ]]; then
  usage
fi

command="$1"
shift

cd "$ROOT_DIR"

case "$command" in
  check)
    version="$(release_version "$@")" || exit $?
    release_check "$version"
    ;;
  tag)
    version="$(release_version "$@")" || exit $?
    release_tag "$version"
    ;;
  publish)
    version="$(release_version "$@")" || exit $?
    release_publish "$version"
    ;;
  version)
    if [[ $# -ne 0 ]]; then
      usage
    fi
    manifest_version
    ;;
  -h|--help|help)
    print_usage
    ;;
  *)
    usage
    ;;
esac
