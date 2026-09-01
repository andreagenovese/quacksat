#!/bin/sh
# Fetch the openWakeWord ONNX models (Apache-2.0, github.com/dscripka/openWakeWord):
# the two shared feature models every wake word needs, plus pretrained
# wake models. quacksat's own "hey Daffy" model ships in the repo and is
# not downloaded here; hey_jarvis is fetched by default for the tests.
# Usage: scripts/fetch-wake-models.sh [dest-dir] [wake-model...]
# Defaults: dest-dir = ./models, wake model = hey_jarvis_v0.1.onnx
set -eu

DEST="${1:-models}"
shift 2>/dev/null || true
RELEASE="https://github.com/dscripka/openWakeWord/releases/download/v0.5.1"

mkdir -p "$DEST"
for f in melspectrogram.onnx embedding_model.onnx "${@:-hey_jarvis_v0.1.onnx}"; do
    if [ -f "$DEST/$f" ]; then
        echo "$DEST/$f already present"
    else
        echo "fetching $f"
        curl -sfL -o "$DEST/$f" "$RELEASE/$f"
    fi
done
echo "wake models ready in $DEST/"
