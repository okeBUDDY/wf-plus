#!/usr/bin/env bash
# Builds the Linux x86_64 release and packs it as a .tar.gz: a folder with
# wfplus (stripped; SDL3 linked in statically) and the third-party licence
# notices. As with the Windows release, none of the original game's files
# are included: the player gets wf108.zip from the official website and
# the game finds it (beside wfplus, in the working directory or in
# ~/Downloads), or asks for it on first run. --with-data adds data.bin and
# the original readmes, for a build of your own.
#
# The binary needs the glibc it was built against or newer, and loads the
# X11, Wayland, PulseAudio, ALSA, PipeWire and libdecor libraries at run
# time if they are there. The release is built on Ubuntu 22.04 (glibc
# 2.35), which covers SteamOS 3 and current desktop distributions; the
# script prints the glibc version the result needs.
#
# usage: scripts/build-linux.sh [--with-data] [out.tar.gz]
# Default out.tar.gz: <cargo target dir>/warning-forever-plus-<version>-linux-x64.tar.gz
# (-with-data.tar.gz with --with-data).
# Needs a C compiler, CMake and pkg-config (SDL3 is built from source) and
# the development headers of the SDL3 back ends to build in: X11 (libx11,
# libxext, libxrandr, libxcursor, libxfixes, libxi, libxss, libxtst),
# Wayland (libwayland, wayland-protocols, libxkbcommon, libegl, libdecor),
# PulseAudio, ALSA, PipeWire, udev and D-Bus. data.bin, readme.txt and
# readme_j.txt come from the folder of $WF_DATA (default data.bin in the
# current folder).
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
TARGET=x86_64-unknown-linux-gnu
TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target}

WITH_DATA=0
if [ "${1:-}" = --with-data ]; then WITH_DATA=1; shift; fi
ORIGINAL_FILES=()
if [ $WITH_DATA = 1 ]; then
    DATA=${WF_DATA:-data.bin}
    if [ -d "$DATA" ]; then ORIG=$DATA; else ORIG=$(dirname "$DATA"); fi
    for f in data.bin readme.txt readme_j.txt; do
        [ -f "$ORIG/$f" ] || { echo "missing $ORIG/$f" >&2; exit 1; }
        ORIGINAL_FILES+=("$f")
    done
fi

VERSION=$(sed -n 's/^pub const VERSION_TEXT: &str = "\(.*\)";/\1/p' "$REPO/crates/wf/src/app_info.rs")
SUFFIX=$([ $WITH_DATA = 1 ] && echo -with-data || true)
NAME=warning-forever-plus-$VERSION-linux-x64
OUT=${1:-$TARGET_DIR/$NAME$SUFFIX.tar.gz}

(cd "$REPO" && cargo build --release -p wf --target "$TARGET")
BIN=$TARGET_DIR/$TARGET/release/wfplus

# SDL3 picks its back ends from the headers it finds; a build without the
# video ones would start with no window on the player's machine.
# The newest build of it, if cargo kept older ones.
CONFIG=$(ls -t $(find "$TARGET_DIR/$TARGET/release/build" -path '*sdl3-sys-*' -name SDL_build_config.h) | head -1)
for d in VIDEO_DRIVER_X11 VIDEO_DRIVER_WAYLAND AUDIO_DRIVER_PULSEAUDIO; do
    grep -q "^#define SDL_$d 1" "$CONFIG" || { echo "SDL3 was built without $d: install its development headers" >&2; exit 1; }
done

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir "$STAGE/$NAME"
strip -o "$STAGE/$NAME/wfplus" "$BIN"
chmod 755 "$STAGE/$NAME/wfplus"
for f in "${ORIGINAL_FILES[@]}"; do cp "$ORIG/$f" "$STAGE/$NAME/"; done
cp "$REPO/licenses/THIRD-PARTY-LICENSES.txt" "$STAGE/$NAME/"

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT.tmp"
tar -C "$STAGE" --owner=0 --group=0 --numeric-owner -czf "$OUT.tmp" "$NAME"
mv -f "$OUT.tmp" "$OUT"
echo "$OUT"
tar -tvzf "$OUT"
GLIBC=$(objdump -T "$STAGE/$NAME/wfplus" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)
echo "needs ${GLIBC/_/ } or newer"
