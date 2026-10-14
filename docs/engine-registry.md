# Registro de engines — Apêndice G

> **GERADO** por `cargo xtask engine-report` a partir de `data/engines.toml`.
> Escopo: **registro/detecção dos 28 itens**, não implementação de runtimes.

DEX/ELF não identificam a linguagem de origem. `signature` é uma
heurística de nomes/manifest, não validação binária; `candidate` é
compatibilidade de formato; `not_detected` não exclui um framework.
Um APK híbrido pode ter várias assinaturas. Nenhuma autoriza execução.

## Consulta

```sh
cargo build -p rd-cli
./target/debug/rd engine list --json
./target/debug/rd engine detect app.apk --json
./target/debug/rd engine check flutter --json # exit 2, NOT_IMPLEMENTED
./target/debug/rd inspect app.apk --json     # campo engines
```

A tool MCP `app_info` expõe o mesmo relatório em `engines`; não há
nova tool. O catálogo/relatório funciona no build mínimo, sem features
opcionais. `--data` / `RD_DATA_DIR` explícitos exigem `engines.toml`;
arquivo ausente, inválido ou schema desconhecido retorna erro tipado,
nunca fallback silencioso. Sem override, o catálogo embutido permite
distribuir o binário sozinho. MCP usa o catálogo embutido.

## Matriz completa

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

## Features e dependências

Features selecionam contratos, **não compilam/baixam engines**. Todas são
opt-in, sem features default. Dependências são transitivas e propagadas
pelo `rd-cli`/`rd-agent`; o catálogo permanece completo em qualquer build.
A seleção de runtimes no manifest/spec não mudou nesta entrega.

| Módulo | Cargo feature | Dependências diretas | Nota |
|---|---|---|---|
| LANG-01 | `lang-01` | — | DEX compartilhado; VM existente tem cobertura parcial. |
| LANG-02 | `lang-02` | LANG-01 | ELF/JNI; loaders e JNI completos ainda não implementados. |
| LANG-03 | `lang-03` | LANG-02 | Rust NDK reutiliza a fronteira ELF/JNI. |
| LANG-04 | `lang-04` | LANG-02 | Flutter: Dart AOT e engine/render próprio pendentes. |
| LANG-05 | `lang-05` | LANG-02 | JS/RN/NativeScript: engine JS e bridges pendentes. |
| LANG-06 | `lang-06` | LANG-01 | WebView/bridges Android; não baixa páginas na detecção. |
| LANG-07 | `lang-07` | LANG-02 | Mono/IL e render de MAUI/Avalonia pendentes. |
| LANG-08 | `lang-08` | LANG-07 | União das rotas Unity Mono e IL2CPP; não implica suporte gráfico. |
| LANG-09 | `lang-09` | LANG-02 | Pontes independentes de engines; não embute engines no registro. |
| LANG-10 | `lang-10` | LANG-01 | Shared DEX é possível; render Compose MP exige trabalho adicional. |
| LANG-11 | `lang-11` | LANG-01 | Java gerado; só o subconjunto DEX já disponível. |
| LANG-12 | `lang-12` | LANG-01 | WASM in-app; bridge JNI/JS escolhida na implementação futura. |

Exemplo: `cargo build -p rd-cli --no-default-features --features lang-04`
seleciona LANG-04 → LANG-02 → LANG-01. O campo `feature_enabled` confirma
seleção, enquanto `registry_adapter_status` continua `NOT_IMPLEMENTED`.
LANG-08 representa a união Mono/IL2CPP; selecionar o módulo inclui LANG-07.
Compose/render, WebView, JNI, NDK e bridges reais continuam trabalho futuro.

## Regras de detecção e rotas

`all` exige todas as pistas; `any` exige pelo menos uma quando não vazio.
Regras só usam nomes ZIP canônicos (case-sensitive) e classes/metadados
de componentes do manifest. Bibliotecas nativas exigem exatamente
`lib/<abi>/<arquivo>.so`; DEX exige `classes.dex` ou `classesN.dex` (N ≥ 2).
Nenhum asset/lib é descomprimido, extraído, carregado ou executado.

### 1 — `java`

- **Rota existente/futura:** `rd-vm`
- **Runtime:** DEX parcial (rd-vm); adaptador do registro ausente
- **Limite:** DEX tem suporte parcial, não execução universal de Java.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.

### 2 — `kotlin`

- **Rota existente/futura:** `rd-vm`
- **Runtime:** DEX parcial (rd-vm); adaptador do registro ausente
- **Limite:** Dependências Kotlin não provam que o app foi escrito em Kotlin.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.

