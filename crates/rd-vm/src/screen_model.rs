// Screen model: a semantic, screenshot-free view of the last recorded frame.
//
// Included into `graphics.rs` (needs the private display-list types). The
// window display list recorded by the host draw pass is walked with exactly
// the transform / clip / alpha rules of `raster_ops`, but instead of painting
// pixels it collects what a human would see: text runs, filled boxes and
// images, each with window-space bounds. Because it reads what the app
// *drew*, it works for every UI toolkit (Views, Compose, custom canvases)
// without accessibility hooks — and it costs no rasterization or PNG encode.

/// Kind of a [`ScreenItem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenKind {
    /// A visible run of text.
    Text,
    /// A filled or stroked shape (rect, rounded rect, oval, path, colour fill).
    Box,
    /// A bitmap / cached vector image.
    Image,
}

impl ScreenKind {
    /// Stable lower-case name (`text`, `box`, `image`).
    pub fn name(self) -> &'static str {
        match self {
            ScreenKind::Text => "text",
            ScreenKind::Box => "box",
            ScreenKind::Image => "image",
        }
    }
}

/// One visible element of the frame, in window pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenItem {
    pub kind: ScreenKind,
    /// Text content (empty for boxes and images).
    pub text: String,
    /// Visible bounds `[left, top, right, bottom]` after clipping.
    pub bounds: [i32; 4],
    /// Effective colour (alpha already multiplied by layer alpha).
    pub argb: u32,
    /// Effective text size in pixels (0 for non-text items).
    pub size: f32,
    /// Filled shape (boxes): only filled opaque shapes hide what is under
    /// them — a stroked outline (outlined button) does not.
    pub filled: bool,
}

impl ScreenItem {
    /// Centre point of the visible bounds.
    pub fn center(&self) -> (i32, i32) {
        (
            (self.bounds[0] + self.bounds[2]) / 2,
            (self.bounds[1] + self.bounds[3]) / 2,
        )
    }
    fn area(&self) -> i64 {
        (self.bounds[2] - self.bounds[0]).max(0) as i64
            * (self.bounds[3] - self.bounds[1]).max(0) as i64
    }
    fn contains(&self, o: &[i32; 4]) -> bool {
        self.bounds[0] <= o[0] + 1
            && self.bounds[1] <= o[1] + 1
            && self.bounds[2] >= o[2] - 1
            && self.bounds[3] >= o[3] - 1
    }
}

/// A probable tap target: the smallest drawn box that encloses a label.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenTarget {
    pub label: String,
    pub bounds: [i32; 4],
}

impl ScreenTarget {
    pub fn center(&self) -> (i32, i32) {
        (
            (self.bounds[0] + self.bounds[2]) / 2,
            (self.bounds[1] + self.bounds[3]) / 2,
        )
    }
}

/// Semantic summary of the current frame.
#[derive(Clone, Debug, Default)]
pub struct ScreenModel {
    pub width: i32,
    pub height: i32,
    /// Text runs in reading order (top-to-bottom, left-to-right), adjacent
    /// runs on the same baseline merged.
    pub texts: Vec<ScreenItem>,
    /// Boxes that are neither full-screen backgrounds nor specks.
    pub boxes: Vec<ScreenItem>,
    pub images: Vec<ScreenItem>,
    /// Label → enclosing box (heuristic tap targets).
    pub targets: Vec<ScreenTarget>,
    /// Background colour (largest full-window fill), if any.
    pub background: Option<u32>,
    /// Raw display-list op count walked.
    pub ops: usize,
    /// Content fingerprint (stable for identical visible content).
    pub fingerprint: u64,
}

