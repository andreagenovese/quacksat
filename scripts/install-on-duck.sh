#!/usr/bin/env bash
# Install or upgrade quacksat on the duck, over ssh.
#
# Usage: install-on-duck.sh [--dry-run] <user@host> [binary]
#
# It runs from either of two places, and finds its files by its own:
#   - an unpacked release package (quacksat-<version>/, see
#     scripts/package.sh): bin/quacksat sits next to this script, and so
#     do systemd/, quacksat.example.toml and fetch-wake-models.sh;
#   - a checkout of the repository (scripts/install-on-duck.sh): the binary
#     defaults to target/aarch64-unknown-linux-gnu/release/quacksat
#     (scripts/cross-build.sh), the rest is systemd/, quacksat.example.toml
#     and scripts/; any *.onnx in the checkout's models/ (hey_daffy.onnx,
#     and whatever was fetched or trained there) rides along.
# A [binary] given on the command line wins over both.
#
# --dry-run prints every command it would run — the ssh and scp lines and
# the script it would run on the duck — and connects to nothing.
# Extra ssh/scp options, e.g. a port:
#   SSH_OPTS="-p 2222" ./install-on-duck.sh microduck@192.168.1.42
# Wake models fetched on the duck (default hey_daffy.onnx; the two shared
# feature models always come along):
#   WAKE_MODELS="hey_daffy.onnx hey_jarvis_v0.1.onnx" ./install-on-duck.sh ...
#
# Idempotent, and it replaces only what it owns: the binary
# (/usr/local/bin/quacksat), the unit (/etc/systemd/system/
# quacksat.service) and the service account (/etc/sysusers.d/
# quacksat.conf). The config (/etc/robot/quacksat.toml) is installed only
# when there is none, readable by root and the service alone (0640
# root:quacksat: it will hold API keys). The wake models go to
# /var/lib/quacksat/models, a file already there is kept; they are
# downloaded on the duck, so it needs the internet once — without it the
# install goes on and says what is missing. Then the service is enabled
# and (re)started. The user needs sudo on the duck.
set -euo pipefail

USAGE="usage: install-on-duck.sh [--dry-run] <user@host> [binary]"
DRY_RUN=0
if [ "${1:-}" = "--dry-run" ]; then DRY_RUN=1; shift; fi
HOST="${1:?$USAGE}"

HERE="$(cd "$(dirname "$0")" && pwd)"
MODELS=()
if [ -f "$HERE/bin/quacksat" ]; then
    # The release package's layout.
    DEFAULT_BIN="$HERE/bin/quacksat"
    UNIT="$HERE/systemd/quacksat.service"
    SYSUSERS="$HERE/systemd/sysusers.d/quacksat.conf"
    CONFIG="$HERE/quacksat.example.toml"
    FETCH="$HERE/fetch-wake-models.sh"
    # "hey Daffy" is fetched from the repository at the package's commit.
    REF=$(cut -d' ' -f2 "$HERE/SOURCE" 2>/dev/null || echo main)
    BUILD_HINT="the package is incomplete: unpack it again"
