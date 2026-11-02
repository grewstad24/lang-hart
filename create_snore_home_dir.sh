#!/usr/bin/env bash
set -euo pipefail

SNORE_HOME="${HOME}/.snore"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STD_SRC="${PROJECT_ROOT}/std"
MUSL_SRC="${PROJECT_ROOT}/vendor/musl"
MUSL_DEST="${SNORE_HOME}/musl"

# 1. Reset ~/.snore
printf "1/6 : Resetting %s ... " "${SNORE_HOME}"
rm -rf "${SNORE_HOME}"
mkdir -p "${SNORE_HOME}"
echo "done"

# 2. Copy standard library
printf "2/6 : Installing standard library ... "
if [ -d "${STD_SRC}" ]; then
    cp -r "${STD_SRC}" "${SNORE_HOME}/std"
    echo "done"
else
    echo "skipped (warning: '${STD_SRC}' not found)" >&2
fi

# 3. Clean previous build artifacts
if [ ! -f "${MUSL_SRC}/configure" ]; then
    echo "error: musl source not found at '${MUSL_SRC}'" >&2
    exit 1
fi

cd "${MUSL_SRC}"

printf "3/6 : Cleaning musl build artifacts ... "
make distclean >/dev/null 2>&1 || make clean >/dev/null 2>&1 || true
echo "done"

# 4. Configure musl
printf "4/6 : Configuring musl (x86_64-unknown-linux-musl) ... "
CC="clang --target=x86_64-unknown-linux-musl" \
./configure \
    --prefix="${MUSL_DEST}" \
    --syslibdir="${MUSL_DEST}/lib" \
    --enable-static \
    --disable-shared >/dev/null
echo "done"

# 5. Compile musl
printf "5/6 : Compiling musl static library & runtime objects ... "
make -j"$(nproc)" >/dev/null
echo "done"

# 6. Install into ~/.snore/musl
printf "6/6 : Installing musl to %s ... " "${MUSL_DEST}"
make install >/dev/null
echo "done"
