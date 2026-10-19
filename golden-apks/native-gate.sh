#!/usr/bin/env bash
# M9 native gate (LANG-02): five real F-Droid APKs, pinned by version + SHA-256,
# whose bundled lib/x86_64/*.so code is executed by rd-ndk's isolated worker.
# APKs are cached under golden-apks/cache/ (never committed, GH-05).
# Requires: curl, sha256sum, python3 with unicorn==2.1.4 (or RD_NATIVE_PYTHON).
# Output: target/m9-native-gate.json; exit 1 if any required phase fails.
set -euo pipefail
cd "$(dirname "$0")/.."
CACHE=golden-apks/cache; mkdir -p "$CACHE" target
cargo build --release -q -p rd-vm --features native --example native_gate
G=target/release/examples/native_gate
fetch() { # file sha256 (F-Droid moves superseded versions from repo/ to archive/)
  local f="$CACHE/$1"
  if [ ! -f "$f" ] || ! echo "$2  $f" | sha256sum -c --quiet - 2>/dev/null; then
    curl -fsSL -o "$f" "https://f-droid.org/repo/$1" || curl -fsSL -o "$f" "https://f-droid.org/archive/$1"
  fi
  echo "$2  $f" | sha256sum -c --quiet -
}
fetch de.jurihock.voicesmith_13.apk 2a3fe5f5aeeb41dce59bc5241344fd4005e5c041804231df559c27d9b1354f2f
fetch com.termux_1022.apk fdd476982cd74f2f00aac12d3683b1fa260a0b2d146411b94e09d773be3a7b56
fetch eu.faircode.netguard_2026080101.apk 158b4308da0006298d22ed557adbff24b282fddeb1bc0b3468829a60f405a8c4
fetch de.markusfisch.android.binaryeye_180.apk 568cec6598dbf42fbde9071af8747071ffea798473016a3d77b6ddb3bff944f1
fetch org.woheller69.whobird_54.apk 77f13fa643f810ae264a6a3fbcd892d96e840eabcfb7b509eebfc8b8b1be9954
R=target/m9-native-gate; mkdir -p "$R"
# androidx.graphics.path: JNI_OnLoad RegisterNatives, then the real DEX
# ConicConverter.convert() calls internalConicToQuadratics. Skia's
# computeQuadPOW2 predicts 2^4 = 16 quadratics for this conic at tolerance 0.001.
CC='Landroidx/graphics/path/ConicConverter;'
$G "$CACHE/de.jurihock.voicesmith_13.apk" androidx.graphics.path \
  --call "$CC->convert\$default(${CC}[FFFIILjava/lang/Object;)V" \
  "@new:$CC" @floats:0,0,1,1,2,0 0.70710677 0.001 0 0 null \
  --call-on "@obj:$CC" "$CC->getQuadraticCount()I" > "$R/voicesmith.json"
$G "$CACHE/com.termux_1022.apk" termux --call 'Lcom/termux/terminal/JNI;->setPtyWindowSize(IIIII)V' -1 24 80 0 0 --call 'Lcom/termux/terminal/JNI;->close(I)V' -1 > "$R/termux.json"
$G "$CACHE/eu.faircode.netguard_2026080101.apk" netguard --call 'Leu/faircode/netguard/Util;->jni_getprop(Ljava/lang/String;)Ljava/lang/String;' '"net.dns1"' > "$R/netguard.json"
# zxing-cpp: Kotlin `object` receiver via its synthetic static `$default` helper;
# the returned SVG must be a QR symbol (checked below, decodable with zbarimg).
ZX='Lde/markusfisch/android/zxingcpp/ZxingCpp;'
$G "$CACHE/de.markusfisch.android.binaryeye_180.apk" zxingcpp_android \
  --call "$ZX->encodeStringAsSvg\$default(${ZX}Ljava/lang/String;Ljava/lang/String;ZIILjava/lang/Object;)Ljava/lang/String;" \
  "@static:$ZX->INSTANCE" '"RustyDroid"' '"QR_CODE"' false 0 0 null > "$R/binaryeye.json"
# TFLite: allocate a C++ BufferErrorReporter, hand the handle back to delete().
TF='Lorg/tensorflow/lite/NativeInterpreterWrapper;'
$G "$CACHE/org.woheller69.whobird_54.apk" tensorflowlite_jni \
  --call "$TF->createErrorReporter(I)J" 512 \
  --call "$TF->delete(JJJ)V" @ret:1 0 0 > "$R/whobird.json"
python3 - "$R" <<'PY'
import json, sys, pathlib
r = pathlib.Path(sys.argv[1]); out = []; ok = True
for f in sorted(r.glob("*.json")):
    d = json.loads(f.read_text()); phases = d["phases"]
    good = all(p["result"]["status"] == "ok" for p in phases) and d["final"].get("instructions", 0) > 0
    # every APK must make at least one JNI method call from the real DEX
    good &= len(phases) > 1
    if d["package"] == "de.markusfisch.android.binaryeye":
        svg = phases[1]["result"].get("value") or ""
        good &= svg.startswith("<?xml") and '<svg width="21" height="21"' in svg and "<path d=" in svg
    if d["package"] == "de.jurihock.voicesmith":
        good &= phases[2]["result"].get("value") == 16 and d["final"].get("registered_methods") == 8
    if d["package"] == "eu.faircode.netguard":
        good &= phases[1]["result"].get("value") == ""
    if d["package"] == "org.woheller69.whobird":
        good &= isinstance(phases[1]["result"].get("value"), int) and phases[1]["result"]["value"] != 0
    ok &= good
    out.append({"apk": d["package"], "sha256": d["sha256"], "passed": good,
                "native_instructions": d["final"].get("instructions"),
                "registered_natives": d["final"].get("registered_methods"),
                "libraries": d["final"].get("libraries"),
                "jni_calls": [{"call": p["phase"], "result": p["result"], "native_instructions": p.get("native_instructions_delta")} for p in phases[1:]], "logs": d["final"].get("logs")})
pathlib.Path("target/m9-native-gate.json").write_text(json.dumps({"passed": ok, "apks": out}, indent=2))
print(json.dumps({"passed": ok, "apks": [(a["apk"], a["passed"], a["native_instructions"]) for a in out]}))
sys.exit(0 if ok else 1)
PY
