#!/bin/sh
# Pre-compress Trunk's dist output for nginx's `gzip_static`.
#
# Shared by the Dockerfile's build stage and the CI `trunk` job so both produce
# byte-identical artifacts — the CI job uploads dist/ for the image build to
# consume, so a mismatch here would mean the two paths ship different bytes.
#
# Compressing once at build time is the point: the ~32MB wasm previously went
# over the wire uncompressed on every page load, because `trunk serve` applied
# a global `no-store` and no compression.
#
# Usage: deploy/precompress.sh <dist-dir>
set -eu

DIST="${1:?usage: precompress.sh <dist-dir>}"
[ -d "$DIST" ] || { echo "precompress: '$DIST' is not a directory" >&2; exit 1; }

# -k keeps the uncompressed original beside the .gz, for clients that send no
# Accept-Encoding. -f overwrites a stale .gz from a previous run.
find "$DIST" -type f \
  \( -name '*.wasm' -o -name '*.js' -o -name '*.css' -o -name '*.html' \
     -o -name '*.json' -o -name '*.svg' -o -name '*.ico' \) \
  -exec gzip -9 -k -f {} +

echo "precompress: gzipped $(find "$DIST" -name '*.gz' -type f | wc -l) file(s) in $DIST"
