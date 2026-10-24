# compat-matrix — placar oficial do RustyDroid

> **GERADO** por `cargo xtask compat-report` — não editar à mão.
> Lei de Ouro: nenhuma API ausente quebra o app em silêncio — tudo responde `NOT_IMPLEMENTED(id)` e aparece aqui.

## Estado por fase (roadmap, lido de docs/ROADMAP.md)

| Fase | Entrega | Status |
|---|---|---|
| M0 | Workspace + CI + rd-apk + permissions.toml + devices.toml (moto-e5) + behavior-switches.toml + motor de permissões | **✅ implementado** |
| M1 | rd-dex 100% + disassembler | **✅ implementado** |
| M2 | VM Dalvik mínima | **✅ implementado** |
| M3 | Framework essencial | **✅ implementado** |
| M4 | Render+dump | **✅ implementado** |
| M5 | Agente MCP v1 (30 tools + controles) | **✅ implementado** |
| M6 | Executor de testes | **✅ implementado** |
| M7 | Performance + gate piso E5 | ⬜ |
| M8 | Compat real | **✅ implementado** |
| M9 | Componentes avançados + NDK | **✅ implementado** |
| M10 | Dispositivos virtuais | ⬜ |
| M11 | Engines | ⬜ |
| M12 | Ecossistema GH | ⬜ |

## Crates

| Crate | Fase | Escopo |
|---|---|---|
| `rd-apk` | M0 | ZIP + AXML + arsc + assinaturas + manifest model completo |
| `rd-framework` | M0 | fd-permissions (motor completo + gating maxSdk/since_api) · fd-devices · fd-behavior · fd-api-coverage (registro executável do Apêndice C) |
| `rd-cli` | M0–M6 | inspect · perm · device · behavior · api-coverage · dex disasm · vm exec · app run · mcp · test (suítes JUnit, M6) |
| `rd-dex` | M1 | parser DEX completo + disassembler smali validado vs baksmali + fuzz targets |
| `rd-vm` | M2–M9 | interpretador Dalvik + golden harness vs JVM + framework host headless (Activity/View/Handler/Looper) + LayoutInflater AXML/ARSC + componentes M9 (notificações/IME/widgets/a11y/atalhos) + ponte JNI (feature `native`) |
| `rd-testsupport` | M0–M6 | fixtures de teste (APK/DEX/AXML/ARSC sintéticos; dev-only, fora do binário) |
| `rd-render` | M4 | UI dump uiautomator + screenshot headless (software renderer determinístico + PNG sem deps); winit/web são stub → M11 |
| `rd-agent` | M5–M9 | MCP (47 tools + batch + policy sandbox + screen resources/live viewer; AG-09 estático classificado pelo Apêndice C; M9: widgets/IME/a11y/shortcuts/native_state) · executor de suítes `rd test` (JSON → JUnit XML, M6) |
| `rd-jni` | M9 | mangling JNI (curto/sobrecarga), descritores, MUTF-8, frames de refs locais/globais |
| `rd-ndk` | M9 | parser ELF32/64 limitado (x86_64/AArch64/ARM) + runtime `native` em worker Unicorn isolado (ADR 004) |
| `rd-engine-registry` | Apêndice G | registro/detecção dos 28 itens; features/contratos; NÃO executa engines |

## Suítes automatizadas (`rd test`, M6)

| Suíte | Arquivo | Cenários | Passos |
|---|---|---|---|
| contract-no-app | `tests/suites/contract-no-app.json` | 8 | 10 |
| m8-compat | `tests/suites/m8-compat.json` | 2 | 4 |

Executar: `rd test tests/suites/<arquivo>.json --junit <saida>.xml` (formato em `docs/suite-format.md`).

## Engines (Apêndice G — registro/detecção, não execução)

**28 registros completos; 12 módulos opt-in.** Features não implementam runtimes; todos os adaptadores do registro retornam `NOT_IMPLEMENTED(LANG-xx)`. DEX e ELF/JNI parciais pertencem às rotas existentes (native é opt-in), não aos adaptadores do registro. M11 continua pendente. Detecção golden não é golden de execução.

