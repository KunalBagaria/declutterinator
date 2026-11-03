#!/bin/sh
# Build a universal (Apple silicon + Intel) release and stage it for the website.
#
#   scripts/release.sh             build + stage into site/public
#   scripts/release.sh --publish   ...then deploy the site and create the GitHub release
#
# Produces site/public/releases/v<version>/declutterinator-<version>-macos-universal.tar.gz
# (+ .sha256) and points site/public/releases/latest.txt at it.

set -eu

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
name="declutterinator-${version}-macos-universal"
out="site/public/releases/v${version}"
work="target/dist/${name}"

echo "building declutterinator ${version}"
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked --target "$target"
done

rm -rf "$work"
mkdir -p "$work" "$out"
lipo -create -output "$work/declutterinator" \
    target/aarch64-apple-darwin/release/declutterinator \
    target/x86_64-apple-darwin/release/declutterinator
# lipo keeps each slice's signature, but re-sign ad hoc so the fat binary verifies as a whole
codesign --force --sign - "$work/declutterinator"
codesign --verify "$work/declutterinator"
cp LICENSE README.md "$work/"

tar -C target/dist -czf "$out/${name}.tar.gz" "$name"
(cd "$out" && shasum -a 256 "${name}.tar.gz" > "${name}.tar.gz.sha256")
printf '%s\n' "$version" > site/public/releases/latest.txt
cp install.sh site/public/install.sh

lipo -info "$work/declutterinator"
ls -lh "$out"
cat "$out/${name}.tar.gz.sha256"

if [ "${1:-}" = "--publish" ]; then
    (cd site && wrangler deploy)
    gh release view "v${version}" >/dev/null 2>&1 ||
        gh release create "v${version}" --title "v${version}" --generate-notes \
            "$out/${name}.tar.gz" "$out/${name}.tar.gz.sha256"
fi
