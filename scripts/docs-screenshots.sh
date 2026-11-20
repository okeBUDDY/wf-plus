#!/usr/bin/env bash
# Remakes the README's screenshots in docs/ at 1280x720 (16:9), headless.
#
# usage: scripts/docs-screenshots.sh [data.bin] [recordings dir]
# Needs a release build (cargo build --release -p wf).
set -eu
DATA=${1:-${WF_DATA:-data.bin}}
REC_DIR=${2:-recordings}
WF=$(realpath "${WF_BIN:-${CARGO_TARGET_DIR:-target}/release/wfplus}")
DATA=$(realpath "$DATA")
OUT=$(realpath "$(dirname "$0")/../docs")
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# shot NAME FRAMES [args...]: a fresh home each time.
shot() {
    local name=$1 frames=$2; shift 2
    rm -rf "$WORK/home"; mkdir -p "$WORK/home/rec"
    (cd "$WORK/home" && "$WF" --data "$DATA" --home . --headless --frames "$frames" --screenshot "$OUT/$name.png" "$@" >/dev/null)
    echo "docs/$name.png"
}
# replay NAME REC FRAMES: a frame of a recording made with the original.
replay() {
    rm -rf "$WORK/home"; mkdir -p "$WORK/home/rec"
    cp "$2" "$WORK/home/rec/"
    (cd "$WORK/home" && "$WF" --data "$DATA" --home . --headless --size 1280x720 --frames "$3" \
        --replay "rec/$(basename "$2")" --screenshot "$OUT/$1.png" >/dev/null)
    echo "docs/$1.png"
}

# Start a game from the title (GAME START, then the arena), fire and circle.
play="60:z,320:z,360:z,400-8700:z"
for f in $(seq 400 90 8610); do
    play="$play,$f-$((f + 89)):$(printf 'left\nup\nright\ndown' | sed -n "$(( (f / 90) % 4 + 1 ))p")"
done
# The options: GAME START, down twice to OPTION, then E per tab.
options="60:z,320:down,340:down,380:z"

shot title 300 --size 1280x720 --script 60:z
shot game 1400 --size 1280x720 --arena classic --script "$play"
shot arena-border 1400 --size 1680x720 --arena classic --script "$play"
shot hd 2300 --size 1280x720 --arena hd --script "$play"
shot name-entry 9400 --size 1280x720 --arena hd --script "$play"
shot options 640 --size 1280x720 --script "$options"
shot options-display 640 --size 1280x720 --script "$options,520:e"
shot options-controls 640 --size 1280x720 --script "$options,520:e,540:e,560:e"
# The menus lay out to the whole window: a 16:10 one.
shot title-2000x1253 300 --size 2000x1253 --script 60:z
shot options-2000x1253 640 --size 2000x1253 --script "$options"
replay boss "$REC_DIR/01-normal-bot-stage8/stage07.rec" 600
replay explosions "$REC_DIR/01-normal-bot-stage8/stage07.rec" 1800
shot viewer-fonts 120 --size 1280x720 --viewer
