#!/usr/bin/env bash
# Build deterministic ABI fixtures from source, not downloaded binary blobs.
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p golden/native/artifacts
common=(-shared -fPIC -O1 -nostdlib -fno-stack-protector -Wl,--hash-style=both -Wl,--build-id=none)
gcc "${common[@]}" golden/native/fixture.c -o golden/native/artifacts/x86_64.so
aarch64-linux-gnu-gcc "${common[@]}" golden/native/fixture.c -o golden/native/artifacts/arm64-v8a.so
arm-linux-gnueabihf-gcc "${common[@]}" -mfloat-abi=softfp -Wl,--no-warn-mismatch golden/native/fixture.c -o golden/native/artifacts/armeabi-v7a.so
