#!/usr/bin/env bash
# Build the macOS installer:  build-pkg.sh VERSION BINARY [OUTDIR]
#
# Installs /usr/local/bin/vdf and /usr/local/share/man/man1/vdf.1.
#
# Optional signing and notarization, enabled by environment variables:
#   APP_SIGN_IDENTITY        "Developer ID Application: Name (TEAMID)", signs the binary
#   INSTALLER_SIGN_IDENTITY  "Developer ID Installer: Name (TEAMID)", signs the .pkg
#   NOTARY_APPLE_ID, NOTARY_TEAM_ID, NOTARY_PASSWORD (app-specific password)
#                            notarize the signed .pkg and staple the ticket
# Without them the package is unsigned. It installs fine from the command line
# (`sudo installer -pkg ...`), but Gatekeeper asks for confirmation when it is
# opened from a browser download.
set -euo pipefail

version=${1:?version}
bin=${2:?binary}
out=${3:-dist}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
ident=io.github.jggonz.vdf

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
payload=$work/root
mkdir -p "$payload/usr/local/bin" "$payload/usr/local/share/man/man1" "$work/resources" "$out"

install -m 755 "$bin" "$payload/usr/local/bin/vdf"
install -m 644 "$root/man/vdf.1" "$payload/usr/local/share/man/man1/vdf.1"

if [[ -n ${APP_SIGN_IDENTITY:-} ]]; then
  codesign --force --options runtime --timestamp --sign "$APP_SIGN_IDENTITY" "$payload/usr/local/bin/vdf"
fi

pkgbuild \
  --root "$payload" \
  --identifier "$ident" \
  --version "$version" \
  --install-location / \
  --ownership recommended \
  "$work/vdf-component.pkg"

cp "$root/LICENSE" "$work/resources/LICENSE.txt"
sed "s/@VERSION@/$version/g" "$here/resources/welcome.html" > "$work/resources/welcome.html"
cp "$here/resources/conclusion.html" "$work/resources/conclusion.html"
sed "s/@VERSION@/$version/g; s/@IDENT@/$ident/g" "$here/distribution.xml" > "$work/distribution.xml"

sign_args=()
if [[ -n ${INSTALLER_SIGN_IDENTITY:-} ]]; then
  sign_args=(--sign "$INSTALLER_SIGN_IDENTITY" --timestamp)
fi

pkg=$out/vdf-$version.pkg
productbuild \
  --distribution "$work/distribution.xml" \
  --resources "$work/resources" \
  --package-path "$work" \
  ${sign_args[@]+"${sign_args[@]}"} \
  "$pkg"

if [[ -n ${INSTALLER_SIGN_IDENTITY:-} && -n ${NOTARY_APPLE_ID:-} && -n ${NOTARY_TEAM_ID:-} && -n ${NOTARY_PASSWORD:-} ]]; then
  xcrun notarytool submit "$pkg" \
    --apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD" --wait
  xcrun stapler staple "$pkg"
fi

(cd "$out" && shasum -a 256 "$(basename "$pkg")" > "$(basename "$pkg").sha256")
echo "$pkg"