### 3 — `kotlin-multiplatform`

- **Rota existente/futura:** `rd-engine-compose-mp`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Não é possível distinguir KMP/Compose MP apenas pela presença de DEX.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.
- **candidate:** all=[`suffix:.klib`]; any=[] — klib é pista, não prova render Compose Multiplatform.

### 4 — `jvm-dynamic`

- **Rota existente/futura:** `rd-vm`
- **Runtime:** DEX parcial (rd-vm); adaptador do registro ausente
- **Limite:** invoke-custom/method handles e runtimes dinâmicos não são declarados completos.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.

### 5 — `c-ndk`

- **Rota existente/futura:** `rd-ndk/rd-jni`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** C não pode ser atribuído a uma .so genérica.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 6 — `cpp-ndk`

- **Rota existente/futura:** `rd-ndk/rd-jni`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** libc++_shared.so indica dependência STL, não a linguagem do app.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 7 — `rust-ndk`

- **Rota existente/futura:** `rd-ndk/rd-jni`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Nome .so e símbolos removidos não provam Rust.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 8 — `go-mobile`

- **Rota existente/futura:** `rd-engine-gomobile`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Marcador gojni é uma assinatura heurística, não análise de símbolos.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.
- **signature:** all=[`native:libgojni.so`]; any=[] — Marcador de bind gomobile.

### 9 — `objective-c-ndk`

- **Rota existente/futura:** `rd-ndk/rd-jni`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Rota rara; uma .so genérica só produz candidato.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 10 — `zig-llvm`

- **Rota existente/futura:** `rd-ndk/rd-jni`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** ELF não preserva necessariamente a linguagem do compilador.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 11 — `flutter`

- **Rota existente/futura:** `rd-engine-flutter`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Não interpreta Dart AOT nem converte widgets nesta entrega.
- **signature:** all=[`native:libflutter.so`, `prefix:assets/flutter_assets/`]; any=[] — Engine Flutter acompanhado de assets.
- **candidate:** all=[`native:libapp.so`]; any=[] — libapp.so isolada não prova Flutter.

### 12 — `react-native`

- **Rota existente/futura:** `rd-engine-react-native`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Hermes sozinho não prova React Native; não valida bytecode.
- **signature:** all=[`path:assets/index.android.bundle`]; any=[`native:libreactnativejni.so`, `native:libreactnative.so`] — Lib React Native acompanhada de bundle.
- **candidate:** all=[`path:assets/index.android.bundle`]; any=[] — Bundle convencional isolado é apenas candidato.
- **candidate:** all=[`native:libhermes.so`]; any=[] — Hermes pode ser usado fora de React Native.

### 13 — `cordova`

- **Rota existente/futura:** `rd-engine-webview`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Assets web sem marcador específico são candidatos, não framework confirmado.
- **signature:** all=[`prefix:assets/www/`, `path:assets/www/cordova.js`]; any=[] — www com bootstrap Cordova.
- **signature:** all=[`prefix:assets/public/`, `path:assets/capacitor.config.json`]; any=[] — Assets web com configuração Capacitor.
- **candidate:** all=[`prefix:assets/www/`]; any=[] — Assets HTML compatíveis com engines web.
- **candidate:** all=[`prefix:assets/public/`]; any=[] — Assets HTML compatíveis com engines web.

### 14 — `twa-pwa`

- **Rota existente/futura:** `rd-engine-webview`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Ausência de payload local não descarta TWA/PWA.
- **signature:** all=[]; any=[`class:com.google.androidbrowserhelper.trusted.`, `class:android.support.customtabs.trusted.`, `metadata:android.support.customtabs.trusted.DEFAULT_URL`] — Launcher/metadata de Trusted Web Activity.

### 15 — `nativescript`

- **Rota existente/futura:** `rd-engine-nativescript`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Nenhum bridge JS/Android é executado pelo registro.
- **signature:** all=[`native:libNativeScript.so`, `prefix:assets/app/`]; any=[] — Lib NativeScript e payload app.

### 16 — `tauri`

- **Rota existente/futura:** `rd-engine-tauri`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Sem configuração Tauri, Rust+WebView não identifica Tauri unicamente.
- **signature:** all=[`path:assets/tauri.conf.json`]; any=[`prefix:assets/www/`, `prefix:assets/public/`] — Config Tauri embarcada acompanhada de assets web.
- **candidate:** all=[`native-so`]; any=[`prefix:assets/www/`, `prefix:assets/public/`] — Combinação de lib nativa e assets web é ambígua.

