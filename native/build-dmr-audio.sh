#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
set -eu
if [ "$#" -ne 1 ]; then
    echo "Uso: $0 /ruta/al/código-fuente-de-mbelib" >&2
    exit 2
fi
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mbe=$(CDPATH= cd -- "$1" && pwd)
mkdir -p "$root/bin"
stage=$(mktemp "$root/bin/.dmr-audio.XXXXXX")
trap 'rm -f "$stage"' EXIT
"${CC:-cc}" -O2 -fPIC -shared -I"$mbe" -o "$stage" \
    "$root/native/dmr_audio.c" "$mbe/ambe3600x2450.c" "$mbe/ecc.c" "$mbe/mbelib.c" -lm
install -m 644 "$mbe/COPYRIGHT" "$root/bin/libsdstudio-dmr.COPYRIGHT"
chmod 755 "$stage"
mv -f "$stage" "$root/bin/libsdstudio-dmr.so"