else
    # The repository's layout (this script in scripts/).
    ROOT="$(cd "$HERE/.." && pwd)"
    DEFAULT_BIN="$ROOT/target/aarch64-unknown-linux-gnu/release/quacksat"
    UNIT="$ROOT/systemd/quacksat.service"
    SYSUSERS="$ROOT/systemd/sysusers.d/quacksat.conf"
    CONFIG="$ROOT/quacksat.example.toml"
    FETCH="$ROOT/scripts/fetch-wake-models.sh"
    REF=main
    for m in "$ROOT"/models/*.onnx; do
        [ -f "$m" ] && MODELS+=("$m")
    done
    BUILD_HINT="run scripts/cross-build.sh first"
fi
BIN="${2:-$DEFAULT_BIN}"
WAKE_MODELS="${WAKE_MODELS:-hey_daffy.onnx}"
case "$WAKE_MODELS$REF" in
    *[!A-Za-z0-9._\ -]*) echo "WAKE_MODELS: file names only" >&2; exit 1 ;;
esac
# ${A[@]+"${A[@]}"} below: macOS bash 3.2 calls an empty array unbound under set -u.
read -ra OPTS <<< "${SSH_OPTS:-}"

[ -f "$BIN" ] || { echo "missing $BIN — $BUILD_HINT" >&2; exit 1; }
for f in "$UNIT" "$SYSUSERS" "$CONFIG" "$FETCH"; do
    [ -f "$f" ] || { echo "missing $f — $BUILD_HINT" >&2; exit 1; }
done
if command -v file >/dev/null && ! file "$BIN" | grep -q 'ARM aarch64'; then
    echo "$BIN is not an aarch64 binary: $(file -b "$BIN")" >&2
    exit 1
fi

SCP_OPTS=()
for ((i = 0; i < ${#OPTS[@]}; i++)); do
    # scp spells ssh's -p (port) as -P.
    if [ "${OPTS[i]}" = "-p" ]; then SCP_OPTS+=("-P"); else SCP_OPTS+=("${OPTS[i]}"); fi
done

# Run a local command, or with --dry-run only print it.
step() {
    if [ "$DRY_RUN" = 1 ]; then
        printf '+'; printf ' %q' "$@"; printf '\n'
    else
        "$@"
    fi
}

if [ "$DRY_RUN" = 1 ]; then
    echo "dry run: nothing is copied and nothing runs on $HOST"
    STAGE=/tmp/quacksat-install.XXXXXX
    step ssh ${OPTS[@]+"${OPTS[@]}"} "$HOST" mktemp -d /tmp/quacksat-install.XXXXXX
else
    STAGE=$(ssh ${OPTS[@]+"${OPTS[@]}"} "$HOST" mktemp -d /tmp/quacksat-install.XXXXXX)
fi
echo "copying to $HOST:$STAGE"
step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "$BIN" "$HOST:$STAGE/quacksat"
step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "$UNIT" "$HOST:$STAGE/quacksat.service"
step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "$SYSUSERS" "$HOST:$STAGE/sysusers.conf"
step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "$CONFIG" "$HOST:$STAGE/quacksat.toml"
step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "$FETCH" "$HOST:$STAGE/fetch-wake-models.sh"
if [ ${#MODELS[@]} -gt 0 ]; then
    step ssh ${OPTS[@]+"${OPTS[@]}"} "$HOST" mkdir "$STAGE/models"
    step scp ${SCP_OPTS[@]+"${SCP_OPTS[@]}"} -q "${MODELS[@]}" "$HOST:$STAGE/models/"
fi

# shellcheck disable=SC2087 # $STAGE, $REF, $WAKE_MODELS are expanded here on purpose
REMOTE=$(cat <<REMOTE
set -euo pipefail
cd "$STAGE"
run() { echo "+ \$*"; sudo "\$@"; }

run install -m 755 quacksat /usr/local/bin/quacksat
run install -m 644 quacksat.service /etc/systemd/system/quacksat.service
run install -D -m 644 sysusers.conf /etc/sysusers.d/quacksat.conf
run systemd-sysusers /etc/sysusers.d/quacksat.conf
for g in robot audio; do
    getent group "\$g" >/dev/null \\
        || echo "warning: no '\$g' group here: the unit's SupplementaryGroups=robot audio will fail (robot: robotd's socket, audio: /dev/snd)"
done
command -v arecord >/dev/null && command -v aplay >/dev/null \\
    || echo "warning: no arecord/aplay here (alsa-utils): the satellite can neither listen nor speak"

if sudo test -f /etc/robot/quacksat.toml; then
    echo "keeping /etc/robot/quacksat.toml (\$(sudo stat -c '%a %U:%G' /etc/robot/quacksat.toml))"
else
    run install -d -m 755 /etc/robot
    run install -o root -g quacksat -m 640 quacksat.toml /etc/robot/quacksat.toml
    echo "installed /etc/robot/quacksat.toml: edit it (backend, endpoints, keys), then: sudo systemctl restart quacksat"
fi

# The wake models: those that came along first, then the download of
# whatever is still missing (each file checked against its sha256).
run install -d -m 755 /var/lib/quacksat /var/lib/quacksat/models
if [ -d models ]; then
    for m in models/*.onnx; do
        if sudo test -f "/var/lib/quacksat/\$m"; then
            echo "keeping /var/lib/quacksat/\$m"
        else
            run install -m 644 "\$m" "/var/lib/quacksat/\$m"
        fi
    done
fi
if ! run env QUACKSAT_REF="$REF" sh fetch-wake-models.sh /var/lib/quacksat/models $WAKE_MODELS; then
    echo "warning: the wake models could not all be downloaded (no internet?): the satellite"
    echo "         will not wake until /var/lib/quacksat/models holds them — run the installer again"
fi

run systemctl daemon-reload
run systemctl enable quacksat
run systemctl restart quacksat
sleep 2
sudo systemctl --no-pager --lines=8 status quacksat || true
rm -rf "$STAGE"
REMOTE
)

if [ "$DRY_RUN" = 1 ]; then
    step ssh ${OPTS[@]+"${OPTS[@]}"} "$HOST" bash -s
    echo "  with this script on its standard input:"
    printf '%s\n' "$REMOTE" | sed 's/^/  | /'
    exit 0
fi
ssh ${OPTS[@]+"${OPTS[@]}"} "$HOST" bash -s <<< "$REMOTE"

echo "installed on $HOST — follow it with: ssh $HOST journalctl -u quacksat -f"
