# M5: Android lifecycle and visible UI

The portable `as-framework` adapter runs Application/Activity DEX bodies on the
shared RustyDroid Engine. Android descriptors select implemented API contracts;
there are no package-name or game branches. The host and Horizon runner use
the same adapter and rasterizer. See [the execution receipt](../reports/m5-ui.json).

Implemented scope includes Application context attachment/onCreate, an Activity
back stack, explicit Intent navigation/extras, binary XML and ARSC resources,
View/TextView/Button/LinearLayout/FrameLayout, click/touch callbacks, basic
Canvas/Paint fills/text, and Handler post/postDelayed/removal. The Looper uses
the platform monotonic clock. Framework references and side tables participate
in the VM collector. Callbacks invoke real app methods; unavailable APIs and
unsupported XML attributes fail with their required name.

The software renderer reuses RustyDroid's DejaVu glyph atlas. Font shaping,
themes, advanced layouts/widgets, AppCompat/Compose, full Canvas state/clip and
arbitrary resources are not implemented. This is a supported subset, not a
claim that whole APKs are compatible. Android reference-device execution and
physical Switch verification remain not run.

## Evidence and reproduction

```powershell
$env:CARGO_BUILD_JOBS='1'
./scripts/build-m5-fixture.ps1
cargo test -p as-framework --features host --test lifecycle -- --include-ignored
cargo clippy -p as-framework --features host --all-targets -- -D warnings
cargo build -p as-framework --features host --bin as-ui-probe
./scripts/build-androswitch.ps1
./scripts/test-ryujinx-ui.ps1 -Language java
./scripts/test-ryujinx-ui.ps1 -Language kotlin
```

The Java APK uses an Application subclass, XML/string/color resources, click
listeners, a delayed Runnable, private-file save/restart, a second Activity and
a custom View drawing Canvas shapes/text and receiving MotionEvent input.
The independent Kotlin APK uses its own DEX listener and widgets; its normal
Kotlin standard library is included in the APK. The pinned D8 emits Kotlin
metadata-rewriting warnings for that library; these are retained in the build
log. APK bytecode is not patched during execution.

Three host integration tests check lifecycle order, Looper deadlines, save
restart, Canvas pixels, callbacks after GC and named unavailable methods.
Ryujinx checks 10 Java and four Kotlin snapshots against host frame hashes and
lifecycle event order. Test input is explicitly synthetic Android input sent
through the same dispatcher as the physical Joy-Con/touch adapter. It does
not establish physical-controller verification.

The C backend writes a real libnx framebuffer, calls `framebufferEnd`, and
reports the submitted pixels' checksum. Its final PPM capture is made from the
CPU framebuffer before presentation, not by reading the emulator's window.
The app viewport is 640×360 at density 1, scaled 2× onto the 1280×720 window.
Receipts, CPU framebuffer capture, fresh session requests and emulator logs
are stored under ignored `local-artifacts/m5/runs/<session>/`.

Lifecycle follows the [Android Activity contract](https://developer.android.com/guide/components/activities/activity-lifecycle).
Native media is M6; the unified import/install/launch interface is M7.
