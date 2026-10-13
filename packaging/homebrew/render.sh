#!/bin/sh
# Renders the Homebrew formula for a release.
#   packaging/homebrew/render.sh <version> <dir with tiger-v<version>-<target>.tar.gz.sha256 files>
set -eu
version="$1"
dist="$2"
here="$(dirname "$0")"

sha() {
  file="$dist/tiger-v$version-$1.tar.gz.sha256"
  [ -f "$file" ] || { echo "missing $file" >&2; exit 1; }
  cut -d' ' -f1 "$file"
}

sed -e "s/@VERSION@/$version/g" \
  -e "s/@SHA_AARCH64_APPLE_DARWIN@/$(sha aarch64-apple-darwin)/" \
  -e "s/@SHA_X86_64_APPLE_DARWIN@/$(sha x86_64-apple-darwin)/" \
  -e "s/@SHA_AARCH64_UNKNOWN_LINUX_GNU@/$(sha aarch64-unknown-linux-gnu)/" \
  -e "s/@SHA_X86_64_UNKNOWN_LINUX_GNU@/$(sha x86_64-unknown-linux-gnu)/" \
  "$here/tiger.rb.tmpl"
