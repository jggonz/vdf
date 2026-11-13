#!/usr/bin/env bash
# Print the Homebrew formula for a release:  generate-formula.sh VERSION SHA256SUMS
# SHA256SUMS is the checksum file attached to the GitHub release.
set -euo pipefail

version=${1:?version}
sums=${2:?SHA256SUMS file}
base=https://github.com/jggonz/vdf/releases/download/v$version

sha() {
  local s
  s=$(awk -v f="vdf-$version-$1.tar.gz" '$2 == f || $2 == "*"f {print $1}' "$sums")
  [[ -n $s ]] || { echo "missing checksum for vdf-$version-$1.tar.gz" >&2; exit 1; }
  echo "$s"
}

cat <<EOF
class Vdf < Formula
  desc "Visual disk free: df -h with colour-gradient bars, grouped by physical disk"
  homepage "https://github.com/jggonz/vdf"
  license "MIT"

  # One universal binary serves both Mac architectures.
  on_macos do
    on_arm do
      url "$base/vdf-$version-macos-universal.tar.gz"
      sha256 "$(sha macos-universal)"
    end
    on_intel do
      url "$base/vdf-$version-macos-universal.tar.gz"
      sha256 "$(sha macos-universal)"
    end
  end

  on_linux do
    on_intel do
      url "$base/vdf-$version-linux-x86_64.tar.gz"
      sha256 "$(sha linux-x86_64)"
    end
    on_arm do
      url "$base/vdf-$version-linux-aarch64.tar.gz"
      sha256 "$(sha linux-aarch64)"
    end
  end

  def install
    bin.install "vdf"
    man1.install "vdf.1"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/vdf --version")
    assert_match "Mounted on", shell_output("#{bin}/vdf --flat --color never /")
  end
end
EOF
