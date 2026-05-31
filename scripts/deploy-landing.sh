#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/landing"

mode="${1:-production}"

bun install --frozen-lockfile

case "$mode" in
  production)
    bun run deploy
    ;;
  preview)
    bun run deploy:preview
    ;;
  *)
    echo "usage: scripts/deploy-landing.sh [production|preview]" >&2
    exit 2
    ;;
esac
