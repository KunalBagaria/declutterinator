#!/bin/sh
# Render the social preview cards and PNG icons with headless Chrome.
#   site/og/render.sh   → site/public/{og.png, og-guide.png, apple-touch-icon.png, favicon-48.png, icon-192.png, icon-512.png}
set -eu
cd "$(dirname "$0")/.."
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
profile=$(mktemp -d)
port=8799
python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 &
server=$!
trap 'kill $server 2>/dev/null; rm -rf "$profile"' EXIT
sleep 1

shoot() { # url width height out — headless Chrome sometimes never exits, so cap each shot at 30s
    rm -f "$4"
    "$chrome" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$profile" \
        --use-mock-keychain --password-store=basic --no-first-run --force-device-scale-factor=1 \
        --window-size="$2,$3" --screenshot="$4" "$1" >/dev/null 2>&1 &
    pid=$!
    i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 60 ]; do sleep 0.5; i=$((i + 1)); done
    kill "$pid" 2>/dev/null || true
    pkill -f "$profile" 2>/dev/null || true
    [ -s "$4" ] || { echo "failed to render $1" >&2; exit 1; }
}

shoot "http://127.0.0.1:$port/og/card.html" 1200 630 public/og.png
shoot "http://127.0.0.1:$port/og/card.html?guide" 1200 630 public/og-guide.png
for size in 48 180 192 512; do
    tmp="$profile/icon-$size.png"
    shoot "http://127.0.0.1:$port/og/icon.html?size=$size" 640 640 "$tmp"
    sips -c "$size" "$size" "$tmp" --out "$tmp" >/dev/null   # center crop to the tile
    case $size in
        48) cp "$tmp" public/favicon-48.png ;;
        180) cp "$tmp" public/apple-touch-icon.png ;;
        *) cp "$tmp" "public/icon-$size.png" ;;
    esac
done
ls -l public/*.png