impl ScreenModel {
    /// Compact plain-text rendering: one line per visual text row, with the
    /// row's y and every run's x, plus targets. Designed for LLM context.
    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let mut rows: Vec<(i32, Vec<&ScreenItem>)> = Vec::new();
        for t in &self.texts {
            let cy = (t.bounds[1] + t.bounds[3]) / 2;
            match rows.iter_mut().find(|(y, _)| (cy - *y).abs() <= 6) {
                Some((_, v)) => v.push(t),
                None => rows.push((cy, vec![t])),
            }
        }
        rows.sort_by_key(|r| r.0);
        for (y, mut runs) in rows {
            runs.sort_by_key(|r| r.bounds[0]);
            let parts: Vec<String> = runs
                .iter()
                .map(|r| format!("[x{}] {}", r.bounds[0], r.text))
                .collect();
            let _ = writeln!(out, "y{y:<5} {}", parts.join("   "));
        }
        if !self.targets.is_empty() {
            let _ = writeln!(out, "-- targets (label @ center):");
            for t in &self.targets {
                let (x, y) = t.center();
                let _ = writeln!(out, "  {:?} @ {x},{y}", t.label);
            }
        }
        if !self.images.is_empty() {
            let _ = writeln!(out, "-- images: {}", self.images.len());
        }
        out
    }
}

struct ScreenWalk {
    items: Vec<ScreenItem>,
    ops: usize,
    w: f32,
    h: f32,
}

fn clip_rect(clip: [f32; 4], p0: (f32, f32), p1: (f32, f32)) -> [f32; 4] {
    [
        clip[0].max(p0.0.min(p1.0)),
        clip[1].max(p0.1.min(p1.1)),
        clip[2].min(p0.0.max(p1.0)),
        clip[3].min(p0.1.max(p1.1)),
    ]
}

impl Engine {
    /// Walks the last recorded window display list (see `draw_frame`) and
    /// returns a semantic [`ScreenModel`]. Empty when no frame was recorded.
    pub fn screen_model(&self) -> ScreenModel {
        let (w, h) = (FB_W as f32, FB_H as f32);
        let mut walk = ScreenWalk {
            items: Vec::new(),
            ops: 0,
            w,
            h,
        };
        if self.has_frame() {
            let ops = self.gfx.frame.clone();
            self.screen_ops(&mut walk, &ops, &ID, [0.0, 0.0, w, h], 1.0, false, 0);
        }
        build_model(walk)
    }

