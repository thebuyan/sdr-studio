#!/bin/sh
# SPDX-License-Identifier: GPL-2.0-only
set -eu
SDR_STUDIO_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -x "$SDR_STUDIO_DIR/bin/sdr-studio" ]; then
    exec "$SDR_STUDIO_DIR/bin/sdr-studio" "$@"
fi
if [ -f "$SDR_STUDIO_DIR/bin/libsdstudio-dmr.so" ]; then
    export SDR_STUDIO_DMR_LIBRARY="${SDR_STUDIO_DMR_LIBRARY:-$SDR_STUDIO_DIR/bin/libsdstudio-dmr.so}"
fi
exec cargo run --locked --manifest-path "$SDR_STUDIO_DIR/Cargo.toml" -- "$@"
