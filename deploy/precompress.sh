#!/bin/sh
# Gzips Trunk's dist output for nginx gzip_static. Usage: deploy/precompress.sh <dist-dir>
set -eu

DIST="${1:?usage: precompress.sh <dist-dir>}"
[ -d "$DIST" ] || { echo "precompress: '$DIST' is not a directory" >&2; exit 1; }

# -k keeps each original beside its .gz; -f replaces a stale .gz.
find "$DIST" -type f \
  \( -name '*.wasm' -o -name '*.js' -o -name '*.css' -o -name '*.html' \
     -o -name '*.json' -o -name '*.svg' -o -name '*.ico' \) \
  -exec gzip -9 -k -f {} +

echo "precompress: gzipped $(find "$DIST" -name '*.gz' -type f | wc -l) file(s) in $DIST"
