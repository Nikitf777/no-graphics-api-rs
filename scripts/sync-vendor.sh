#!/usr/bin/env bash
# Replace vendor/NoGraphicsAPI with the sources from another NoGraphicsAPI checkout.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 /path/to/NoGraphicsAPI" >&2
    exit 1
fi

src=$(cd "$1" && pwd)
root=$(cd "$(dirname "$0")/.." && pwd)
dst="$root/crates/no-graphics-api-sys/vendor/NoGraphicsAPI"

for required in include/NoGraphicsAPI/NoGraphicsAPI.hpp src/NoGraphicsAPI.cpp src/BufferAddressMap.hpp LICENSE; do
    if [ ! -f "$src/$required" ]; then
        echo "missing $src/$required" >&2
        exit 1
    fi
done

rm -rf "$dst/include" "$dst/src"
mkdir -p "$dst/include" "$dst/src"
cp -r "$src/include/NoGraphicsAPI" "$dst/include/"
cp "$src/src/NoGraphicsAPI.cpp" "$src/src/BufferAddressMap.hpp" "$dst/src/"
if [ -f "$src/src/NoGraphicsAPIMetal.mm" ]; then
    cp "$src/src/NoGraphicsAPIMetal.mm" "$dst/src/"
fi
cp "$src/LICENSE" "$dst/LICENSE"

commit=$(git -C "$src" log -1 --format='%H (%ci, "%s")' 2>/dev/null || echo "unknown")
cat > "$dst/UPSTREAM.md" <<EOF
# Vendored upstream

Source: https://github.com/sebbbi/NoGraphicsAPI
Commit: $commit

Only \`include/\` and \`src/\` are kept, plus the upstream license files.
Re-sync with \`scripts/sync-vendor.sh [path-to-checkout]\`.
EOF

echo "synced $dst from $src @ $commit"
