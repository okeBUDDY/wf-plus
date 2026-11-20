#!/usr/bin/env bash
# Headless playtest: plays back every stageNN.rec of every recording made with
# the original game and checks that each stage ends on the score the next
# stage's recording starts with, and that the last one ends on the best score
# in the recording's wf.sco (or the recording's last.rec header when the run
# was not entered).
#
# usage: scripts/playtest.sh [recordings dir] [data.bin]
# Needs a release build (cargo build --release -p wf) and python3.
set -u
REC_DIR=${1:-recordings}
DATA=${2:-${WF_DATA:-data.bin}}
WF=${WF_BIN:-${CARGO_TARGET_DIR:-target}/release/wf}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

# The score field of a .rec header (version, seed, stage, score as i32 LE).
header_score() { python3 -I -c 'import struct,sys; print(struct.unpack_from("<i", open(sys.argv[1],"rb").read(), 12)[0])' "$1"; }

# The best score in wf.sco for the rules of a .rec (rules: 4 u16 at 24 in the
# .rec; wf.sco: 12-byte header, then 0x88-byte records of rules and 8 entries
# of 16 bytes, the score first).
best_score() { python3 -I -c '
import struct, sys
sco = open(sys.argv[1], "rb").read(); rules = open(sys.argv[2], "rb").read()[24:32]
_, count, size = struct.unpack_from("<3i", sco, 0)
for i in range(count):
    r = 12 + i * size
    if sco[r:r + 8] == rules:
        print(struct.unpack_from("<i", sco, r + 8)[0])
' "$1" "$2"; }

# Start-up must stay quick (the renderer and the art load; a headless run
# builds the same renderer as a window). 10 s is far above the usual
# fraction of a second, even for a software GPU.
mkdir -p "$WORK/startup"
start=$(date +%s%N)
(cd "$WORK/startup" && "$WF" --data "$DATA" --home "$WORK/startup" --headless --frames 1 >/dev/null 2>&1)
ms=$(( ($(date +%s%N) - start) / 1000000 ))
if [ "$ms" -gt 10000 ]; then
    printf 'start-up: FAIL %d ms (over 10000)\n' "$ms"
    fail=1
else
    printf 'start-up: ok %d ms\n' "$ms"
fi

for dir in "$REC_DIR"/*/; do
    name=$(basename "$dir")
    stages=$(ls "$dir"stage??.rec 2>/dev/null | sort)
    [ -n "$stages" ] || continue
    rm -rf "$WORK/home"; mkdir -p "$WORK/home/rec"
    cp "$dir"*.rec "$WORK/home/rec/"
    for rec in $stages; do
        n=$(basename "$rec" .rec); n=${n#stage}
        next=$(printf '%sstage%02d.rec' "$dir" $((10#$n + 1)))
        out=$(cd "$WORK/home" && WF_LOG=info "$WF" --data "$DATA" --home "$WORK/home" --headless \
            --replay "rec/stage$n.rec" --frames 40000 2>&1 | grep -E 'cleared|game over' | head -1)
        got=$(sed -nE 's/.*score ([0-9]+).*/\1/p' <<<"$out")
        if [ -f "$next" ]; then
            want=$(header_score "$next")
        elif [ -f "${dir}wf.sco" ]; then
            want=$(best_score "${dir}wf.sco" "$rec")
        else
            want=""
        fi
        if [ -z "$want" ]; then
            printf '%-36s stage %s: %s (no reference)\n' "$name" "$n" "$out"
        elif [ "$got" = "$want" ]; then
            printf '%-36s stage %s: ok %s\n' "$name" "$n" "$got"
        else
            printf '%-36s stage %s: FAIL got %s want %s (%s)\n' "$name" "$n" "$got" "$want" "$out"
            fail=1
        fi
    done
done
exit $fail
