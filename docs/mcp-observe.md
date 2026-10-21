# Optional, on-demand screen observation (headless by default)

The standard `rd mcp` session is headless: no desktop window, live HTTP viewer,
or continuous render loop starts by default. The software screen model is
recorded only when a screen-facing operation requests a frame; ordinary MCP
actions mark the cached view dirty but do not continuously render or poll.

Explicit requests such as `observe`, `tap_text`, `screenshot`, a screen-resource
read, or `observe: true` on an action trigger a refresh when needed. MCP screen
subscriptions (`resources/subscribe`) and the `rd mcp --live` viewer are
separate opt-ins; while enabled, they check for screen changes after app calls
and publish updates only when visible content changes. An unchanged frame is
reused from cache.

A requested frame records the app's software display list (`android.graphics.Canvas`
/ `RenderNode`) and, when enabled, accessibility-provider semantics. This gives
agents structured screen data without requiring a desktop display or a PNG.

| Surface | For | What it gives (when requested) | Cost when requested (Comprix, release, 720×1440) |
|---|---|---|---|
| `observe` tool | agents | explicit query for visible texts, tap targets, icons, frame `version`, and diffs; refreshes only if dirty | 45 ms for a new frame · **0.1–0.4 ms** unchanged-cache read |
| `tap_text` tool | agents | explicit tap by visible label (`"Continuar"`, `"Ajustes"`) instead of coordinates | 25–180 ms (tap + app work) |
| `observe: true` on `launch/tap/tap_text/swipe/type_text/key/wait/wait_for` | agents | opt-in summary appended to that action result | +45 ms |
| MCP resources `rustydroid://screen` (JSON), `rustydroid://screen.txt`, `rustydroid://screen.png`, `rustydroid://logs` | MCP clients | explicit `resources/read`; optional `resources/subscribe` pushes only after app calls when visible content changes | — |
| `rd mcp --live 0.0.0.0:7878` (or `RD_MCP_LIVE`) | humans | opt-in browser page with the frame, screen text, tap-target overlay, last tool + latency; long-poll push | PNG encoded only when the version changes |
| `screenshot` | both | explicit PNG request, cached per frame version and deflate-compressed (about 190 KB vs 3.1 MB) | ~75 ms fresh · 1.4 ms repeated |

These are measured costs for explicit requests in that test setup, not continuous background rendering. Provider semantics can add substantial latency; see [Accessibility labels](#accessibility-labels-round-8).

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

Tools that can run app code mark the screen dirty. Read-only queries do not;
an explicit screen request may still refresh a dirty frame. Dirty is only cache
invalidation; it does not start a render. A fresh frame is recorded
lazily by an explicit `observe`, `tap_text`, `screenshot`, screen-resource read,
action with `observe: true`, or an active subscription/live viewer after an app
call. Without one of those requests/watchers, default `rd mcp` does not refresh
or poll the screen in the background. The PNG is cached per frame version. A
finished activity reports `finished: true` and an empty screen — never a stale
frame.

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

## Accessibility labels (round 8)

`advanced-components` is enabled by default, so provider traversal support is
included in standard builds. The traversal itself runs only during a requested
fresh screen observation (or an opted-in subscription/live refresh after an
app call); it is not a continuous background cost. It calls saved standard
View accessibility delegates/providers, executes the app's real DEX
`createAccessibilityNodeInfo`, and traverses virtual child IDs; no Comprix
class names or icon positions are hardcoded. Mutable node text,
`contentDescription`, screen bounds, child IDs and actions support Compose's
provider. Described nodes with bounds augment the frame targets. For example,
Comprix exposes **Buscar listas**, **Minhas lojas**, and **Voltar**; tapping
`Minhas lojas` by name opens that screen.

Traversal is bounded at 2,048 nodes. Because it executes guest code, it can be
much more expensive than the historical round-6 draw-only timings above; heavy
navigation can take seconds when a fresh observation is requested. Provider
errors are surfaced in the screen's error/warning field (currently labelled
`draw_error`), not silently ignored. Dialogs use the same provider path,
including ContextThemeWrapper base-context delegation and bounded rectilinear
Region clipping.

## Limits (honest)

- Images with no drawn text or accessibility description still appear only
  as icon centres (`-- icons @ center`). This is not OCR or icon recognition.
- This is provider-node traversal, not a complete Android accessibility
  service framework or complete Region API. Boolean Region operations have
  rectangle/work limits and return structured errors on exhaustion.
- The virtual clock only advances inside tools (`wait`, actions): the live
  viewer shows the latest frame between calls; it does not animate on its own
  (determinism is a feature for agents).
- Text inside images rasterized from fonts at runtime is found; text baked
  into PNG assets is not (no OCR).
