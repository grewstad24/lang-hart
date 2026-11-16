#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HART_HOME="${HART_HOME:-${HOME}/.hart}"

mkdir -p "${HART_HOME}"
rm -rf "${HART_HOME}/std"
cp -r "${PROJECT_ROOT}/std" "${HART_HOME}/std"

if [ ! -d "${HART_HOME}/cosmocc" ]; then
    if [ -d "${PROJECT_ROOT}/vendor/cosmocc" ]; then
        cp -r "${PROJECT_ROOT}/vendor/cosmocc" "${HART_HOME}/cosmocc"
    fi
fi

echo "Reloaded standard library into ${HART_HOME}"
