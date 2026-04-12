#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/check-agent-guides.sh

Checks crate AGENTS.md quality gates:
  1) Every direct child crate directory under configured crate roots has AGENTS.md.
  2) Every crate guide has the required section headers.
  3) Every crate guide references src/lib.rs or src/main.rs.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

crate_roots=( "crates" )

required_sections=(
  "## Purpose"
  "## Key entrypoints"
  "## Edit here for X"
  "## Invariants"
  "## Common commands"
)

missing_guides=()
missing_sections=()
missing_entry_ref=()

for root in "${crate_roots[@]}"; do
  [[ -d "$root" ]] || continue
  while IFS= read -r crate_dir; do
    guide_path="${crate_dir}/AGENTS.md"
    if [[ ! -f "$guide_path" ]]; then
      missing_guides+=("$guide_path")
      continue
    fi

    for section in "${required_sections[@]}"; do
      if ! rg -q "^${section}$" "$guide_path"; then
        missing_sections+=("${guide_path}: missing section '${section}'")
      fi
    done

    if ! rg -q '`src/(lib|main)\.rs`' "$guide_path"; then
      missing_entry_ref+=("${guide_path}: missing src/lib.rs or src/main.rs entrypoint reference")
    fi
  done < <(find "$root" -mindepth 1 -maxdepth 1 -type d | sort)
done

had_error=0

if (( ${#missing_guides[@]} > 0 )); then
  had_error=1
  echo "Missing crate AGENTS.md files:"
  printf '  - %s\n' "${missing_guides[@]}"
fi

if (( ${#missing_sections[@]} > 0 )); then
  had_error=1
  echo "Missing required sections:"
  printf '  - %s\n' "${missing_sections[@]}"
fi

if (( ${#missing_entry_ref[@]} > 0 )); then
  had_error=1
  echo "Missing crate entrypoint references:"
  printf '  - %s\n' "${missing_entry_ref[@]}"
fi

if (( had_error > 0 )); then
  exit 1
fi

guide_count="$(find crates  -mindepth 2 -maxdepth 2 -name AGENTS.md 2>/dev/null | wc -l | tr -d ' ')"
echo "crate agent guide check passed:"
echo "  - ${guide_count} crate AGENTS.md files validated."
echo "  - required sections and entrypoint references are compliant."
