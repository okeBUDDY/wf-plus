#!/usr/bin/env bash
# Cross-builds the Windows release from Linux and zips it: wf+.exe (static
# CRT, stripped, with its icon, version resource and manifest), data.bin and
# the original readmes, all at the top of the zip.
#
# usage: scripts/build-windows.sh [out.zip]
# Default out.zip: <cargo target dir>/warning-forever-plus-<version>-windows-x64.zip
# Needs the rustup target x86_64-pc-windows-gnu, mingw-w64 (gcc, windres,
# strip), CMake (SDL3 is built from source) and zip. data.bin, readme.txt
# and readme_j.txt come from the folder of $WF_DATA (default
# data.bin in the current folder); the art from WF_ART_DIR, WF_FONT_DIR and
# WF_BITMAP_DIR (default: under $WF_ART_ROOT, the shared 1.08 folder).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
TARGET=x86_64-pc-windows-gnu
TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target}

DATA=${WF_DATA:-data.bin}
if [ -d "$DATA" ]; then ORIG=$DATA; else ORIG=$(dirname "$DATA"); fi
for f in data.bin readme.txt readme_j.txt; do
    [ -f "$ORIG/$f" ] || { echo "missing $ORIG/$f" >&2; exit 1; }
done

# build.rs defaults to ../software/warning-forever/1.08 beside the repo;
# name the shared folder so a clone elsewhere builds too.
ART_ROOT=${WF_ART_ROOT:-$REPO/../software/warning-forever/1.08}
export WF_ART_DIR=${WF_ART_DIR:-$ART_ROOT/sprites/vector}
export WF_FONT_DIR=${WF_FONT_DIR:-$ART_ROOT/data}
export WF_BITMAP_DIR=${WF_BITMAP_DIR:-$ART_ROOT/sprites/sheets}

VERSION=$(sed -n 's/^pub const VERSION_TEXT: &str = "\(.*\)";/\1/p' "$REPO/crates/wf/src/app_info.rs")
OUT=${1:-$TARGET_DIR/warning-forever-plus-$VERSION-windows-x64.zip}

# Static CRT: no libgcc or winpthread DLLs beside the exe.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C target-feature=+crt-static ${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS:-}"
(cd "$REPO" && cargo build --release -p wf --target "$TARGET")

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
x86_64-w64-mingw32-strip -o "$STAGE/wf+.exe" "$TARGET_DIR/$TARGET/release/wfplus.exe"
cp "$ORIG/data.bin" "$ORIG/readme.txt" "$ORIG/readme_j.txt" "$STAGE/"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT.tmp"
(cd "$STAGE" && zip -q -X -9 "$OUT.tmp" "wf+.exe" data.bin readme.txt readme_j.txt)
mv -f "$OUT.tmp" "$OUT"
echo "$OUT"
unzip -l "$OUT"
