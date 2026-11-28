#!/bin/sh
# Executed only after native setup consent. All build products are private to the staging folder.
set -eu
umask 077
command -v git >/dev/null || { echo 'Install git, then select ik_llama.cpp again.' >&2; exit 1; }
command -v cmake >/dev/null || { echo 'Install CMake, then select ik_llama.cpp again.' >&2; exit 1; }
git clone --depth 1 https://github.com/ikawrakow/ik_llama.cpp.git src
METAL_FLAGS=""
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64)
    /usr/bin/xcrun --find clang >/dev/null
    METAL_FLAGS="-DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON"
    ;;
esac
# shellcheck disable=SC2086
cmake -S src -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=ON \
  -DLLAMA_CURL=OFF $METAL_FLAGS
JOBS=4
if command -v nproc >/dev/null 2>&1; then
  JOBS="$(nproc)"
elif command -v sysctl >/dev/null 2>&1; then
  JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"
fi
cmake --build build --target llama-server -j "$JOBS"
build/bin/llama-server --help >/dev/null