### 17 — `dotnet-maui`

- **Rota existente/futura:** `rd-engine-mono`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Assembly stores comprimidos/renomeados podem não ter .dll visível.
- **signature:** all=[`prefix:assemblies/`]; any=[`native:libmonodroid.so`, `native:libmonosgen-2.0.so`] — Payload managed acompanhado de runtime Mono.
- **candidate:** all=[`prefix:assemblies/`]; any=[] — Assemblies indicam payload managed, não MAUI unicamente.

### 18 — `unity-mono`

- **Rota existente/futura:** `rd-engine-unity`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Identifica a rota Mono por marcadores, não executa o jogo.
- **signature:** all=[`native:libunity.so`, `path:assets/bin/Data/Managed/Assembly-CSharp.dll`]; any=[] — Unity com Assembly-CSharp managed.
- **candidate:** all=[`path:assets/bin/Data/Managed/Assembly-CSharp.dll`]; any=[] — Assembly-CSharp isolado é apenas candidato.

### 19 — `unity-il2cpp`

- **Rota existente/futura:** `rd-engine-unity`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Não interpreta IL2CPP nem valida versão de global-metadata.
- **signature:** all=[`native:libil2cpp.so`, `path:assets/bin/Data/Managed/Metadata/global-metadata.dat`]; any=[] — IL2CPP acompanhado de metadata.
- **candidate:** all=[`native:libil2cpp.so`]; any=[] — Lib IL2CPP sem metadata é assinatura incompleta.

### 20 — `unreal`

- **Rota existente/futura:** `rd-engine-unreal`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Dados também podem vir de OBB fora do APK.
- **signature:** all=[]; any=[`native:libUE4.so`, `native:libUnreal.so`] — Marcador de biblioteca Unreal.

### 21 — `godot`

- **Rota existente/futura:** `rd-engine-godot`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** PCK isolado é candidato; C# demanda Mono além de Godot.
- **signature:** all=[`suffix:.pck`]; any=[`native:libgodot_android.so`, `native:libgodot.so`] — Biblioteca Godot e pacote/descritor do projeto.
- **signature:** all=[`path:assets/project.binary`]; any=[`native:libgodot_android.so`, `native:libgodot.so`] — Biblioteca Godot e project.binary.
- **candidate:** all=[`suffix:.pck`]; any=[] — Pacote .pck isolado não prova Godot.

### 22 — `lua-engines`

- **Rota existente/futura:** `rd-engine-lua`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Bytecode/containers podem ser comprimidos; .lua é apenas candidato.
- **signature:** all=[]; any=[`native:libcorona.so`, `native:libdefold.so`] — Marcador de engine Lua.
- **candidate:** all=[`suffix:.lua`]; any=[] — Payload Lua não distingue Defold/Solar2D/Cocos2d-x.
- **candidate:** all=[`suffix:.luac`]; any=[] — Bytecode Lua apenas por extensão; conteúdo não validado.
- **candidate:** all=[`native:libcocos2dcpp.so`]; any=[] — Cocos2d-x não implica uso de Lua.

### 23 — `python-engines`

- **Rota existente/futura:** `rd-engine-python`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Não descompacta private.mp3/stdlib.zip nem executa Python.
- **signature:** all=[`prefix:assets/chaquopy/`]; any=[] — Marcador de payload Chaquopy.
- **candidate:** all=[]; any=[`suffix:.py`, `suffix:.pyc`] — Payload Python identificável por extensão.
- **candidate:** all=[`path:assets/private.mp3`]; any=[] — Container convencional python-for-android; conteúdo não verificado.

### 24 — `haxe`

- **Rota existente/futura:** `rd-engine-haxe`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Formato DEX/ELF não identifica Haxe; ambas as rotas são candidatos.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.
- **candidate:** all=[`native-so`]; any=[] — ELF é compartilhado: não identifica a linguagem de origem nem valida ABI/payload.

### 25 — `qt-qml`

- **Rota existente/futura:** `rd-engine-qt`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** QML pode estar em recursos compilados, fora de assets visíveis.
- **signature:** all=[`suffix:.qml`]; any=[`native-prefix:libQt5Core`, `native-prefix:libQt6Core`] — Runtime Qt e payload QML.
- **candidate:** all=[]; any=[`native-prefix:libQt5Core`, `native-prefix:libQt6Core`] — Runtime Qt por nome de lib.
- **candidate:** all=[`suffix:.qml`]; any=[] — QML isolado é candidato.