    #[allow(clippy::too_many_arguments)]
    fn screen_ops(
        &self,
        sw: &mut ScreenWalk,
        ops: &[Op],
        parent: &M,
        pclip: [f32; 4],
        palpha: f32,
        in_image: bool,
        depth: usize,
    ) {
        if depth > 64 || sw.ops >= MAX_RASTER_OPS_VISITED {
            return;
        }
        for op in ops {
            if sw.ops >= MAX_RASTER_OPS_VISITED {
                return;
            }
            sw.ops += 1;
            let m = mul(parent, &op.m);
            let cc = [
                ap(parent, op.clip[0].max(-1e6), op.clip[1].max(-1e6)),
                ap(parent, op.clip[2].min(1e6), op.clip[3].min(1e6)),
            ];
            let clip = clip_rect(pclip, cc[0], cc[1]);
            if clip[0] >= clip[2] || clip[1] >= clip[3] || op.p.clear {
                continue;
            }
            let alpha = palpha * op.alpha;
            let col = mod_alpha(op.p.tint.unwrap_or(op.p.argb), alpha);
            let visible = (col >> 24) >= 0x18;
            let filled = op.p.style != 1;
            let push = |sw: &mut ScreenWalk, kind, text: String, r: [f32; 4], argb, size, filled| {
                let b = [
                    r[0].max(0.0).floor() as i32,
                    r[1].max(0.0).floor() as i32,
                    r[2].min(sw.w).ceil() as i32,
                    r[3].min(sw.h).ceil() as i32,
                ];
                if b[2] - b[0] >= 2 && b[3] - b[1] >= 2 && sw.items.len() < 2_048 {
                    sw.items.push(ScreenItem {
                        kind,
                        text,
                        bounds: b,
                        argb,
                        size,
                        filled,
                    });
                }
            };
            let shape_bounds = |pts: &[(f32, f32)]| -> Option<[f32; 4]> {
                let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for &(x, y) in pts {
                    let (tx, ty) = ap(&m, x, y);
                    b = [b[0].min(tx), b[1].min(ty), b[2].max(tx), b[3].max(ty)];
                }
                let r = clip_rect(clip, (b[0], b[1]), (b[2], b[3]));
                (r[0] < r[2] && r[1] < r[3]).then_some(r)
            };
            match &op.k {
                OpK::Rect(l, t, r, b) | OpK::RRect(l, t, r, b, _, _) | OpK::Oval(l, t, r, b) => {
                    if visible && !in_image {
                        if let Some(rb) = shape_bounds(&[(*l, *t), (*r, *t), (*r, *b), (*l, *b)]) {
                            push(sw, ScreenKind::Box, String::new(), rb, col, 0.0, filled);
                        }
                    }
                }
                OpK::Path(polys, _) => {
                    if visible && !in_image {
                        let pts: Vec<(f32, f32)> = polys.iter().flatten().copied().collect();
                        if let Some(rb) = shape_bounds(&pts) {
                            push(sw, ScreenKind::Box, String::new(), rb, col, 0.0, filled);
                        }
                    }
                }
                OpK::Color(c) => {
                    let c = mod_alpha(*c, alpha);
                    if (c >> 24) >= 0x18 && !in_image {
                        push(sw, ScreenKind::Box, String::new(), clip, c, 0.0, true);
                    }
                }
                OpK::Line(..) => {}
                OpK::Text(s, x, y, size) => {
                    if !visible || s.trim().is_empty() {
                        continue;
                    }
                    let ms = (m[0] * m[0] + m[1] * m[1]).sqrt().max(1e-3);
                    let (ox, oy) = ap(&m, *x, *y);
                    let width = measure(s, *size, op.p.ls) * ms;
                    // atlas metrics are signed by font convention; the glyph
                    // box is always [baseline - |ascent|, baseline + |descent|]
                    let top = oy - ascent(*size).abs() * ms;
                    let bot = oy + descent(*size).abs() * ms;
                    let r = clip_rect(clip, (ox, top), (ox + width, bot));
                    // at least ~a third of the glyph box must survive clipping
                    if r[2] - r[0] < width.min(4.0) || (r[3] - r[1]) < (bot - top) * 0.34 {
                        continue;
                    }
                    let text = s.replace(['\n', '\r'], " ").trim_end().to_string();
                    push(sw, ScreenKind::Text, text, r, col, size * ms, false);
                }
                OpK::Node(n) => {
                    if let Some(G::Node(ns)) = self.gfx.objs.get(n) {
                        let nm = mul(&m, &ns.matrix());
                        let bounds = if ns.clip_outline {
                            ns.outline.or(Some([
                                0.0,
                                0.0,
                                ns.r.saturating_sub(ns.l) as f32,
                                ns.b.saturating_sub(ns.t) as f32,
                            ]))
                        } else if ns.clip_bounds {
                            Some([
                                0.0,
                                0.0,
                                ns.r.saturating_sub(ns.l) as f32,
                                ns.b.saturating_sub(ns.t) as f32,
                            ])
                        } else {
                            None
                        };
                        let nclip = match bounds {
                            Some(bb) => clip_rect(clip, ap(&nm, bb[0], bb[1]), ap(&nm, bb[2], bb[3])),
                            None => clip,
                        };
                        let na = alpha * ns.g("alpha", 1.0);
                        if na > 0.05 {
                            self.screen_ops(sw, &ns.ops, &nm, nclip, na, in_image, depth + 1);
                        }
                    }
                }
                OpK::Bitmap(bm, dst) => {
                    if let Some(G::Bitmap(bw, bh, bops)) = self.gfx.objs.get(bm) {
                        let s = mul(
                            &m,
                            &mul(
                                &tr(dst[0], dst[1]),
                                &sc(
                                    (dst[2] - dst[0]) / (*bw).max(1) as f32,
                                    (dst[3] - dst[1]) / (*bh).max(1) as f32,
                                ),
                            ),
                        );
                        let (p0, p1) = (ap(&s, 0.0, 0.0), ap(&s, *bw as f32, *bh as f32));
                        let bclip = clip_rect(clip, p0, p1);
                        if bclip[0] < bclip[2] && bclip[1] < bclip[3] && alpha > 0.05 {
                            if !in_image {
                                let c = op.p.tint.map(|t| mod_alpha(t, alpha)).unwrap_or(0);
                                push(sw, ScreenKind::Image, String::new(), bclip, c, 0.0, false);
                            }
                            // text drawn into a bitmap is still text
                            self.screen_ops(sw, bops, &s, bclip, alpha, true, depth + 1);
                        }
                    }
                }
            }
        }
    }
}

