# Seeing the screen without screenshots (`observe`, resources, live viewer)

RustyDroid renders every frame itself (tiny-skia-style software raster of the
display list the app records through `android.graphics.Canvas` /
`RenderNode`). That means the runtime already knows **what is on screen as
data**, before any pixel exists. Round 6 exposes that knowledge to agents and
humans:

| Surface | For | What it gives | Cost (Comprix, release, 720×1440) |
|---|---|---|---|
| `observe` tool | agents | visible texts with bounds, tap targets (label + centre), icons, frame `version`, diff since last observe | 45 ms when a new frame must be recorded · **0.1–0.4 ms** when nothing changed |
| `tap_text` tool | agents | tap by visible label (`"Continuar"`, `"Ajustes"`) instead of coordinates | 25–180 ms (tap + app work) |
| `observe: true` on `launch/tap/tap_text/swipe/type_text/key/wait/wait_for` | agents | the action result carries the new screen summary → 1 round-trip per step | +45 ms |
| MCP resources `rustydroid://screen` (JSON), `rustydroid://screen.txt`, `rustydroid://screen.png`, `rustydroid://logs` | MCP clients | `resources/read` + `resources/subscribe` → `notifications/resources/updated` whenever visible content changes | — |
| `rd mcp --live 0.0.0.0:7878` (or `RD_MCP_LIVE`) | humans | browser page with the RustyDroid-rendered frame, the screen text, tap-target overlay, last tool + latency; long-poll push | PNG encoded only when the version changes |
| `screenshot` | both | PNG, now **cached per frame version** and **deflate-compressed** (adaptive row filters; ~190 KB instead of ~3.1 MB) | ~75 ms fresh · 1.4 ms repeated |

Before round 6, the only way to see a Compose app was `screenshot`
(0.3–1.6 s, ~3 MB base64 per call) — `ui_tree` shows one opaque
`AndroidComposeView` node.

## How the screen model is built (`rd-vm/src/screen_model.rs`)

1. The host draw pass (`draw_frame`) records the window display list (it runs
   anyway for screenshots).
2. `Engine::screen_model()` walks it with **exactly the rasterizer's rules**
   — matrix concatenation, clip intersection (op clip, `clipToBounds`,
   outlines), layer alpha, render nodes and bitmaps — but collects items
   instead of pixels:
   - `Text` ops → text runs with glyph-box bounds (`measure`, ascent/descent of
     the same glyph atlas the rasterizer uses); runs mostly clipped away or
     nearly transparent are dropped;
   - filled/stroked rects, rounded rects, ovals, paths, colour fills → boxes;
   - bitmaps (incl. Compose's vector-icon caches) → images.
3. Post-processing: painter's-order occlusion (an item fully under an opaque
   box drawn later is hidden — sheets and dialogs hide the screen behind
   them), merge of runs on one baseline, reading order, background detection,
   **targets** = smallest box (< 40 % of the screen) enclosing a label, except
   *containers* (a box with labels side by side on one row — navigation bars,
   toolbars) where each label is tapped at its own position.
4. A content fingerprint drives the frame `version`: it only changes when the
   visible content changes, so agents can poll cheaply and subscriptions fire
   only on real changes.
5. `rd-agent` merges the host view tree: clickable host views become targets
   (`source: "view"`), host-painted texts (TextView, Button…) are added.

It is toolkit-agnostic (Views, Compose, custom `onDraw`) because it reads what
the app *drew*, not an accessibility tree the app may not populate.

### Caching and invalidation

Any tool that can run app code (everything except `observe`, `ui_tree`,
`screenshot`, `logs`, asserts, info tools) marks the screen dirty. The next
observer records one frame; the PNG is cached per version. A finished activity
reports `finished: true` and an empty screen — never a stale frame.

## Robustness

- A runtime panic during a tool no longer kills `rd mcp`: the release profile
  unwinds (`panic = "unwind"`; there are no `extern "C"` frames — native code
  runs out of process) and `call_tool_isolated` turns the panic into an
  `INTERNAL_PANIC` tool error, ends the app and keeps the session.
- A failing draw pass is reported in `observe` (`draw_error`), in the
  `screenshot` result (warning text) and once in `logs`.

## Example (Comprix v1.6.0, after "Criar e adicionar itens")

```
frame v7 · Lbr/com/comprix/presentation/MainActivity; · 25 texts · 5 targets · 13 images
y51    [x113] Compras de janeiro
y233   [x34] Adição rápida
y466   [x30] Ex.: “2 kg de arroz, meia dúzia de ovos, 3 leite”
y542   [x76] Anotando preços em:   [x384] Escolher loja
y1074  [x166] Lista vazia por enquanto.
y1303  [x30] 0 itens   [x128] •   [x158] Total: R$ 0,00   [x529] 0/0 no carrinho
y1377  [x200] Comparar estabelecimentos
-- targets (label @ center x,y):
  "Escolher loja" @ 435,542
  "Adicionar loja" @ 546,704
  "Comparar estabelecimentos" @ 360,1377
-- changed since v6: +["Adição rápida", …] -["Minhas listas", …]
```

## Limits (honest)

- Labels come from drawn text: icon-only buttons appear as icon centres
  (`-- icons @ center`) without a name — Compose `contentDescription` lives in
  the semantics tree, which is not modelled yet.
- The virtual clock only advances inside tools (`wait`, actions): the live
  viewer shows the latest frame between calls; it does not animate on its own
  (determinism is a feature for agents).
- Text inside images rasterized from fonts at runtime is found; text baked
  into PNG assets is not (no OCR).
