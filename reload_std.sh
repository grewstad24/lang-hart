#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP_SNORE="/tmp/.snore"

mkdir -p "${TMP_SNORE}/musl"
rm -rf "${TMP_SNORE}/std"
cp -r "${PROJECT_ROOT}/std" "${TMP_SNORE}/std"

if [ ! -d "${TMP_SNORE}/musl/lib" ]; then
    ln -sfn "${PROJECT_ROOT}/vendor/musl/lib" "${TMP_SNORE}/musl/lib"
fi

echo "Reloaded standard library into ${TMP_SNORE}"

