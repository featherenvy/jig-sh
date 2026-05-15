#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT_DIR"

tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

list_agent_guides() {
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    {
      git ls-files -z -- '*AGENTS.md'
      git ls-files -z --others --exclude-standard -- '*AGENTS.md'
    } | xargs -0 -n1 printf '%s\n' | sort -u
    return
  fi

  find . -name AGENTS.md -not -path '*/.git/*' | sed 's#^\./##' | sort
}

{
  echo "# Agent Map"
  echo
  echo "Fast jump index for agent-facing guidance in this repository."
  echo
  echo "## Root guide"
  echo
  echo "- [Repository AGENTS.md](./AGENTS.md)"
  echo

  guide_paths=()
  while IFS= read -r guide_path; do
    guide_paths+=("$guide_path")
  done < <(list_agent_guides)
  nested_count=0
  for guide_path in "${guide_paths[@]}"; do
    if [[ "$guide_path" == "AGENTS.md" ]]; then
      continue
    fi
    if (( nested_count == 0 )); then
      echo "## Nested guides"
      echo
    fi
    nested_count=$((nested_count + 1))
    label="${guide_path%/AGENTS.md}"
    echo "- [${label}](./${guide_path})"
  done

  if (( nested_count == 0 )); then
    echo "## Nested guides"
    echo
    echo "_None yet_"
  fi

  echo
  echo "## Suggested usage pattern"
  echo
  echo "1. Start with the root [AGENTS.md](./AGENTS.md)."
  echo "2. Open the nearest guide for the area you will change."
  echo "3. Follow that guide's entrypoint map before editing."
} > "$tmp_file"

mv "$tmp_file" agent-map.md
echo "Wrote agent-map.md"
