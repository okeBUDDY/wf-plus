#!/usr/bin/env bash
# HD determinism check: records a scripted HD game at 4:3, 16:9,
# 21:9 and portrait window sizes, then replays each recording at its own
# size and at another one. The recording must lock its arena (the replays'
# per-frame state hash logs are identical whatever the window) and the
# replays must end on the live game's result.
#
# usage: scripts/hd-determinism.sh [data.bin]
# Needs a release build (cargo build --release -p wf) and python3.
set -u
DATA=${1:-${WF_DATA:-data.bin}}
WF=${WF_BIN:-${CARGO_TARGET_DIR:-target}/release/wf}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
fail=0

# Title: skip and start (GAME_START, then the arena choice); then fire and
# circle until the time runs out.
script="60:z,100:z,140:z,180:z,220:z,260:z,300:z,340:z,380:z,420:z,460:z,500:z,540:z,580:z,620:z,660:z,700-40000:z"
for f in $(seq 700 90 39910); do
    script="$script,$f-$((f + 89)):$(printf 'left\nup\nright\ndown' | sed -n "$(( (f / 90) % 4 + 1 ))p")"
done
result() { grep -E 'cleared|game over' | head -1 | sed -E 's/^\[[a-z]+\] //'; }
arena() { python3 -I -c 'import struct,sys; d=open(sys.argv[1],"rb").read(); v=struct.unpack_from("<i",d,0)[0]; print("%d %dx%d" % ((v,)+struct.unpack_from("<2H",d,32)) if v==1103 else v)' "$1"; }

for spec in "4:3 640x480 1280x720" "16:9 1280x720 640x480" "21:9 2560x1080 1280x720" "portrait 720x1280 1280x720"; do
    set -- $spec
    name=$1; size=$2; other=$3
    home="$WORK/$name"; mkdir -p "$home/rec"
    live=$(cd "$home" && WF_LOG=info "$WF" --data "$DATA" --home . --headless --size "$size" --arena hd \
        --frames 40000 --script "$script" 2>&1 | result)
    rec="$home/rec/last.rec"
    if [ ! -f "$rec" ]; then
        printf '%-9s FAIL no recording (%s)\n' "$name" "$live"; fail=1; continue
    fi
    for s in "$size" "$other"; do
        got=$(cd "$home" && WF_LOG=info "$WF" --data "$DATA" --home . --headless --size "$s" \
            --replay rec/last.rec --frames 40000 --hash-log "$WORK/$name-$s.txt" 2>&1 | result)
        if [ "$got" != "$live" ]; then
            printf '%-9s FAIL replay at %s: %s, live: %s\n' "$name" "$s" "$got" "$live"; fail=1
        fi
    done
    if cmp -s "$WORK/$name-$size.txt" "$WORK/$name-$other.txt"; then
        printf '%-9s ok: window %s, arena %s, %s hashes match at %s; %s\n' "$name" "$size" "$(arena "$rec")" \
            "$(wc -l < "$WORK/$name-$size.txt")" "$other" "$live"
    else
        printf '%-9s FAIL hash logs differ between %s and %s\n' "$name" "$size" "$other"; fail=1
    fi
done
exit $fail