fn contains_b(o: &[i32; 4], i: &[i32; 4]) -> bool {
    o[0] <= i[0] + 1 && o[1] <= i[1] + 1 && o[2] >= i[2] - 1 && o[3] >= i[3] - 1
}

fn build_model(walk: ScreenWalk) -> ScreenModel {
    use std::hash::{Hash, Hasher};
    let (w, h) = (walk.w as i32, walk.h as i32);
    let screen_area = w as i64 * h as i64;
    let mut texts: Vec<ScreenItem> = Vec::new();
    let mut boxes: Vec<ScreenItem> = Vec::new();
    let mut images: Vec<ScreenItem> = Vec::new();
    let mut background = None;
    // painter's order: an item fully covered by an opaque box drawn LATER is
    // not visible (sheet/dialog surfaces over the screen behind them)
    let items = walk.items;
    let opaque: Vec<(usize, [i32; 4])> = items
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind == ScreenKind::Box && b.filled && (b.argb >> 24) >= 0xf0)
        .map(|(i, b)| (i, b.bounds))
        .collect();
    let hidden = |i: usize, it: &ScreenItem| {
        opaque
            .iter()
            .any(|(j, ob)| *j > i && contains_b(ob, &it.bounds) && ob != &it.bounds)
    };
    let visible: Vec<ScreenItem> = items
        .iter()
        .enumerate()
        .filter(|(i, it)| !hidden(*i, it))
        .map(|(_, it)| it.clone())
        .collect();
    for it in visible {
        match it.kind {
            ScreenKind::Text => texts.push(it),
            ScreenKind::Image => {
                if !images.iter().any(|o| o.bounds == it.bounds) {
                    images.push(it)
                }
            }
            ScreenKind::Box => {
                let a = it.area();
                if a * 10 >= screen_area * 9 {
                    background = Some(it.argb);
                } else if it.bounds[2] - it.bounds[0] >= 12
                    && it.bounds[3] - it.bounds[1] >= 12
                    && !boxes.iter().any(|o| o.bounds == it.bounds)
                {
                    boxes.push(it)
                }
            }
        }
    }
    // reading order, then merge runs that continue on the same baseline
    texts.sort_by_key(|t| (t.bounds[3], t.bounds[0]));
    let mut merged: Vec<ScreenItem> = Vec::new();
    for t in texts {
        if let Some(last) = merged.last_mut() {
            let same_line = (last.bounds[3] - t.bounds[3]).abs() <= 2
                && (last.size - t.size).abs() < 0.5
                && last.argb == t.argb;
            let gap = t.bounds[0] - last.bounds[2];
            if same_line && gap >= -2 && (gap as f32) < t.size * 0.6 {
                if gap as f32 > t.size * 0.15 && !last.text.ends_with(' ') {
                    last.text.push(' ');
                }
                last.text.push_str(&t.text);
                last.bounds[1] = last.bounds[1].min(t.bounds[1]);
                last.bounds[2] = last.bounds[2].max(t.bounds[2]);
                continue;
            }
            // identical run drawn twice (shadow / crossfade) → keep one
            if last.text == t.text && last.bounds == t.bounds {
                continue;
            }
        }
        merged.push(t);
    }
    let mut texts = merged;
    texts.sort_by_key(|t| ((t.bounds[1] + t.bounds[3]) / 2 / 8, t.bounds[0]));
    // targets: smallest enclosing box (< 40% of the screen) per label;
    // labels sharing a box are joined in reading order
    let mut targets: Vec<ScreenTarget> = Vec::new();
    for t in &texts {
        let enclosing = boxes
            .iter()
            .filter(|b| b.contains(&t.bounds) && b.area() * 5 < screen_area * 2)
            .filter(|b| b.area() > t.area())
            .min_by_key(|b| b.area());
        if let Some(b) = enclosing {
            match targets.iter_mut().find(|x| x.bounds == b.bounds) {
                Some(x) => {
                    x.label.push(' ');
                    x.label.push_str(&t.text);
                }
                None => targets.push(ScreenTarget {
                    label: t.text.clone(),
                    bounds: b.bounds,
                }),
            }
        }
    }
    // a box holding labels side by side on one row (navigation bar, toolbar,
    // segmented row) is a container, not a button: drop it so each label is
    // tapped on its own position instead of the container's centre
    targets.retain(|tg| {
        let inside: Vec<&ScreenItem> = texts.iter().filter(|t| contains_b(&tg.bounds, &t.bounds)).collect();
        !inside.iter().enumerate().any(|(i, a)| {
            inside[i + 1..].iter().any(|b| {
                let v_overlap = a.bounds[3].min(b.bounds[3]) - a.bounds[1].max(b.bounds[1]);
                let h_disjoint = a.bounds[2] <= b.bounds[0] || b.bounds[2] <= a.bounds[0];
                let gap = (b.bounds[0] - a.bounds[2]).max(a.bounds[0] - b.bounds[2]);
                v_overlap > 0 && h_disjoint && gap as f32 > a.size.max(b.size) * 0.8
            })
        })
    });
    let mut hs = std::collections::hash_map::DefaultHasher::new();
    for t in &texts {
        t.text.hash(&mut hs);
        t.bounds.hash(&mut hs);
        t.argb.hash(&mut hs);
    }
    for b in boxes.iter().chain(images.iter()) {
        b.kind.hash(&mut hs);
        b.bounds.hash(&mut hs);
        b.argb.hash(&mut hs);
    }
    background.hash(&mut hs);
    ScreenModel {
        width: w,
        height: h,
        texts,
        boxes,
        images,
        targets,
        background,
        ops: walk.ops,
        fingerprint: hs.finish(),
    }
}

