#!/bin/sh
# Prints one version's section of CHANGELOG.md: the release notes.
#   scripts/release-notes.sh 0.1.0
# Exits 1 when the section is missing or empty.
set -eu
cd "$(dirname "$0")/.."
version="$1"

notes=$(awk -v heading="## [$version]" '
  index($0, heading) == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on && /^\[[^]]+\]: / { next }
  on { print }
' CHANGELOG.md | sed -e '/./,$!d')

[ -n "$(printf '%s' "$notes" | tr -d '[:space:]')" ] || { echo "release-notes: CHANGELOG.md has no notes for $version" >&2; exit 1; }
printf '%s\n' "$notes"
