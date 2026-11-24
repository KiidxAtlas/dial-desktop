#!/bin/sh
# Executed only after native setup consent. All build products are private to the staging folder.
set -eu
umask 077
revision=e110381ae2066d4f5018585263b9b302cd2f4516
sha=a81915519486d3c68ece9c35fdcca1c94e3d537cb518272adfbb54852bdcd32d
case "$(uname -s)/$(uname -m)" in Darwin/arm64) ;; *) echo 'Atomic setup requires Apple Silicon.' >&2; exit 1;; esac
command -v cmake >/dev/null || { echo 'Install CMake, then select Atomic again.' >&2; exit 1; }
/usr/bin/xcrun --find clang >/dev/null
/usr/bin/curl -q --fail --silent --show-error --max-time 240 --max-filesize 134217728 --proto '=https' --output source.tar.gz "https://codeload.github.com/AtomicBot-ai/atomic-engine/tar.gz/$revision"
printf '%s  source.tar.gz\n' "$sha" | /usr/bin/shasum -a 256 -c -
/usr/bin/tar -xzf source.tar.gz
cmake -S "atomic-engine-$revision" -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_APP=OFF -DLLAMA_BUILD_UI=OFF -DLLAMA_OPENSSL=OFF \
  -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON -DGGML_BLAS=OFF -DGGML_CCACHE=OFF
cmake --build build --target llama-server -j 6
build/bin/llama-server --version