| # | Linguagem/Framework | Artefato no APK | Módulo | Estratégia | Status |
|---|---|---|---|---|---|
| 1 | Java | classes*.dex | LANG-01 | VM DEX própria | Registro ✅; runtime DEX parcial |
| 2 | Kotlin | classes*.dex | LANG-01 | VM DEX; coroutines/threads dependem da cobertura da VM | Registro ✅; runtime DEX parcial |
| 3 | Kotlin Multiplatform / Compose MP | DEX; recursos Compose; klib nem sempre é distribuída | LANG-10 | Shared DEX + futuro render Compose MP | Registro ✅; runtime NOT_IMPLEMENTED |
| 4 | Scala / Clojure / Groovy | classes*.dex | LANG-01 | VM DEX; invocações dinâmicas conforme cobertura | Registro ✅; runtime DEX parcial |
| 5 | C (NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 6 | C++ (NDK) | lib/<abi>/*.so; STL opcional | LANG-02 | ELF + JNI + STL | Registro ✅; runtime ELF/JNI parcial (native) |
| 7 | Rust (cargo-ndk/xbuild) | lib/<abi>/*.so | LANG-03 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 8 | Go (gomobile) | lib/<abi>/*.so; wrappers bind | LANG-09 | ELF + JNI + gomobile bind | Registro ✅; runtime NOT_IMPLEMENTED |
| 9 | Objective-C/C (cross via NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI; runtime ObjC se necessário | Registro ✅; runtime ELF/JNI parcial (native) |
| 10 | Zig / outras linguagens LLVM | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime ELF/JNI parcial (native) |
| 11 | Dart / Flutter | libapp.so; libflutter.so; flutter_assets | LANG-04 | Dart AOT + engine Flutter headless futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 12 | JS/TS (React Native / Expo) | bundle JS; Hermes bytecode/JSC; libs RN | LANG-05 | Engine JS + RN/Expo bridge futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 13 | HTML/JS (Cordova / Capacitor / Ionic) | assets/www/ ou assets/public/; configs bridge | LANG-06 | WebView headless + bridges futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 14 | HTML/JS (TWA / PWA) | Manifest/URL; pode não haver payload web local | LANG-06 | WebView/TWA futuro; rede não consultada | Registro ✅; runtime NOT_IMPLEMENTED |
| 15 | JS/TS (NativeScript) | assets/app/; libNativeScript.so | LANG-05+LANG-02 | Engine JS + JNI + views futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 16 | JS/TS (Tauri v2) | assets web + Rust .so; markers Tauri opcionais | LANG-06+LANG-03 | WebView + Rust JNI futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 17 | C# (.NET MAUI / Xamarin) | assemblies/*.dll; Mono; blobs de assemblies | LANG-07 | Mono/IL + bridges futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 18 | C# (Unity Mono) | Assembly-CSharp.dll; libunity.so | LANG-08 | Mono + stubs/render Unity futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 19 | C# (Unity IL2CPP) | libil2cpp.so; global-metadata.dat | LANG-08 | Interp nativo + stubs/render Unity futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 20 | C++ (Unreal) | libUE4.so / libUnreal.so; pak/obb | LANG-09 | Engine/stubs Unreal futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 21 | GDScript/C# (Godot 3/4) | libgodot*.so; .pck/assets | LANG-09 | Godot headless embed futuro | Registro ✅; runtime NOT_IMPLEMENTED |
| 22 | Lua (Defold / Solar2D / Cocos2d-x) | Lua source/bytecode; assets; libs engine | LANG-09 | Lua embed + bridges específicos futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 23 | Python (Kivy / Buildozer / Chaquopy / BeeWare) | Python source/bytecode; libs; payloads compactados | LANG-09 | CPython/PyO3 + bridges específicos futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 24 | Haxe (OpenFL / Heaps) | DEX ou lib/<abi>/*.so | LANG-01+LANG-02 | VM DEX ou ELF/JNI; bridge Haxe futuro | Registro ✅; runtime NOT_IMPLEMENTED |
| 25 | Qt / QML (C++) | libQt*.so; QML assets/recursos | LANG-09 | Qt headless + JNI futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 26 | Avalonia (C#) | assemblies Avalonia; runtime managed | LANG-07 | Mono + render próprio futuros | Registro ✅; runtime NOT_IMPLEMENTED |
| 27 | B4X / App Inventor / Thunkable | classes*.dex | LANG-01+LANG-11 | VM DEX; fixtures específicas futuras | Registro ✅; runtime DEX parcial |
| 28 | WebAssembly in-app | assets/*.wasm (ou containers opacos) | LANG-12 | Runtime WASM + bridge JNI/JS futuros | Registro ✅; runtime NOT_IMPLEMENTED |

Regras, dependências e limites em [engine-registry.md](engine-registry.md); fonte: `data/engines.toml`.

## Superfície android.* (Apêndice C — registro, não execução)

**106 superfícies em 17 domínios** (supported 4, partial 33, stub 11, not_implemented 58). Fonte executável: `data/api-coverage.toml`; relatório completo em [api-coverage.md](api-coverage.md), programa literal e bloqueadores em [appendix-c-literal-completion.md](appendix-c-literal-completion.md), consulta via `rd api-coverage` e classificação estática no MCP `get_api_usage`. `supported` é apenas o subconjunto documentado de cada superfície — nunca o pacote AOSP inteiro; cobertura de execução continua sendo `get_coverage`.

| Domínio | Superfícies/pacotes principais | Status das superfícies | Módulos |
|---|---|---|---|
| App/lifecycle | `android.app`, `android.app.admin`, `android.app.backup`, `android.app.job`, `android.app.slice`, `android.content`, `android.content.pm`, `android.content.res` | partial 5 · not_implemented 3 | `none`, `rd-vm/android-pm`, `rd-vm/android-res`, `rd-vm/framework`, `rd-vm/m8` |
| View/UI | `android.view`, `android.view.accessibility`, `android.view.animation`, `android.view.inputmethod`, `android.view.textservice`, `android.view.translation`, `android.webkit`, `android.widget` | partial 5 · stub 1 · not_implemented 2 | `none`, `rd-vm/android-sysservices`, `rd-vm/classpath`, `rd-vm/m9`, `rd-vm/view-layout` |
| Graphics | `android.graphics`, `android.graphics.drawable`, `android.graphics.pdf`, `android.opengl`, `android.renderscript` | partial 2 · not_implemented 3 | `none`, `rd-vm/graphics` |
| Text | `android.text`, `android.text.format`, `android.text.method`, `android.text.style`, `android.text.util` | partial 5 | `rd-vm/android-sysservices`, `rd-vm/android-text`, `rd-vm/graphics` |
| OS/IPC | `android.os`, `android.os.Binder`, `android.os.Build`, `android.os.Bundle`, `android.os.Parcel`, `android.os.Parcelable`, `android.util` | supported 2 · partial 5 | `rd-vm/android-build`, `rd-vm/android-util`, `rd-vm/bundle`, `rd-vm/framework`, `rd-vm/parcel` |
| Database | `android.database`, `android.database.sqlite`, `android.provider`, `android.provider.Settings` | supported 1 · partial 2 · stub 1 | `rd-vm/content`, `rd-vm/jsqlite` |
| Net | `android.net`, `android.net.Uri`, `android.net.http`, `android.net.ipsec.ike`, `android.net.nsd`, `android.net.sip`, `android.net.wifi` | supported 1 · partial 1 · not_implemented 5 | `none`, `rd-vm/android-uri`, `rd-vm/framework` |
| Media | `android.media`, `android.media.audiofx`, `android.media.browse`, `android.media.midi`, `android.media.projection`, `android.media.session`, `android.media.tv` | not_implemented 7 | `none` |
| Camera | `android.hardware.Camera`, `android.hardware.camera2`, `android.hardware.camera2.params` | stub 2 · not_implemented 1 | `none`, `rd-vm/android-util`, `rd-vm/classpath` |
| Hardware | `android.hardware`, `android.hardware.biometrics`, `android.hardware.display`, `android.hardware.fingerprint`, `android.hardware.hdmi`, `android.hardware.input`, `android.hardware.soundtrigger`, `android.hardware.usb`, `android.location` | partial 1 · stub 1 · not_implemented 7 | `none`, `rd-vm/android-display`, `rd-vm/classpath` |
| Telephony | `android.telecom`, `android.telephony`, `android.telephony.cellbroadcast`, `android.telephony.euicc`, `android.telephony.gsm`, `android.telephony.mbms` | not_implemented 6 | `none` |
| Connectivity | `android.bluetooth`, `android.bluetooth.le`, `android.companion`, `android.nfc`, `android.nfc.cardemulation`, `android.nfc.tech` | not_implemented 6 | `none` |
| Security | `android.se.omapi`, `android.security`, `android.security.identity`, `android.security.keystore`, `android.service.credentials` | stub 1 · not_implemented 4 | `none`, `rd-vm/jsecurity` |
| Speech/ML | `android.service.voice`, `android.speech`, `android.speech.tts`, `android.view.textclassifier` | stub 1 · not_implemented 3 | `none`, `rd-vm/classpath` |
| Services ext | `android.service.autofill`, `android.service.carrier`, `android.service.chooser`, `android.service.dreams`, `android.service.media`, `android.service.notification`, `android.service.quicksettings`, `android.service.textservice`, `android.service.wallpaper` | partial 1 · stub 1 · not_implemented 7 | `none`, `rd-vm/classpath`, `rd-vm/m9` |
| System UI | `android.accessibilityservice`, `android.app.usage`, `android.appwidget`, `android.inputmethodservice`, `android.print` | partial 3 · not_implemented 2 | `none`, `rd-vm/m9` |
| Misc | `android.accounts`, `android.animation`, `android.gesture`, `android.icu`, `android.mtp`, `android.preference`, `android.system`, `android.transition` | partial 3 · stub 3 · not_implemented 2 | `none`, `rd-vm/android-sysservices`, `rd-vm/classpath`, `rd-vm/framework`, `rd-vm/intrinsics` |

## Dados versionados (M0)

- permissions.toml: **175** permissões + 13 roles (Apêndice B)
- devices.toml: **6** perfis (piso: `moto-e5`)
- behavior-switches.toml: **25** comutadores targetSdk 26→36 (PRF-07)
- api-coverage.toml: **106** superfícies android.* em 17 domínios (Apêndice C)

## Compatibilidade por módulo (checklist PARTE 0)

- Núcleo (0.0): parcial (CORE-01..06 M0; rd-dex M1; VM M2 — métodos puros executam)
- Framework (M3): Activity lifecycle + views host + Handler/Looper + touch + **LayoutInflater de layout XML real** (setContentView(I): resid → @layout/key no arsc → AXML do APK → árvore host; attrs id/text/orientation/visibility/enabled/onClick/layout_height; setText(I), getString(I), findViewById) — classes de view sem host (ImageView/…) e configs específicas (land/locale) respondem tipadas
- Render/agente: render headless, árvore acessível e MCP já disponíveis em subconjuntos documentados; APIs ausentes retornam `NOT_IMPLEMENTED(module_id)`
