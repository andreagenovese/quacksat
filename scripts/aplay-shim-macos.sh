#!/bin/sh
# aplay → sox `play` shim for macOS development (brew install sox).
# quacksat spawns its playback program with aplay-style arguments; this
# translates them so TTS comes out of the Mac's speakers. Point
# [audio].playback_program at this script.
set -eu

rate=22050
channels=1
raw=0
file=""
while [ $# -gt 0 ]; do
    case "$1" in
        -r) rate="$2"; shift 2 ;;
        -c) channels="$2"; shift 2 ;;
        -t) [ "$2" = "raw" ] && raw=1; shift 2 ;;
        -D|-f) shift 2 ;;
        -q) shift ;;
        *) file="$1"; shift ;;
    esac
done

if [ "$raw" = 1 ]; then
    exec play -q -t raw -r "$rate" -c "$channels" -e signed-integer -b 16 -
else
    exec play -q "$file"
fi
