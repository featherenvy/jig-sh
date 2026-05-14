#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PACKAGE_NAME="jig-sh"
# Publish the split support crate first; jig-sh depends on this exact version
# from crates.io once package verification leaves path dependencies behind.
PUBLISH_PACKAGE_NAMES=("jig-dev-proxy" "$PACKAGE_NAME")
BIN_NAME="jig"
RELEASE_FIXTURE_FILES=(
  tests/fixtures/backend-only.toml
  tests/fixtures/full-stack.toml
  tests/fixtures/tooling-only.toml
)

print_usage() {
  cat <<'EOF'
Usage:
  scripts/release.sh check [VERSION]
  scripts/release.sh prepare [VERSION]
  scripts/release.sh notes [VERSION]
  scripts/release.sh stage
  scripts/release.sh tag [VERSION]
  scripts/release.sh publish [VERSION]
  scripts/release.sh github [VERSION]
  scripts/release.sh next-version [major|minor|patch]
  scripts/release.sh version

Commands:
  check     Run the full local release validation and cargo publish dry run.
  prepare   Update pinned versions and CHANGELOG.md for VERSION.
  notes     Generate or replace the CHANGELOG.md section for VERSION.
  stage     Stage files updated by release-prepare.
  tag       Run release validation, then create annotated tag vVERSION.
  publish   Run release validation, cargo publish all crates, then push vVERSION to origin.
  github    Create the GitHub Release for vVERSION from CHANGELOG.md.
  next-version
            Print the next semantic version using the requested bump. Defaults to patch.
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
  local package_name="${1:-$PACKAGE_NAME}"
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
' "$package_name"
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

next_version() {
  local bump="${1:-patch}"
  if [[ $# -gt 1 ]]; then
    usage
  fi

  python3 - "$(manifest_version)" "$bump" <<'PY'
import re
import sys

version = sys.argv[1]
bump = sys.argv[2]
match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
if not match:
    raise SystemExit(f"Cannot bump non-semver version: {version}")

major, minor, patch = (int(part) for part in match.groups())
if bump == "major":
    major += 1
    minor = 0
    patch = 0
elif bump == "minor":
    minor += 1
    patch = 0
elif bump == "patch":
    patch += 1
else:
    raise SystemExit("Bump must be major, minor, or patch.")

print(f"{major}.{minor}.{patch}")
PY
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

  local package_name
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    cargo_version="$(manifest_version "$package_name")"
    if [[ "$cargo_version" != "$version" ]]; then
      echo "Cargo package $package_name version is $cargo_version, expected $version." >&2
      exit 1
    fi
  done

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
  mapfile -t answer_files < <(git ls-files | awk '$0 ~ /(^|\/)\.jig\.toml$/ { print }')
  if [[ "${#answer_files[@]}" -eq 0 ]]; then
    echo "No tracked jig answer files found." >&2
    exit 1
  fi

  mapfile -t fixture_files < <(git ls-files -- "${RELEASE_FIXTURE_FILES[@]}")
  if [[ "${#fixture_files[@]}" -eq 0 ]]; then
    echo "No tracked fixture answer files found." >&2
    exit 1
  fi
  for fixture_file in "${fixture_files[@]}"; do
    if ! grep -Eq '^jig_version\s*=' "$ROOT_DIR/$fixture_file"; then
      echo "$fixture_file is a release-pinned fixture and must include jig_version." >&2
      exit 1
    fi
  done

  local version_files=("${answer_files[@]}" "${fixture_files[@]}")

  local version_file
  for version_file in "${version_files[@]}"; do
    if ! git ls-files --error-unmatch "$version_file" >/dev/null 2>&1; then
      echo "Release-pinned jig answer file is not tracked: $version_file" >&2
      exit 1
    fi

    local file_version
    file_version="$("$ROOT_DIR/scripts/jig-toml.sh" get "$ROOT_DIR/$version_file" jig_version)"
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

require_changelog_entry() {
  local version="$1"

  if [[ ! -f "$ROOT_DIR/CHANGELOG.md" ]]; then
    echo "CHANGELOG.md is missing. Run scripts/release.sh notes $version." >&2
    exit 1
  fi

  if ! "$ROOT_DIR/scripts/release-notes.sh" print "$version" >/dev/null; then
    echo "CHANGELOG.md is missing release notes for v$version." >&2
    echo "Run scripts/release.sh notes $version before release validation." >&2
    exit 1
  fi
}

update_version_files() {
  local version="$1"

  python3 - "$ROOT_DIR" "$version" "${RELEASE_FIXTURE_FILES[@]}" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1])
version = sys.argv[2]
release_fixture_files = sys.argv[3:]
if not re.fullmatch(r"\d+\.\d+\.\d+", version):
    raise SystemExit(f"Version must be MAJOR.MINOR.PATCH, got {version!r}.")

def replace_required(path, pattern, replacement, label=None, flags=0):
    text = path.read_text()
    next_text, count = re.subn(pattern, replacement, text, flags=flags)
    if count == 0:
        raise SystemExit(f"Could not update {label or pattern!r} in {path}.")
    path.write_text(next_text)

def replace_exactly_once(path, pattern, replacement, label=None, flags=0):
    text = path.read_text()
    next_text, count = re.subn(pattern, replacement, text, flags=flags)
    if count != 1:
        raise SystemExit(f"Expected to update {label or pattern!r} exactly once in {path}; updated {count}.")
    path.write_text(next_text)

def replace_optional(path, pattern, replacement, flags=0):
    if path.exists():
        replace_required(path, pattern, replacement, flags=flags)

def update_jig_toml(path):
    replace_required(
        path,
        r'(?m)^jig_version\s*=\s*"[^"]*"\s*$',
        f'jig_version = "{version}"',
        "jig_version",
    )

def has_jig_version(path):
    return bool(re.search(r'(?m)^jig_version\s*=', path.read_text()))

def git_ls_files(*patterns):
    return subprocess.check_output(
        ["git", "-C", str(root), "ls-files", "--", *patterns],
        text=True,
    ).splitlines()

replace_exactly_once(
    root / "Cargo.toml",
    r'(?ms)(^\[workspace\.package\]\n(?:(?!^\[).)*?^version\s*=\s*)"[^"]*"',
    rf'\g<1>"{version}"',
    "workspace package version",
)
for package in ("jig-dev-proxy", "jig-sh"):
    replace_exactly_once(
        root / "Cargo.lock",
        rf'(?ms)(\[\[package\]\]\nname = "{re.escape(package)}"\nversion = )"[^"]*"',
        rf'\g<1>"{version}"',
        f"Cargo.lock {package} package version",
    )
replace_exactly_once(
    root / "Makefile",
    r'(?m)^JIG_VERSION\s*\?=\s*[^\n]+$',
    f"JIG_VERSION ?= {version}",
    "JIG_VERSION",
)
replace_exactly_once(
    root / "scripts" / "jig",
    r'(?m)^JIG_VERSION="[^"]*"$',
    f'JIG_VERSION="{version}"',
    "launcher JIG_VERSION",
)

contract_path = root / ".agent" / "jig-contract.json"
contract = json.loads(contract_path.read_text())
contract["jig_version"] = version
contract_path.write_text(json.dumps(contract, indent=2) + "\n")

jig_toml_paths = set()
for relative_path in git_ls_files():
    if pathlib.Path(relative_path).name != ".jig.toml":
        continue
    path = root / relative_path
    jig_toml_paths.add(path)
for relative_path in git_ls_files(*release_fixture_files):
    path = root / relative_path
    if not has_jig_version(path):
        raise SystemExit(f"{path.relative_to(root)} is a release-pinned fixture and must include jig_version.")
    jig_toml_paths.add(path)
if not jig_toml_paths:
    raise SystemExit("No .jig.toml or fixture TOML files found to update.")
for path in sorted(jig_toml_paths):
    update_jig_toml(path)

replace_optional(
    root / "scripts" / "fixtures" / "rendered-repos.sh",
    # This file stores the expected grep pattern literally, including ^ and $.
    r"\^JIG_VERSION \?= \d+\.\d+\.\d+\$",
    f"^JIG_VERSION ?= {version}$",
)
replace_optional(
    root / "scripts" / "fixtures" / "runtime-smoke.sh",
    r"\.git/jig-tools/\d+\.\d+\.\d+/bin/jig",
    f".git/jig-tools/{version}/bin/jig",
)
PY
}

release_notes() {
  local version="$1"
  run "$ROOT_DIR/scripts/release-notes.sh" update "$version"
}

release_stage() {
  local release_path

  for release_path in \
    Cargo.toml Cargo.lock Makefile scripts/jig .agent/jig-contract.json CHANGELOG.md \
    scripts/fixtures/rendered-repos.sh scripts/fixtures/runtime-smoke.sh
  do
    if [[ -e "$ROOT_DIR/$release_path" ]]; then
      run git add "$release_path"
    fi
  done

  git ls-files -z |
    while IFS= read -r -d '' release_path; do
      case "$release_path" in
        .jig.toml|*/.jig.toml)
          run git add "$release_path"
          ;;
      esac
    done

  git ls-files -z -- "${RELEASE_FIXTURE_FILES[@]}" |
    while IFS= read -r -d '' release_path; do
      run git add "$release_path"
    done
}

require_changelog_update_allowed() {
  local version="$1"

  # Check before rewriting version files so an existing generated section fails
  # without leaving a partially prepared release in the worktree.
  if [[ "${RELEASE_NOTES_FORCE:-}" == "1" ]]; then
    return 0
  fi
  if "$ROOT_DIR/scripts/release-notes.sh" print "$version" >/dev/null 2>&1; then
    echo "CHANGELOG.md already has a v$version section. Set RELEASE_NOTES_FORCE=1 to replace it." >&2
    exit 1
  fi
}

release_prepare() {
  local version="$1"

  require_changelog_update_allowed "$version"
  update_version_files "$version"
  run cargo metadata --format-version 1 --no-deps >/dev/null
  release_notes "$version"

  echo "Prepared release files for $PACKAGE_NAME v$version."
}

release_github() {
  local version="$1"
  local tag="v$version"
  local notes_file

  if ! command -v gh >/dev/null 2>&1; then
    echo "GitHub CLI 'gh' is required to create GitHub releases." >&2
    exit 1
  fi

  if gh release view "$tag" >/dev/null 2>&1; then
    echo "GitHub Release $tag already exists."
    return 0
  fi

  notes_file="$(mktemp)"
  if ! "$ROOT_DIR/scripts/release-notes.sh" print "$version" >"$notes_file"; then
    rm -f "$notes_file"
    return 1
  fi
  if ! run gh release create "$tag" --verify-tag --title "$tag" --notes-file "$notes_file"; then
    rm -f "$notes_file"
    return 1
  fi
  rm -f "$notes_file"
}

cargo_dirty_flag=()
cargo_dirty_flags() {
  cargo_dirty_flag=()
  if [[ "${ALLOW_DIRTY:-}" == "1" ]]; then
    cargo_dirty_flag=(--allow-dirty)
  fi
}

crate_version_status() {
  local package_name="$1"
  local version="$2"
  local status
  status="$(curl --max-time 20 -sS -o /dev/null -w '%{http_code}' "https://crates.io/api/v1/crates/$package_name/$version" || true)"
  printf '%s\n' "${status:-000}"
}

wait_for_crate_version() {
  local package_name="$1"
  local version="$2"
  local attempt
  local status

  for attempt in {1..60}; do
    status="$(crate_version_status "$package_name" "$version")"
    if [[ "$status" == "200" ]]; then
      return 0
    fi
    if [[ "$status" != "404" && "$status" != "000" ]]; then
      echo "crates.io version probe for $package_name v$version returned HTTP $status." >&2
    fi
    sleep 10
  done

  echo "Timed out waiting for $package_name v$version to appear on crates.io." >&2
  exit 1
}

publish_package_if_missing() {
  local package_name="$1"
  local version="$2"
  local status

  status="$(crate_version_status "$package_name" "$version")"
  case "$status" in
    200)
      echo "$package_name v$version is already published; skipping cargo publish."
      ;;
    404)
      if [[ "$package_name" == "$PACKAGE_NAME" ]]; then
        wait_for_crate_version "jig-dev-proxy" "$version"
        echo "If publishing $PACKAGE_NAME fails after jig-dev-proxy v$version is published, bump the patch version and rerun the release." >&2
      fi
      run cargo publish -p "$package_name" --locked
      wait_for_crate_version "$package_name" "$version"
      ;;
    *)
      echo "Could not determine whether $package_name v$version is already published; crates.io returned HTTP $status." >&2
      exit 1
      ;;
  esac
}

