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
| M8 | Compat real | ⬜ |
| M9 | Componentes avançados + NDK | ⬜ |
| M10 | Dispositivos virtuais | ⬜ |
| M11 | Engines | ⬜ |
| M12 | Ecossistema GH | ⬜ |

## Crates

| Crate | Fase | Escopo |
|---|---|---|
| `rd-apk` | M0 | ZIP + AXML + arsc + assinaturas + manifest model completo |
| `rd-framework` | M0 | fd-permissions (motor completo + gating maxSdk/since_api) · fd-devices · fd-behavior |
| `rd-cli` | M0–M6 | inspect · perm · device · behavior · dex disasm · vm exec · app run · mcp · test (suítes JUnit, M6) |
| `rd-dex` | M1 | parser DEX completo + disassembler smali validado vs baksmali + fuzz targets |
| `rd-vm` | M2–M3 | interpretador Dalvik + golden harness vs JVM + framework host headless (Activity/View/Handler/Looper) + LayoutInflater AXML/ARSC |
| `rd-testsupport` | M0–M6 | fixtures de teste (APK/DEX/AXML/ARSC sintéticos; dev-only, fora do binário) |
| `rd-render` | M4 | UI dump uiautomator + screenshot headless (software renderer determinístico + PNG sem deps); winit/web são stub → M11 |
| `rd-agent` | M5–M6 | MCP v1 (30 tools + batch + policy sandbox) · executor de suítes `rd test` (JSON → JUnit XML, M6) |
| `rd-engine-registry` | Apêndice G | registro/detecção dos 28 itens; features/contratos; NÃO executa engines |
| `rd-jni` | M3+ | stub NOT_IMPLEMENTED |
| `rd-ndk` | M9 | stub NOT_IMPLEMENTED |

## Suítes automatizadas (`rd test`, M6)

| Suíte | Arquivo | Cenários | Passos |
|---|---|---|---|
| contract-no-app | `tests/suites/contract-no-app.json` | 8 | 10 |

Executar: `rd test tests/suites/<arquivo>.json --junit <saida>.xml` (formato em `docs/suite-format.md`).

## Engines (Apêndice G — registro/detecção, não execução)

**28 registros completos; 12 módulos opt-in.** Features não implementam runtimes; todos os adaptadores do registro retornam `NOT_IMPLEMENTED(LANG-xx)`. DEX parcial é da VM existente. M9/M11 continuam pendentes. Detecção golden não é golden de execução.

| # | Linguagem/Framework | Artefato no APK | Módulo | Estratégia | Status |
|---|---|---|---|---|---|
| 1 | Java | classes*.dex | LANG-01 | VM DEX própria | Registro ✅; runtime DEX parcial |
| 2 | Kotlin | classes*.dex | LANG-01 | VM DEX; coroutines/threads dependem da cobertura da VM | Registro ✅; runtime DEX parcial |
| 3 | Kotlin Multiplatform / Compose MP | DEX; recursos Compose; klib nem sempre é distribuída | LANG-10 | Shared DEX + futuro render Compose MP | Registro ✅; runtime NOT_IMPLEMENTED |
| 4 | Scala / Clojure / Groovy | classes*.dex | LANG-01 | VM DEX; invocações dinâmicas conforme cobertura | Registro ✅; runtime DEX parcial |
| 5 | C (NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime NOT_IMPLEMENTED |
| 6 | C++ (NDK) | lib/<abi>/*.so; STL opcional | LANG-02 | ELF + JNI + STL | Registro ✅; runtime NOT_IMPLEMENTED |
| 7 | Rust (cargo-ndk/xbuild) | lib/<abi>/*.so | LANG-03 | ELF + JNI | Registro ✅; runtime NOT_IMPLEMENTED |
| 8 | Go (gomobile) | lib/<abi>/*.so; wrappers bind | LANG-09 | ELF + JNI + gomobile bind | Registro ✅; runtime NOT_IMPLEMENTED |
| 9 | Objective-C/C (cross via NDK) | lib/<abi>/*.so | LANG-02 | ELF + JNI; runtime ObjC se necessário | Registro ✅; runtime NOT_IMPLEMENTED |
| 10 | Zig / outras linguagens LLVM | lib/<abi>/*.so | LANG-02 | ELF + JNI | Registro ✅; runtime NOT_IMPLEMENTED |
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

## Dados versionados (M0)

- permissions.toml: **175** permissões + 13 roles (Apêndice B)
- devices.toml: **6** perfis (piso: `moto-e5`)
- behavior-switches.toml: **25** comutadores targetSdk 26→36 (PRF-07)

## Compatibilidade por módulo (checklist PARTE 0)

- Núcleo (0.0): parcial (CORE-01..06 M0; rd-dex M1; VM M2 — métodos puros executam)
- Framework (M3): Activity lifecycle + views host + Handler/Looper + touch + **LayoutInflater de layout XML real** (setContentView(I): resid → @layout/key no arsc → AXML do APK → árvore host; attrs id/text/orientation/visibility/enabled/onClick/layout_height; setText(I), getString(I), findViewById) — classes de view sem host (ImageView/…) e configs específicas (land/locale) respondem tipadas
- Render/agente: `NOT_IMPLEMENTED(module_id)` estruturado (M4+)