### 26 — `avalonia`

- **Rota existente/futura:** `rd-engine-avalonia`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Assemblies em stores opacos podem não ser identificáveis.
- **signature:** all=[]; any=[`path:assemblies/Avalonia.Android.dll`, `path:assemblies/Avalonia.Base.dll`] — Assembly Avalonia embarcada.
- **candidate:** all=[`prefix:assemblies/`]; any=[] — Payload managed genérico não prova Avalonia.

### 27 — `generated-java`

- **Rota existente/futura:** `rd-vm`
- **Runtime:** DEX parcial (rd-vm); adaptador do registro ausente
- **Limite:** Java gerado não é distinguível de Java escrito à mão apenas pelo formato.
- **candidate:** all=[`root-dex`]; any=[] — DEX é compartilhado: não identifica a linguagem de origem.

### 28 — `wasm`

- **Rota existente/futura:** `rd-engine-wasm`
- **Runtime:** NOT_IMPLEMENTED
- **Limite:** Extensão .wasm não valida magic/version/imports nem prova uso em runtime.
- **candidate:** all=[`suffix:.wasm`]; any=[] — Arquivo com extensão WASM; payload não validado.

## Contrato e erros

`EngineAdapter` define `parse(Artifact) -> Process`, `event(process, event)`
e `syscall(process, call)`. Os 28 adaptadores do registro são indisponíveis:
toda operação retorna `{code, cause, suggestion, module_id}` com
`code: NOT_IMPLEMENTED` e `module_id: LANG-xx`. Nenhum processo fictício é
retornado, mesmo com `--all-features`. A cobertura DEX existente pertence a
`rd-vm`, pelas rotas `rd vm`/`rd app`, não a estes adaptadores.

| Código | Situação |
|---|---|
| `NOT_IMPLEMENTED` | Operação de runtime/contrato ainda ausente (CLI exit 2) |
| `UNKNOWN_ENGINE` | ID não registrado (exit 1) |
| `INVALID_ENGINE_CATALOG` | Schema, IDs, módulos, ciclos, regras ou status inválidos |
| `CATALOG_TOO_LARGE` | Catálogo maior que 1 MiB |
| `DETECTION_LIMIT` | Mais de 100.000 entradas ZIP |
| `IO_ERROR` | Arquivo de catálogo/APK inacessível |

Diagnósticos do relatório: `DUPLICATE_ZIP_ENTRY`,
`NON_CANONICAL_ARTIFACT_PATH`, `UNKNOWN_ABI`, `AMBIGUOUS_SOURCE_LANGUAGE`.
Nomes duplicados contam uma vez. Evidências/diagnósticos contêm no máximo
4 amostras por predicado/diagnóstico, mantendo a contagem total.
Todos os 28 IDs aparecem, inclusive `not_detected`; ordem e amostras são
determinísticas. `execution_claimed: false` e
`artifact_validation: not-performed` são contratos explícitos.

## Testes e definição de conclusão

- `tests/engines/signatures.json`: exatamente 28 casos golden de DETECÇÃO,
  um por ID. Fixtures APK/AXML/ZIP são geradas em memória; nenhum binário
  é commitado. Testes cobrem híbridos, pistas incompletas, ambiguidades,
  paths hostis, duplicatas, teto de entradas e amostragem.
- Tests de contrato exercitam parse/event/syscall para os 28 itens: sempre
  erro tipado. Builds mínimo, features individuais e todas são independentes.
- `cargo xtask engine-report` regenera este arquivo;
  `cargo xtask engine-spec` regenera apenas a tabela marcada no Apêndice G;
  `cargo xtask compat-report` atualiza o placar oficial. Testes barram drift.
- Integração OSS explícita: baixe um APK com `golden-apks/fetch.sh` e rode
  `RD_TEST_APK=/caminho/app.apk cargo test -p rd-engine-registry --test registry
  real_oss_apk_inventory -- --ignored`. Sem a variável/arquivo, o teste falha;
  não há falso sucesso por pular a fixture. No teste normal ele é `ignored`.

**Registro/detecção concluídos ≠ runtime concluído.** Uma engine só pode
mudar de `NOT_IMPLEMENTED` após crate/adaptador real feature-gated, ADR,
parse/event/syscall funcionais, golden flow com APK OSS e gate Moto E5.
M9/M11 continuam pendentes. Veja [ADR 003](adr/003-appendix-g-engine-registry.md)
e [ROADMAP](ROADMAP.md).
