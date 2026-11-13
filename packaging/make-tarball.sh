#!/usr/bin/env bash
# Build a release tarball:  make-tarball.sh VERSION BINARY OUTDIR [PLATFORM]
# Produces OUTDIR/vdf-VERSION-PLATFORM.tar.gz and a matching .sha256 file.
set -euo pipefail

version=${1:?version}
bin=${2:?binary}
out=${3:-dist}
platform=${4:-}
root=$(cd "$(dirname "$0")/.." && pwd)

if [[ -z $platform ]]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-*)       platform=macos-$(uname -m) ;;
    Linux-x86_64)   platform=linux-x86_64 ;;
    Linux-aarch64|Linux-arm64) platform=linux-aarch64 ;;
    *)              platform=$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m) ;;
  esac
fi

name=vdf-$version-$platform
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/$name" "$out"
install -m 755 "$bin" "$work/$name/vdf"
install -m 644 "$root/man/vdf.1" "$root/LICENSE" "$root/README.md" "$root/CHANGELOG.md" "$work/$name/"

tar -C "$work" -czf "$out/$name.tar.gz" "$name"

cd "$out"
if command -v sha256sum >/dev/null; then
  sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
else
  shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256"
fi
echo "$out/$name.tar.gz"