#[cfg(test)]
mod screen_model_tests {
    use super::*;

    fn item(kind: ScreenKind, text: &str, b: [i32; 4], size: f32) -> ScreenItem {
        ScreenItem {
            kind,
            text: text.into(),
            bounds: b,
            argb: 0xff00_0000,
            size,
            filled: true,
        }
    }

    #[test]
    fn merges_runs_and_finds_targets() {
        let walk = ScreenWalk {
            items: vec![
                item(ScreenKind::Box, "", [0, 0, 720, 1440], 0.0),
                item(ScreenKind::Box, "", [40, 1300, 680, 1410], 0.0),
                item(ScreenKind::Text, "Conti", [300, 1340, 350, 1370], 28.0),
                item(ScreenKind::Text, "nuar", [351, 1340, 400, 1370], 28.0),
                item(ScreenKind::Text, "Bem-vindo", [100, 200, 400, 240], 40.0),
            ],
            ops: 5,
            w: 720.0,
            h: 1440.0,
        };
        let m = build_model(walk);
        assert_eq!(m.background, Some(0xff00_0000));
        let names: Vec<&str> = m.texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(names, vec!["Bem-vindo", "Continuar"]);
        assert_eq!(m.targets.len(), 1);
        assert_eq!(m.targets[0].label, "Continuar");
        assert_eq!(m.targets[0].center(), (360, 1355));
        assert!(m.to_text().contains("\"Continuar\" @ 360,1355"));
    }

