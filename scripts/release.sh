#!/bin/sh
# Releases tiger: bumps the version, runs the gates, commits, tags, and pushes.
# Pushing the tag starts the release workflow, which builds the binaries,
# publishes the GitHub release, and updates the Homebrew formula.
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
[ -z "$(git tag --list "$tag")" ] || fail "tag $tag already exists locally"
[ -z "$(git ls-remote --tags origin "refs/tags/$tag")" ] || fail "tag $tag already exists on origin"

restore() {
  git checkout -- Cargo.toml Cargo.lock 2>/dev/null || true
  git tag -d "$tag" >/dev/null 2>&1 || true
}
trap 'restore; echo "release: stopped; version files restored, nothing pushed" >&2' EXIT

echo "releasing $tag (was $current)"
if [ "$version" != "$current" ]; then
  sed -i.bak "s/^version = \"$current\"$/version = \"$version\"/" Cargo.toml && rm Cargo.toml.bak
fi
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test

if [ -n "$(git status --porcelain -- Cargo.toml Cargo.lock)" ]; then
  git add Cargo.toml Cargo.lock
  git commit -m "🔖 release $tag"
fi
git tag -a "$tag" -m "tiger $tag"
trap - EXIT

# Commit and tag go together, or neither does.
git push --atomic --follow-tags || fail "the push failed; the commit and tag are local. Fix the cause, then run: git push --atomic --follow-tags"
echo "pushed $tag. Next:"
echo "  gh run watch                                   # follow the release workflow"
echo "  gh release edit $tag --notes-file __temp__/v$(echo "$version" | tr -d .)-changelog.md"
