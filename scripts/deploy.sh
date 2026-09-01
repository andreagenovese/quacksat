#!/bin/sh
# Deploy quacksat to the duck (or any aarch64 Armbian/Debian board).
# Usage: scripts/deploy.sh <duck-host>
#
# Installs the cross-built binary, the systemd unit and service account,
# a default config (kept if one already exists), and the wake-word
# models. Everything lands outside the microduck updater's managed set
# (see docs/study/microduck-ipc-and-packaging.md), so robot updates
# never touch it.
set -eu

HOST="${1:?usage: deploy.sh <duck-host>}"
BIN=target/aarch64-unknown-linux-gnu/release/quacksat

cd "$(dirname "$0")/.."
[ -f "$BIN" ] || { echo "missing $BIN — run scripts/build-aarch64.sh first" >&2; exit 1; }

scp "$BIN" "$HOST":/tmp/quacksat
scp systemd/quacksat.service "$HOST":/tmp/quacksat.service
scp systemd/sysusers.d/quacksat.conf "$HOST":/tmp/quacksat-sysusers.conf
scp quacksat.example.toml "$HOST":/tmp/quacksat.example.toml
scp scripts/fetch-wake-models.sh "$HOST":/tmp/fetch-wake-models.sh
# Custom wake models trained locally (e.g. hey_daffy.onnx) ride along.
if ls models/*.onnx >/dev/null 2>&1; then
    ssh "$HOST" 'mkdir -p /tmp/quacksat-models'
    scp models/*.onnx "$HOST":/tmp/quacksat-models/
fi

ssh "$HOST" 'set -eu
sudo install -m 755 /tmp/quacksat /usr/local/bin/quacksat
sudo install -m 644 /tmp/quacksat.service /etc/systemd/system/quacksat.service
sudo install -m 644 /tmp/quacksat-sysusers.conf /etc/sysusers.d/quacksat.conf
sudo systemd-sysusers
# Config: install the example only if none exists — never overwrite.
if [ ! -f /etc/robot/quacksat.toml ]; then
    sudo mkdir -p /etc/robot
    sudo install -m 644 /tmp/quacksat.example.toml /etc/robot/quacksat.toml
    echo "installed default config at /etc/robot/quacksat.toml — edit it"
fi
# Wake-word models (idempotent; skips files already present).
sudo mkdir -p /var/lib/quacksat/models
sudo sh /tmp/fetch-wake-models.sh /var/lib/quacksat/models
if [ -d /tmp/quacksat-models ]; then
    sudo install -m 644 /tmp/quacksat-models/*.onnx /var/lib/quacksat/models/
fi
sudo systemctl daemon-reload
sudo systemctl enable --now quacksat
sudo systemctl restart quacksat
rm -rf /tmp/quacksat /tmp/quacksat.service /tmp/quacksat-sysusers.conf \
      /tmp/quacksat.example.toml /tmp/fetch-wake-models.sh /tmp/quacksat-models'
echo "Deployed to $HOST — check: ssh $HOST journalctl -u quacksat -f"
