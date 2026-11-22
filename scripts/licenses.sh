#!/usr/bin/env bash
# Regenerates licenses/THIRD-PARTY-LICENSES.txt: every crate in Cargo.lock
# that a desktop, Windows, Android or browser build links (cargo-about, see
# about.toml), then SDL3's own licence. Needs cargo-about (--features cli).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
OUT=$REPO/licenses/THIRD-PARTY-LICENSES.txt
cd "$REPO"
cargo about generate -c about.toml licenses/about.hbs -o "$OUT"
SDL=$(cargo metadata --format-version 1 -q | python3 -I -c '
import json, sys
m = json.load(sys.stdin)
print(next(p["manifest_path"].rsplit("/", 1)[0] for p in m["packages"] if p["name"] == "sdl3-src"))')
{
    printf '\n--------------------------------------------------------------------------------\n'
    printf 'SDL3 (sdl3-src %s)\n' "$(basename "$SDL" | sed 's/^sdl3-src-//')"
    printf -- '--------------------------------------------------------------------------------\n\n'
    cat "$SDL/SDL/LICENSE.txt"
} >> "$OUT"
echo "$OUT"
