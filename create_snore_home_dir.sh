#!/usr/bin/env bash
set -euo pipefail

SNORE_HOME="${HOME}/.snore"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STD_SRC="${PROJECT_ROOT}/std"
COSMO_SRC="${PROJECT_ROOT}/vendor/cosmocc"
COSMO_DEST="${SNORE_HOME}/cosmocc"

# 1. Reset ~/.snore
printf "1/3 : Resetting %s ... " "${SNORE_HOME}"
rm -rf "${SNORE_HOME}"
mkdir -p "${SNORE_HOME}"
echo "done"

# 2. Copy standard library
printf "2/3 : Installing standard library ... "
if [ -d "${STD_SRC}" ]; then
    cp -r "${STD_SRC}" "${SNORE_HOME}/std"
    echo "done"
else
    echo "skipped (warning: '${STD_SRC}' not found)" >&2
fi

# 3. Install Cosmopolitan toolchain
printf "3/3 : Installing Cosmopolitan toolchain to %s ... " "${COSMO_DEST}"
if [ -d "${COSMO_SRC}" ]; then
    cp -r "${COSMO_SRC}" "${COSMO_DEST}"
    echo "done"
elif [ -d "${HOME}/.local/opt/cosmocc" ]; then
    cp -r "${HOME}/.local/opt/cosmocc" "${COSMO_DEST}"
    echo "done"
else
    echo "error: cosmocc not found in '${COSMO_SRC}' or '~/.local/opt/cosmocc'" >&2
    exit 1
fi
