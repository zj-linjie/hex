#!/bin/sh
# Isolated fixture preview for Labs › Translation.
# Always rebuilds via cargo run --locked; never reuses a stale binary,
# never launches the installed app, and never writes user settings.
set -eu

ROOT=$(CDPATH= command cd -- "$(dirname -- "$0")" && pwd)
cd "$ROOT"

out=$(mktemp -d "${TMPDIR:-/tmp}/hex-translation-preview.XXXXXX")
export HEX_APPLICATION_SUPPORT_DIR="$out/app-data"
mkdir -p "$HEX_APPLICATION_SUPPORT_DIR"

exec cargo run --locked --bin hex -- app -- \
  --snap "$out/mac-translation-word-dark.png" \
  --scene mac-translation-word \
  --theme dark
