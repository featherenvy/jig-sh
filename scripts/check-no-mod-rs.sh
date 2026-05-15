#!/usr/bin/env bash
set -euo pipefail

violations=""
for root in "crates"; do
  matches="$(git ls-files -- "$root" 2>/dev/null | awk '/(^|\/)mod[.]rs$/' || true)"
  if [[ -n "$matches" ]]; then
    violations="$violations"$'\n'"$matches"
  fi
done

if [[ -n "$violations" ]]; then
  echo "Disallowed Rust module file(s) found. Use named module files instead of mod.rs." >&2
  printf '%s\n' "$violations" | sed '/^$/d' >&2
  exit 1
fi

echo "No disallowed mod.rs files found under configured crate roots."
