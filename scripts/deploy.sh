#!/bin/sh
# Deploy quacksat to the duck. Usage: scripts/deploy.sh <duck-host>
# Expects scripts/build-aarch64.sh to have produced the release binary.
set -eu

HOST="${1:?usage: deploy.sh <duck-host>}"
BIN=target/aarch64-unknown-linux-gnu/release/quacksat

cd "$(dirname "$0")/.."
[ -f "$BIN" ] || { echo "missing $BIN — run scripts/build-aarch64.sh first" >&2; exit 1; }

scp "$BIN" "$HOST":/tmp/quacksat
scp systemd/quacksat.service "$HOST":/tmp/quacksat.service
ssh "$HOST" 'sudo install -m 755 /tmp/quacksat /usr/local/bin/quacksat && \
             sudo install -m 644 /tmp/quacksat.service /etc/systemd/system/quacksat.service && \
             sudo systemctl daemon-reload && \
             sudo systemctl restart quacksat'
echo "Deployed to $HOST"