release_check() {
  local version="$1"
  local package_name
  local dependency_status
  cargo_dirty_flags

  require_clean_tree
  require_version_consistency "$version"
  require_expected_binary_name
  require_changelog_entry "$version"

  run make ci
  run bash scripts/validate-fixtures.sh
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    if [[ "$package_name" == "$PACKAGE_NAME" ]]; then
      dependency_status="$(crate_version_status "jig-dev-proxy" "$version")"
      case "$dependency_status" in
        200)
          ;;
        404)
          echo "jig-dev-proxy v$version is not on crates.io yet; dry-running $PACKAGE_NAME with a local registry patch before any real crate publish."
          # This fallback is only for the first publish of a split crate version.
          # Once jig-dev-proxy v$version is visible in the registry, the normal
          # dry-run path above exercises crates.io dependency resolution.
          run cargo publish -p "$package_name" --locked --dry-run "${cargo_dirty_flag[@]}" --config "patch.crates-io.jig-dev-proxy.path=\"$ROOT_DIR/crates/jig-dev-proxy\""
          continue
          ;;
        *)
          echo "Could not determine whether jig-dev-proxy v$version is already published; crates.io returned HTTP $dependency_status." >&2
          exit 1
          ;;
      esac
    fi
    run cargo publish -p "$package_name" --locked --dry-run "${cargo_dirty_flag[@]}"
  done

  echo "Release check passed for workspace crates v$version."
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
  echo "Created tag $tag. release-publish will push it to origin after all crates publish successfully."
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
  local package_name
  for package_name in "${PUBLISH_PACKAGE_NAMES[@]}"; do
    publish_package_if_missing "$package_name" "$version"
  done
  ensure_remote_tag_at_head "$tag" "$head_commit"
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
  prepare)
    version="$(release_version "$@")" || exit $?
    release_prepare "$version"
    ;;
  notes)
    version="$(release_version "$@")" || exit $?
    release_notes "$version"
    ;;
  stage)
    if [[ $# -ne 0 ]]; then
      usage
    fi
    release_stage
    ;;
  tag)
    version="$(release_version "$@")" || exit $?
    release_tag "$version"
    ;;
  publish)
    version="$(release_version "$@")" || exit $?
    release_publish "$version"
    ;;
  github)
    version="$(release_version "$@")" || exit $?
    release_github "$version"
    ;;
  next-version)
    next_version "$@"
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
