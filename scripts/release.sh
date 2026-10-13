#!/bin/sh
# Releases tiger: bumps the version, stamps the changelog, runs the gates,
# commits, tags, and pushes. Pushing the tag starts the release workflow,
# which builds the binaries, publishes the GitHub release with the
# changelog's section as its notes, and updates the Homebrew formula.
#
#   scripts/release.sh patch|minor|major    bump, then release
#   scripts/release.sh 0.1.0                release this exact version
#
# Nothing is pushed unless every step before the push succeeds; a failure
# before then restores the version files and removes the local tag.
set -eu
cd "$(dirname "$0")/.."

fail() { echo "release: $1" >&2; exit 1; }

[ $# -eq 1 ] || fail "usage: scripts/release.sh patch|minor|major|<version>"
[ -z "$(git status --porcelain)" ] || fail "the tree isn't clean; commit or stash first"
branch=$(git symbolic-ref --quiet --short HEAD) || fail "not on a branch"
git rev-parse --abbrev-ref '@{upstream}' >/dev/null 2>&1 || fail "$branch has no upstream; push it once with: git push -u origin $branch"

current=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
major=${current%%.*}; rest=${current#*.}; minor=${rest%%.*}; patch=${rest#*.}
case "$1" in
  patch) version="$major.$minor.$((patch + 1))" ;;
  minor) version="$major.$((minor + 1)).0" ;;
  major) version="$((major + 1)).0.0" ;;
  [0-9]*.[0-9]*.[0-9]*) version="$1" ;;
  *) fail "expected patch, minor, major, or a version like 0.1.0" ;;
esac
tag="v$version"
repo_url="https://github.com/mareszhar/tiger"
[ -z "$(git tag --list "$tag")" ] || fail "tag $tag already exists locally"
[ -z "$(git ls-remote --tags origin "refs/tags/$tag")" ] || fail "tag $tag already exists on origin"

# The Unreleased section becomes this version's notes, so it can't be empty.
unreleased=$(awk '
  index($0, "## [Unreleased]") == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on && /^\[[^]]+\]: / { next }
  on { print }
' CHANGELOG.md | tr -d '[:space:]')
[ -n "$unreleased" ] || fail "CHANGELOG.md has nothing under ## [Unreleased]; describe what changed first"
previous=$(sed -n 's/^## \[\([0-9][0-9.]*\)\].*/\1/p' CHANGELOG.md | head -1)

restore() {
  git checkout -- Cargo.toml Cargo.lock CHANGELOG.md 2>/dev/null || true
  git tag -d "$tag" >/dev/null 2>&1 || true
}
trap 'restore; echo "release: stopped; version files restored, nothing pushed" >&2' EXIT

echo "releasing $tag (was $current)"
if [ "$version" != "$current" ]; then
  sed -i.bak "s/^version = \"$current\"$/version = \"$version\"/" Cargo.toml && rm Cargo.toml.bak
fi
# Stamp the changelog: Unreleased becomes this version, and its links follow.
if [ -n "$previous" ]; then
  version_link="[$version]: $repo_url/compare/v$previous...$tag"
else
  version_link="[$version]: $repo_url/releases/tag/$tag"
fi
awk -v version="$version" -v date="$(date +%Y-%m-%d)" -v unreleased_link="[unreleased]: $repo_url/compare/$tag...HEAD" -v version_link="$version_link" '
  index($0, "## [Unreleased]") == 1 { print; print ""; print "## [" version "] - " date; next }
  /^\[unreleased\]: / { print unreleased_link; print version_link; next }
  { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md
scripts/release-notes.sh "$version" >/dev/null

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "🔖 release $tag"
git tag -a "$tag" -m "tiger $tag"
trap - EXIT

# Commit and tag go together, or neither does.
git push --atomic --follow-tags || fail "the push failed; the commit and tag are local. Fix the cause, then run: git push --atomic --follow-tags"
echo "pushed $tag; the release workflow publishes it. Follow it with: gh run watch"
