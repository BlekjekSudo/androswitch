#!/usr/bin/env bash
# Build deterministic ABI fixtures from source, not downloaded binary blobs.
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p golden/native/artifacts
common=(-shared -fPIC -O1 -nostdlib -fno-stack-protector -Wl,--hash-style=both -Wl,--build-id=none)
# -march=x86-64: pin the baseline ISA. Ubuntu 26.04 runners default to x86-64-v3,
# which makes GCC emit VEX/AVX scalar code (vaddsd, vmovq). The Unicorn 2.1.4 worker
# mis-executes those instructions (xmm0 is left unchanged), so floor(5.5)+0.25
# came out as 0.25 on CI only (issue #393). The SSE baseline runs correctly.
gcc "${common[@]}" -march=x86-64 golden/native/fixture.c -o golden/native/artifacts/x86_64.so
# Negative fixture (issue #400): same source built for x86-64-v3, so GCC emits VEX.
# The worker must reject it with NOT_IMPLEMENTED instead of returning a wrong value.
gcc "${common[@]}" -march=x86-64-v3 golden/native/fixture.c -o golden/native/artifacts/x86_64-avx2.so
aarch64-linux-gnu-gcc "${common[@]}" golden/native/fixture.c -o golden/native/artifacts/arm64-v8a.so
arm-linux-gnueabihf-gcc "${common[@]}" -mfloat-abi=softfp -Wl,--no-warn-mismatch golden/native/fixture.c -o golden/native/artifacts/armeabi-v7a.so
