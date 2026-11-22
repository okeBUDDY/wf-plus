#!/usr/bin/env bash
# Builds the browser preview: the game as a wasm module (wgpu on WebGL2,
# see crates/wf/src/web.rs) and web/index.html, in a folder to serve over
# HTTP (browsers do not load wasm modules from file:// pages), e.g.
#   python3 -m http.server -d OUT_DIR
# Like the other releases it has none of the original game's files: the
# page asks the player for wf108.zip or data.bin.
#
# usage: scripts/build-web.sh [out dir]
# Default out dir: <cargo target dir>/web.
# Needs the rustup target wasm32-unknown-unknown, wasm-bindgen-cli at the
# version of the wasm-bindgen crate in Cargo.lock, and python3.
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target}
OUT=${1:-$TARGET_DIR/web}

want=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, ""); print }' "$REPO/Cargo.lock")
have=$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }')
if [ "$want" != "$have" ]; then
    echo "wasm-bindgen-cli $want is needed (have: ${have:-none}): cargo install wasm-bindgen-cli --version $want --locked" >&2
    exit 1
fi

cargo build --manifest-path "$REPO/Cargo.toml" --profile web --target wasm32-unknown-unknown -p wf --lib
rm -rf "$OUT"
mkdir -p "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT/pkg" "$TARGET_DIR/wasm32-unknown-unknown/web/wfplus.wasm"
# Without the function names (the name section, 0.8 MB): wasm-opt is not
# needed for that, and strip = true in the profile would drop the
# target_features section wasm-bindgen reads too.
python3 -I - "$OUT/pkg/wfplus_bg.wasm" <<'PY'
import sys
path = sys.argv[1]
b = open(path, "rb").read()
def leb(i):
    r = s = 0
    while True:
        x = b[i]; i += 1; r |= (x & 0x7f) << s; s += 7
        if x < 0x80:
            return r, i
out, i = [b[:8]], 8
while i < len(b):
    n, j = leb(i + 1)
    keep = True
    if b[i] == 0:
        l, k = leb(j)
        keep = b[k:k + l] not in (b"name", b"producers")
    if keep:
        out.append(b[i:j + n])
    i = j + n
open(path, "wb").write(b"".join(out))
PY
# The page shows the download's progress against the module's size.
size=$(stat -c %s "$OUT/pkg/wfplus_bg.wasm")
sed "s/__WASM_SIZE__/$size/" "$REPO/web/index.html" > "$OUT/index.html"
echo "built $OUT ($(du -sh "$OUT" | cut -f1))"
