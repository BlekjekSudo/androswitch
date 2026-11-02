# ADR 019 — Renderer M4: tiny-skia + shaping (spec) vs. rasterizador próprio atual

- **Status:** Proposto (2026-10) — aguarda decisão do mantenedor. Até lá, a decisão vigente continua sendo a da spec, e o M4 permanece ⚠️ parcial.

## Contexto

A spec (`RustyDroid.md`, "DECISÕES TÉCNICAS JÁ TOMADAS") fixa renderer software em `tiny-skia` e shaping em `rustybuzz` (UI-12). O DoD do M4 diz: "Seguir `tiny-skia` + shaping definido na especificação, ou aprovar ADR que substitua essa decisão".

A implementação atual (`crates/rd-render/src/shot.rs`) não usa `tiny-skia` nem `rustybuzz`:

- framebuffer RGB próprio 720×1440, retângulos por classe/estado;
- texto com um **atlas pré-rasterizado de DejaVu Sans 32 px** (`crates/rd-vm/assets/glyphs.bin`, gerado por `scripts/gen_glyph_atlas.py`), sem shaping e sem bidi;
- encoder PNG próprio (zlib STORED + crc32/adler32), zero dependências.

Isso é determinístico e testável, mas **não é fidelidade Android**: o Android real renderiza com Skia/FreeType. Os testes exatos de PNG sintético provam só o encoder e a pintura de retângulos, não a aparência de um app real.

Restrição a verificar antes de qualquer adoção: o workspace é MSRV 1.76 (`rust-msrv` no CI). Ainda não foi confirmado o MSRV de `tiny-skia` e de `rustybuzz` nas versões compatíveis com 1.76; o ADR 001 mostrou que o `rmcp` quebrava esse piso.

## Opções

1. **Adotar `tiny-skia` + `rustybuzz`** (cumprir a spec). Requer: confirmar MSRV 1.76 ou subir o MSRV (decisão vinculante, exige ADR próprio); medir custo de build e de memória no piso `moto-e5` (PRF-06, ≤ 512 MB de processo). Substitui o atlas por rasterização com `tiny-skia` e shaping com `rustybuzz`.
2. **Manter o rasterizador próprio e registrar este ADR como substituto** da decisão de `tiny-skia`. Ganho: zero dependências de runtime, determinismo total, sem risco de MSRV. Custo: não há shaping nem bidi (o atlas DejaVu é fonte real, mas pré-rasterizada); a fidelidade visual com Android depende de golden de referência, que ainda não existe. Exige que a spec e o README deixem de afirmar "tiny-skia".
3. **Híbrido:** rasterizador próprio para retângulos/PNG e `tiny-skia` apenas para texto/paths, atrás de um trait `Renderer`, com golden de referência para decidir a tolerância de pixel.

## Consequências (qualquer opção)

- Até a decisão, README e spec não podem afirmar que o renderer usa `tiny-skia` como se estivesse ativo. O README foi corrigido nesse sentido na mesma mudança que criou este ADR.
- Golden screenshots devem vir de APK real e de uma referência Android, com tolerância de pixel documentada; a opção escolhida define como essa referência é produzida.
- O M4 só volta a ✅ quando houver: (a) renderização de texto com fonte real ou ADR aprovado que a dispense; (b) golden de ≥5 APKs reais comparados a referência; (c) seleção de recursos por `DeviceConfig`.