    #[test]
    fn side_by_side_labels_make_a_container_not_a_target() {
        // bottom navigation: one bar box, five labels on one row
        let mut items = vec![item(ScreenKind::Box, "", [0, 1320, 720, 1440], 0.0)];
        for (i, l) in ["Listas", "Comparar", "Scanner"].iter().enumerate() {
            let x = 40 + i as i32 * 240;
            items.push(item(ScreenKind::Text, l, [x, 1390, x + 90, 1412], 18.0));
        }
        let m = build_model(ScreenWalk {
            items,
            ops: 4,
            w: 720.0,
            h: 1440.0,
        });
        assert!(m.targets.is_empty(), "{:?}", m.targets);
        assert_eq!(m.texts.len(), 3);
    }

    #[test]
    fn items_under_a_later_opaque_surface_are_hidden() {
        let mut sheet = item(ScreenKind::Box, "", [0, 600, 720, 1440], 0.0);
        sheet.argb = 0xffff_ffff;
        let mut scrim = item(ScreenKind::Box, "", [0, 0, 720, 1440], 0.0);
        scrim.argb = 0x5200_0000;
        let m = build_model(ScreenWalk {
            items: vec![
                item(ScreenKind::Text, "behind sheet", [100, 700, 300, 730], 20.0),
                item(ScreenKind::Text, "above sheet", [100, 200, 300, 230], 20.0),
                scrim,
                sheet,
                item(ScreenKind::Text, "on sheet", [100, 800, 300, 830], 20.0),
            ],
            ops: 5,
            w: 720.0,
            h: 1440.0,
        });
        let names: Vec<&str> = m.texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(names, vec!["above sheet", "on sheet"], "scrim is translucent, sheet is opaque");
    }

    #[test]
    fn stroked_outline_drawn_after_label_does_not_hide_it() {
        let mut outline = item(ScreenKind::Box, "", [580, 78, 684, 160], 0.0);
        outline.filled = false;
        let m = build_model(ScreenWalk {
            items: vec![item(ScreenKind::Text, "Pular", [606, 107, 660, 132], 21.0), outline],
            ops: 2,
            w: 720.0,
            h: 1440.0,
        });
        assert_eq!(m.texts.len(), 1);
        assert_eq!(m.targets.len(), 1);
        assert_eq!(m.targets[0].label, "Pular");
    }

    #[test]
    fn stacked_lines_in_one_card_stay_one_target() {
        let m = build_model(ScreenWalk {
            items: vec![
                item(ScreenKind::Box, "", [20, 300, 700, 420], 0.0),
                item(ScreenKind::Text, "Compras de janeiro", [40, 320, 400, 350], 26.0),
                item(ScreenKind::Text, "3 itens", [40, 370, 160, 395], 20.0),
            ],
            ops: 3,
            w: 720.0,
            h: 1440.0,
        });
        assert_eq!(m.targets.len(), 1);
        assert_eq!(m.targets[0].label, "Compras de janeiro 3 itens");
    }

    #[test]
    fn fingerprint_tracks_content() {
        let mk = |t: &str| {
            build_model(ScreenWalk {
                items: vec![item(ScreenKind::Text, t, [10, 10, 100, 40], 20.0)],
                ops: 1,
                w: 720.0,
                h: 1440.0,
            })
        };
        assert_eq!(mk("a").fingerprint, mk("a").fingerprint);
        assert_ne!(mk("a").fingerprint, mk("b").fingerprint);
    }
}
