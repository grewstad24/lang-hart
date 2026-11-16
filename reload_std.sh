#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP_SNORE="/tmp/.snore"

mkdir -p "${TMP_SNORE}"
rm -rf "${TMP_SNORE}/std"
cp -r "${PROJECT_ROOT}/std" "${TMP_SNORE}/std"

if [ ! -d "${TMP_SNORE}/cosmocc" ]; then
    if [ -d "${PROJECT_ROOT}/vendor/cosmocc" ]; then
        cp -r "${PROJECT_ROOT}/vendor/cosmocc" "${TMP_SNORE}/cosmocc"
    fi
fi

echo "Reloaded standard library into ${TMP_SNORE}"

