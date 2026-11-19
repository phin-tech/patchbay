#!/usr/bin/env bash
# Draws the pictures for Patchbay's page on apps.phin.tech into docs/media, with `just shot`: hidden windows on a
# gateway of their own, so nobody's servers, secrets or clients are in them. `just media` runs it.
set -euo pipefail
cd "$(dirname "$0")/.."
shots="${CARGO_TARGET_DIR:-target}/shots"

shot() { # name appearance keys
    PATCHBAY_KEYS="$3" just shot "$1" "$2" >/dev/null 2>&1
    cp "$shots/$1.png" "docs/media/$1.png"
    echo "docs/media/$1.png"
}

mkdir -p docs/media
shot servers light "wait wait"
shot servers-dark dark "wait wait"
# Clients, with Codex connected in the capture's stand-in home, so its entries show.
shot clients light "wait cmd-2 wait click:960,52 wait wait"
shot add-server light "wait cmd-n wait"
