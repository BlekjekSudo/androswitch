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
| `rd mcp --live 0.0.0.0:7878` (or `RD_MCP_LIVE`) | humans | browser page with the RustyDroid-rendered frame, the screen text, tap-target overlay (checkbox "tap targets"), package/activity, last tool + latency, frame version; long-poll push (`/state.json?since=N`, ≤20 s) | PNG served from `/frame.png` (see the routes below) |
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

## Accessibility labels (round 8)

With `advanced-components` (enabled by default), the refresh path also calls
saved standard View accessibility delegates/providers. It executes the app's
real DEX `createAccessibilityNodeInfo` and traverses virtual child IDs; no
Comprix class names or icon positions are hardcoded. Mutable node text,
`contentDescription`, screen bounds, child IDs and actions support Compose's
provider. Described nodes with bounds augment the frame targets. For example,
Comprix exposes **Buscar listas**, **Minhas lojas**, and **Voltar**; tapping
`Minhas lojas` by name opens that screen.

Traversal is bounded at 2,048 nodes. It executes guest code and can be much
more expensive than the historical round-6 draw-only timings above; heavy
navigation can take seconds. Provider errors are surfaced in the screen's
error/warning field (currently labelled `draw_error`), not silently ignored.
Dialogs use the same provider path, including ContextThemeWrapper base-context
delegation and bounded rectilinear Region clipping.

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

## Live viewer: routes and page (`rd mcp --live`)

Served by `live_serve_conn` in `crates/rd-agent/src/observe.rs` (`observe.rs:710`; std-only HTTP, one thread per connection, `Connection: close`). The address comes from `rd mcp --live <addr>` or `RD_MCP_LIVE` (`crates/rd-cli/src/main.rs:590`, `crates/rd-agent/src/mcp.rs:3195`). The viewer is started in `serve_stdio_with` (`crates/rd-agent/src/mcp.rs:3201-3212`) and logs `[rd mcp] live viewer: http://<addr>/` to stderr.

| Route | Response | Notes |
|---|---|---|
| `/` or `/index.html` | HTML page (`LIVE_HTML`, `observe.rs:765-796`) | Self-contained page: frame canvas, tap-target overlay, screen text, status pills |
| `/state.json` | JSON state | Long-poll: `?since=N` blocks until the frame version passes `N`, up to 20 s (`observe.rs:720-742`). Without `since` it returns at once |
| `/frame.png` | `image/png` | `204 No Content` while no frame has been recorded yet (`observe.rs:744-749`) |
| any other path | `404 Not Found` | Empty body |

The page's header shows the package and activity, the frame version (`frame vN`), and the last tool with its latency (`last: <tool> <ms> ms`, `observe.rs:791`). It shows the screen text and a `draw pass failed` message when the host draw fails. The overlay is a checkbox labelled "tap targets" (`observe.rs:779`).

Successful (200) responses send `Cache-Control: no-store`. All responses close the connection.
