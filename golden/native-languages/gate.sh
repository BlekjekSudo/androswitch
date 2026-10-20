#!/usr/bin/env bash
# Rebuild genuine Android APKs, then require positive 41-tool MCP coverage x 18.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
OUT="${RD_NATIVE_LANGUAGE_OUT:-$ROOT/target/native-languages/apks}"
EVIDENCE="${RD_NATIVE_LANGUAGE_EVIDENCE:-$ROOT/target/native-languages/evidence}"
python golden/native-languages/build.py --out "$OUT"
cargo build -p rd-cli --features native-engines,jit --locked
for abi in x86_64 arm64-v8a armeabi-v7a; do
  RD_NATIVE_ABI="$abi" RD_NATIVE_LANGUAGE_APKS="$OUT" \
    cargo test -p rd-engine-registry --features native-execution --locked \
    --test native_execution -- --ignored --nocapture
  # The public CLI must emit one JSON artifact with guest stdout captured inside it.
  RD_NATIVE_ABI="$abi" RD_NATIVE_LANGUAGE_APKS="$OUT" \
    cargo test -p rd-cli --features native-engines,jit --locked \
    --test engine_cli -- --ignored --nocapture
  for jit in off on; do
    python scripts/native-languages-mcp.py --rd "$ROOT/target/debug/rd" \
      --apks "$OUT" --out "$EVIDENCE/$jit/$abi" --abi "$abi" --jit "$jit"
  done
done
python scripts/native-languages-negative.py --rd "$ROOT/target/debug/rd" \
  --apks "$OUT" --out "$EVIDENCE/negative"
