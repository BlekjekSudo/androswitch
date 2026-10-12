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
| `rd-jni` | M3+ | stub NOT_IMPLEMENTED |
| `rd-ndk` | M9 | stub NOT_IMPLEMENTED |

## Suítes automatizadas (`rd test`, M6)

| Suíte | Arquivo | Cenários | Passos |
|---|---|---|---|
| contract-no-app | `tests/suites/contract-no-app.json` | 6 | 7 |

Executar: `rd test tests/suites/<arquivo>.json --junit <saida>.xml` (formato em `docs/suite-format.md`).

## Dados versionados (M0)

- permissions.toml: **175** permissões + 13 roles (Apêndice B)
- devices.toml: **6** perfis (piso: `moto-e5`)
- behavior-switches.toml: **25** comutadores targetSdk 26→36 (PRF-07)

## Compatibilidade por módulo (checklist PARTE 0)

- Núcleo (0.0): parcial (CORE-01..06 M0; rd-dex M1; VM M2 — métodos puros executam)
- Framework (M3): Activity lifecycle + views host + Handler/Looper + touch + **LayoutInflater de layout XML real** (setContentView(I): resid → @layout/key no arsc → AXML do APK → árvore host; attrs id/text/orientation/visibility/enabled/onClick/layout_height; setText(I), getString(I), findViewById) — classes de view sem host (ImageView/…) e configs específicas (land/locale) respondem tipadas
- Render/agente: `NOT_IMPLEMENTED(module_id)` estruturado (M4+)
