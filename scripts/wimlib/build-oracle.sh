#!/usr/bin/env bash
# Build a test-only upstream oracle without modifying the preserved source tree.
set -euo pipefail
if (( $# != 2 )); then
    echo "usage: bash $0 SOURCE DISPOSABLE_COPY" >&2
    exit 2
fi
source_dir=$(realpath "$1")
oracle_dir=$(realpath -m "$2")
if [[ -e "$oracle_dir" ]]; then
    echo "oracle destination already exists: $oracle_dir" >&2
    exit 2
fi
mkdir -p "$oracle_dir"
cp -R "$source_dir"/. "$oracle_dir"/
cd "$oracle_dir"
./bootstrap
./configure --without-fuse --without-ntfs-3g --enable-test-support
make -j4
make tests/wlfuzz
printf 'Oracle built at %s\n' "$oracle_dir"
