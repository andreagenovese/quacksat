#!/bin/sh
# Fetch the wake-word models: the two shared openWakeWord feature models
# every wake word needs (melspectrogram, embedding_model), plus wake
# models — openWakeWord's pretrained ones (github.com/dscripka/openWakeWord,
# release v0.5.1) or quacksat's own "hey Daffy" (models/hey_daffy.onnx in
# this repository, fetched at $QUACKSAT_REF, default main — main's copy
# when GitHub does not have that ref).
#
# Usage: scripts/fetch-wake-models.sh [dest-dir] [wake-model...]
# Defaults: dest-dir = ./models, wake model = hey_jarvis_v0.1.onnx (the
# one the tests use). The installer asks for hey_daffy.onnx.
#
# Licences: openWakeWord's code is Apache-2.0, but its README puts the
# pre-trained models it publishes under CC BY-NC-SA 4.0 (non-commercial)
# because of their training data. That is why quacksat's release package
# carries no model file: they are downloaded from their own homes, here.
#
# Every file named below is checked against its sha256; a mismatch is
# deleted and fails the run. A file already present is kept as it is.
set -eu

DEST="${1:-models}"
shift 2>/dev/null || true
OWW="https://github.com/dscripka/openWakeWord/releases/download/v0.5.1"
MAIN="https://raw.githubusercontent.com/andreagenovese/quacksat/main/models"
OWN="https://raw.githubusercontent.com/andreagenovese/quacksat/${QUACKSAT_REF:-main}/models"

expected() {
    case "$1" in
        melspectrogram.onnx)  echo ba2b0e0f8b7b875369a2c89cb13360ff53bac436f2895cced9f479fa65eb176f ;;
        embedding_model.onnx) echo 70d164290c1d095d1d4ee149bc5e00543250a7316b59f31d056cff7bd3075c1f ;;
        hey_jarvis_v0.1.onnx) echo 94a13cfe60075b132f6a472e7e462e8123ee70861bc3fb58434a73712ee0d2cb ;;
        hey_daffy.onnx)       echo 8ef4e59eb0414a2ddf8ac0088613f9cd019c8f74ce1f125c4d8e841558fcffcf ;;
        *) echo "" ;;
    esac
}

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

mkdir -p "$DEST"
if [ $# -eq 0 ]; then set -- hey_jarvis_v0.1.onnx; fi
for f in melspectrogram.onnx embedding_model.onnx "$@"; do
    if [ -f "$DEST/$f" ]; then
        echo "$DEST/$f already present"
        continue
    fi
    case "$f" in
        hey_daffy.onnx) url="$OWN/$f" ;;
        *) url="$OWW/$f" ;;
    esac
    echo "fetching $f"
    if ! curl -sfL -o "$DEST/$f.part" "$url"; then
        # A ref GitHub does not have (a package built from a commit never
        # pushed): main's copy, which the checksum below still pins.
        case "$url" in
            "$OWN"/*) url="$MAIN/$f"; echo "  not at that ref, trying main" ;;
            *) url="" ;;
        esac
        if [ -z "$url" ] || ! curl -sfL -o "$DEST/$f.part" "$url"; then
            rm -f "$DEST/$f.part"
            echo "could not fetch $f" >&2
            exit 1
        fi
    fi
    want=$(expected "$f")
    if [ -n "$want" ] && [ "$(sha256 "$DEST/$f.part")" != "$want" ]; then
        rm -f "$DEST/$f.part"
        echo "$f: checksum mismatch, deleted" >&2
        exit 1
    fi
    mv "$DEST/$f.part" "$DEST/$f"
done
echo "wake models ready in $DEST/"
