#!/usr/bin/env bash
# Rebuild genuine Android APKs, then require positive 41-tool MCP coverage × 9.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
OUT="${RD_NATIVE_LANGUAGE_OUT:-$ROOT/target/native-languages/apks}"
EVIDENCE="${RD_NATIVE_LANGUAGE_EVIDENCE:-$ROOT/target/native-languages/evidence}"
python golden/native-languages/build.py --out "$OUT"
cargo build -p rd-cli --features native-engines
for abi in x86_64 arm64-v8a armeabi-v7a; do
  RD_NATIVE_ABI="$abi" RD_NATIVE_LANGUAGE_APKS="$OUT" \
    cargo test -p rd-engine-registry --features native-execution \
    --test native_execution -- --ignored --nocapture
  python scripts/native-languages-mcp.py --rd target/debug/rd \
    --apks "$OUT" --out "$EVIDENCE/$abi" --abi "$abi"
done

python scripts/native-languages-negative.py --rd target/debug/rd --apks "$OUT" --out "$EVIDENCE/negative"
