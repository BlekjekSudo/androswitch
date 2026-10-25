//! `android.graphics` + `android.text` layout: a recording 2D pipeline.
//!
//! **Model.** Host graphics objects keep their native state in a side table
//! (`Engine::gfx`, keyed by `ObjRef`): `Paint`/`TextPaint`, `Path`, `Matrix`,
//! `Canvas`/`RecordingCanvas`, `RenderNode`, `Bitmap`, `Outline`, `Typeface`
//! and text layouts (`StaticLayout`, `BoringLayout`, `StaticLayout$Builder`).
//! DEX subclasses (Compose's `AndroidTextPaint extends TextPaint`, …) work
//! because the state is created by the host super-constructor and later calls
//! are routed by receiver. `Rect`/`RectF`/`Point`/`PointF` and the
//! `FontMetrics*`/`BoringLayout$Metrics` structs keep real heap fields
//! (`left`, `top`, …) because DEX code reads them with `iget`.
//!
//! **Recording.** A canvas records draw ops with its current affine matrix,
//! clip and layer alpha baked in. A canvas targets a `RenderNode` display list
//! (`beginRecording`/`endRecording`), a `Bitmap` (`new Canvas(bitmap)`) or the
//! window frame (the host draw pass, see `draw_pass.rs`). `drawRenderNode`
//! records a reference, so a node re-recorded later shows its latest content,
//! as with AOSP's RenderThread.
//!
//! **Rasterizing.** `Engine::rasterize_frame` flattens the window display list
//! into a 720×1440 RGB framebuffer: rects, round rects, ovals, paths (nonzero
//! scanline fill, 4× vertical supersampling), strokes (as quads), bitmaps
//! (replayed), and text drawn from a DejaVu Sans glyph atlas
//! (`assets/glyphs.bin`, `scripts/gen_glyph_atlas.py`).
//!
//! **Text.** Measurement uses the same atlas advances, so layout and pixels
//! agree. Line breaking is greedy on spaces with forced breaks at `\n`; there
//! is no shaping, bidi or hyphenation (documented deviation). Unknown glyphs
//! use the replacement glyph's advance.
//!
//! **Deviations.** Shaders, color filters, blend modes, path effects, shadows
//! and 3D rotation are accepted and ignored. A shader-only paint draws with
//! its color. Clips are rectangular: a path clip uses its bounds and
//! `DIFFERENCE` clips are ignored. Bitmaps hold a display list, not pixels, so
//! `getPixels` returns transparent pixels and `compress` returns false.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::{ElemKind, ObjRef};
use crate::value::Value;
use std::collections::HashMap;

pub const FB_W: usize = 720;
pub const FB_H: usize = 1440;

type M = [f32; 6]; // x' = a x + c y + e ; y' = b x + d y + f  → [a b c d e f]
const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mul(p: &M, q: &M) -> M {
    // p ∘ q (apply q first, then p)
    [
        p[0] * q[0] + p[2] * q[1],
        p[1] * q[0] + p[3] * q[1],
        p[0] * q[2] + p[2] * q[3],
        p[1] * q[2] + p[3] * q[3],
        p[0] * q[4] + p[2] * q[5] + p[4],
        p[1] * q[4] + p[3] * q[5] + p[5],
    ]
}
fn ap(m: &M, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}
fn tr(x: f32, y: f32) -> M {
    [1.0, 0.0, 0.0, 1.0, x, y]
}
fn sc(x: f32, y: f32) -> M {
    [x, 0.0, 0.0, y, 0.0, 0.0]
}
fn rot(deg: f32) -> M {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s, -s, c, 0.0, 0.0]
}
fn inv(m: &M) -> Option<M> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return None;
    }
    let (a, b, c, d) = (m[3] / det, -m[1] / det, -m[2] / det, m[0] / det);
    Some([a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])])
}
/// Android 3×3 (row-major MSCALE_X, MSKEW_X, MTRANS_X, MSKEW_Y, MSCALE_Y, MTRANS_Y, …) → affine.
fn from9(v: &[f32; 9]) -> M {
    [v[0], v[3], v[1], v[4], v[2], v[5]]
}
fn to9(m: &M) -> [f32; 9] {
    [m[0], m[2], m[4], m[1], m[3], m[5], 0.0, 0.0, 1.0]
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintSnap {
    argb: u32,
    style: i32,
    sw: f32,
    /// `BlendMode.CLEAR` / `PorterDuff.Mode.CLEAR` transfer mode.
    clear: bool,
    /// Colour of a `SRC_IN`/`SRC_ATOP` colour filter (tint): the source's
    /// coverage/alpha is kept and its colour replaced.
    tint: Option<u32>,
    /// Letter spacing (em), applied to the pen advance of text ops.
    ls: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct PaintS {
    argb: u32,
    style: i32,
    sw: f32,
    text_size: f32,
    flags: i32,
    letter_spacing: f32,
    scale_x: f32,
    typeface: Value,
    misc: HashMap<String, Value>,
}
impl Default for PaintS {
    fn default() -> Self {
        PaintS {
            argb: 0xFF00_0000,
            style: 0,
            sw: 0.0,
            text_size: 12.0,
            flags: 0,
            letter_spacing: 0.0,
            scale_x: 1.0,
            typeface: Value::Null,
            misc: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum OpK {
    Rect(f32, f32, f32, f32),
    RRect(f32, f32, f32, f32, f32, f32),
    Oval(f32, f32, f32, f32),
    Line(f32, f32, f32, f32),
    Path(Vec<Vec<(f32, f32)>>, bool),
    Text(String, f32, f32, f32),
    Color(u32),
    Node(ObjRef),
    Bitmap(ObjRef, [f32; 4]),
}

#[derive(Clone, Debug)]
pub(crate) struct Op {
    m: M,
    clip: [f32; 4],
    alpha: f32,
    p: PaintSnap,
    k: OpK,
}

#[derive(Clone, Copy, Debug)]
struct St {
    m: M,
    clip: [f32; 4],
    alpha: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum Target {
    None,
    Node(ObjRef),
    Bitmap(ObjRef),
    Frame,
}

#[derive(Clone, Debug)]
pub(crate) struct CanvasS {
    target: Target,
    stack: Vec<St>,
    w: i32,
    h: i32,
}
impl CanvasS {
    fn new(target: Target, w: i32, h: i32) -> Self {
        CanvasS {
            target,
            stack: vec![St {
                m: ID,
                clip: [-1e9, -1e9, 1e9, 1e9],
                alpha: 1.0,
            }],
            w,
            h,
        }
    }
    fn top(&mut self) -> &mut St {
        self.stack.last_mut().expect("canvas stack never empty")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeS {
    l: i32,
    t: i32,
    r: i32,
    b: i32,
    f: HashMap<&'static str, f32>,
    clip_bounds: bool,
    clip_outline: bool,
    outline: Option<[f32; 4]>,
    ops: Vec<Op>,
    has_dl: bool,
}
impl Default for NodeS {
    fn default() -> Self {
        NodeS {
            l: 0,
            t: 0,
            r: 0,
            b: 0,
            f: HashMap::new(),
            clip_bounds: true,
            clip_outline: false,
            outline: None,
            ops: Vec::new(),
            has_dl: false,
        }
    }
}
impl NodeS {
    fn g(&self, k: &str, d: f32) -> f32 {
        self.f.get(k).copied().unwrap_or(d)
    }
    fn matrix(&self) -> M {
        let (w, h) = ((self.r - self.l) as f32, (self.b - self.t) as f32);
        let px = self.g("pivotX", w / 2.0);
        let py = self.g("pivotY", h / 2.0);
        let local = mul(
            &tr(
                px + self.g("translationX", 0.0),
                py + self.g("translationY", 0.0),
            ),
            &mul(
                &mul(
                    &rot(self.g("rotationZ", 0.0)),
                    &sc(self.g("scaleX", 1.0), self.g("scaleY", 1.0)),
                ),
                &tr(-px, -py),
            ),
        );
        mul(&tr(self.l as f32, self.t as f32), &local)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutS {
    text: String,
    cs: Value,
    paint: Value,
    size: f32,
    #[allow(dead_code)] // kept for span-colour queries
    argb: u32,
    ls: f32,
    width: i32,
    align: i32,
    lines: Vec<(usize, usize, f32)>, // char start, char end (excl.), width
    asc: f32,
    desc: f32,
    lh: f32,
    ell: Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub(crate) struct BuilderS {
    cs: Value,
    start: i32,
    end: i32,
    paint: Value,
    width: i32,
    align: i32,
    max_lines: i32,
    ellipsize: bool,
    spacing_mult: f32,
    spacing_add: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum G {
    Paint(PaintS),
    Path(Vec<Vec<(f32, f32)>>, bool),
    Matrix(M),
    Canvas(CanvasS),
    Node(NodeS),
    Bitmap(i32, i32, Vec<Op>),
    Layout(LayoutS),
    Builder(BuilderS),
    Outline(Option<[f32; 4]>, f32, f32),
}

#[derive(Default)]
pub(crate) struct Gfx {
    pub(crate) objs: HashMap<ObjRef, G>,
    pub(crate) frame: Vec<Op>,
    pub(crate) frame_valid: bool,
}

// ── glyph atlas ────────────────────────────────────────────────────────

pub(crate) struct Glyph {
    adv: f32,
    left: i16,
    top: i16,
    w: u16,
    h: u16,
    off: usize,
}
pub(crate) struct Atlas {
    size: f32,
    asc: f32,
    desc: f32,
    map: HashMap<u32, Glyph>,
    data: &'static [u8],
}
static GLYPHS: &[u8] = include_bytes!("../assets/glyphs.bin");

fn atlas() -> &'static Atlas {
    static A: std::sync::OnceLock<Atlas> = std::sync::OnceLock::new();
    A.get_or_init(|| {
        let d = GLYPHS;
        let u16_at = |i: usize| u16::from_le_bytes([d[i], d[i + 1]]);
        let i16_at = |i: usize| i16::from_le_bytes([d[i], d[i + 1]]);
        let size = u16_at(4) as f32;
        let (asc, desc, n) = (i16_at(6) as f32, i16_at(8) as f32, u16_at(10) as usize);
        let mut map = HashMap::new();
        let mut p = 12;
        for _ in 0..n {
            let cp = u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]);
            let g = Glyph {
                adv: u16_at(p + 4) as f32 / 64.0,
                left: i16_at(p + 6),
                top: i16_at(p + 8),
                w: u16_at(p + 10),
                h: u16_at(p + 12),
                off: p + 14,
            };
            p += 14 + g.w as usize * g.h as usize;
            map.insert(cp, g);
        }
        Atlas {
            size,
            asc,
            desc,
            map,
            data: d,
        }
    })
}
fn glyph(c: char) -> &'static Glyph {
    let a = atlas();
    a.map
        .get(&(c as u32))
        .or_else(|| a.map.get(&0xFFFD))
        .expect("replacement glyph in atlas")
}
fn char_adv(c: char, size: f32, ls: f32) -> f32 {
    match c {
        '\n' | '\r' => 0.0,
        '\t' => glyph(' ').adv * 4.0 * size / atlas().size,
        _ => glyph(c).adv * size / atlas().size + ls * size,
    }
}
pub(crate) fn measure(s: &str, size: f32, ls: f32) -> f32 {
    s.chars().map(|c| char_adv(c, size, ls)).sum()
}
fn ascent(size: f32) -> f32 {
    -atlas().asc * size / atlas().size
}
fn descent(size: f32) -> f32 {
    atlas().desc * size / atlas().size
}

// ── helpers ────────────────────────────────────────────────────────────

fn exc(c: &str, m: impl Into<String>) -> VmExit {
    VmExit::Exception(Throwable::new(c, m.into()))
}
fn put(vm: &mut Engine, r: ObjRef, f: &str, v: Value) -> Result<(), VmExit> {
    crate::err::put_field_result(vm.heap.put_field(r, f, v))
}
fn getf(vm: &Engine, r: ObjRef, f: &str) -> f32 {
    match vm.heap.get_field(r, f).unwrap_or(Value::Null) {
        Value::Float(x) => x,
        Value::Int(i) => i as f32,
        Value::Double(d) => d as f32,
        Value::Long(l) => l as f32,
        _ => 0.0,
    }
}
/// A `float` argument. DEX registers are untyped, so a float can arrive as
/// `Value::Int` holding its IEEE-754 bits (the VM convention, see
/// `Value::as_float`). Ints whose magnitude is at least 2^23 are read as bits:
/// every normal float except ±0 has such a bit pattern, while a genuine int
/// that large is not a plausible graphics coordinate. Smaller ints are taken
/// numerically, for host callers that pass ints to float parameters.
fn fl(v: Option<&Value>) -> f32 {
    match v {
        Some(Value::Float(x)) => *x,
        Some(Value::Int(i)) if i.unsigned_abs() >= 1 << 23 => f32::from_bits(*i as u32),
        Some(Value::Int(i)) => *i as f32,
        Some(Value::Double(d)) => *d as f32,
        Some(Value::Long(l)) => *l as f32,
        _ => 0.0,
    }
}
fn it(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Int(i)) => *i,
        Some(Value::Float(x)) => *x as i32,
        Some(Value::Long(l)) => *l as i32,
        _ => 0,
    }
}
fn obj(v: Option<&Value>) -> Option<ObjRef> {
    match v {
        Some(Value::Obj(r)) => Some(*r),
        _ => None,
    }
}
fn b(x: bool) -> Value {
    Value::Int(x as i32)
}
fn st(vm: &mut Engine, t: String) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(vm, t)?))
}

/// Text of a CharSequence (String fast path, else guest `toString()`).
pub(crate) fn cs_text(vm: &mut Engine, v: &Value) -> Result<String, VmExit> {
    let Value::Obj(r) = v else {
        return Ok(String::new());
    };
    if let Ok(s) = vm.heap.as_str(*r) {
        return Ok(s.to_string());
    }
    if let Ok((e, _)) = vm.heap.as_array_elems(*r) {
        return Ok(e
            .iter()
            .filter_map(|c| match c {
                Value::Int(i) => char::from_u32(*i as u32),
                _ => None,
            })
            .collect());
    }
    // Full virtual dispatch: guest CharSequences *and* host ones
    // (SpannableString, StringBuilder, …) answer `toString()`.
    match vm.invoke_instance(*r, "toString", "()Ljava/lang/String;", &[]) {
        Ok(Value::Obj(s)) => Ok(vm.heap.as_str(s).map(str::to_string).unwrap_or_default()),
        Ok(_) => Ok(String::new()),
        Err(VmExit::Exception(t)) => Err(VmExit::Exception(t)),
        Err(_) => Ok(String::new()),
    }
}
fn checked_text_slice(
    vm: &mut Engine,
    v: Option<&Value>,
    is_char_array: bool,
    i0: i32,
    i1: i32,
    count_mode: bool,
) -> Result<String, VmExit> {
    let Some(Value::Obj(first_ref)) = v else {
        return Err(exc("Ljava/lang/NullPointerException;", "text == null"));
    };
    let units: Vec<u16> = if is_char_array {
        vm.heap
            .as_array_elems(*first_ref)
            .map(|(e, _)| {
                e.iter()
                    .map(|c| match c {
                        Value::Int(i) => *i as u16,
                        _ => 0,
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        cs_text(vm, &Value::Obj(*first_ref))?
            .encode_utf16()
            .collect()
    };
    if count_mode {
        if i0 < 0
            || i1 < 0
            || (i0 as usize) > units.len()
            || (i1 as usize) > units.len() - (i0 as usize)
        {
            return Err(exc(
                "Ljava/lang/IndexOutOfBoundsException;",
                "char[] index/count out of bounds",
            ));
        }
        let st_u = i0 as usize;
        Ok(String::from_utf16_lossy(&units[st_u..st_u + i1 as usize]))
    } else {
        if i0 < 0 || i1 < i0 || (i1 as usize) > units.len() {
            return Err(exc(
                "Ljava/lang/IndexOutOfBoundsException;",
                "text start/end out of bounds",
            ));
        }
        Ok(String::from_utf16_lossy(&units[i0 as usize..i1 as usize]))
    }
}

fn sub(s: &str, a: i32, e: i32) -> String {
    let a = a.max(0) as usize;
    let e = (e.max(0) as usize).max(a);
    s.chars().skip(a).take(e - a).collect()
}

fn rect_of(vm: &Engine, r: Option<ObjRef>) -> [f32; 4] {
    match r {
        Some(r) => [
            getf(vm, r, "left"),
            getf(vm, r, "top"),
            getf(vm, r, "right"),
            getf(vm, r, "bottom"),
        ],
        None => [0.0; 4],
    }
}
fn set_rect(vm: &mut Engine, r: ObjRef, v: [f32; 4], int: bool) -> Result<(), VmExit> {
    for (k, x) in ["left", "top", "right", "bottom"].iter().zip(v) {
        put(
            vm,
            r,
            k,
            if int {
                Value::Int(x.round() as i32)
            } else {
                Value::Float(x)
            },
        )?;
    }
    Ok(())
}

fn paint_of(vm: &Engine, v: Option<&Value>) -> PaintS {
    match obj(v).and_then(|r| vm.gfx.objs.get(&r)) {
        Some(G::Paint(p)) => p.clone(),
        _ => PaintS::default(),
    }
}
fn snap(vm: &Engine, p: &PaintS) -> PaintSnap {
    let ord_of = |v: Option<&Value>| match v {
        Some(Value::Obj(r)) => vm
            .heap
            .get_field(*r, "rd_ord")
            .ok()
            .and_then(|v| v.as_int().ok()),
        _ => None,
    };
    let field = |v: Option<&Value>, f: &str| match v {
        Some(Value::Obj(r)) => vm.heap.get_field(*r, f).ok(),
        _ => None,
    };
    // CLEAR = ordinal 0 in both BlendMode and PorterDuff.Mode
    let clear = ord_of(p.misc.get("BlendMode")) == Some(0)
        || ord_of(field(p.misc.get("Xfermode"), "rd_mode").as_ref()) == Some(0);
    let cf = p.misc.get("ColorFilter");
    let tint = match ord_of(field(cf, "rd_mode").as_ref()) {
        // SRC_IN = 5, SRC_ATOP = 9 in both enums
        Some(5 | 9) => field(cf, "rd_color")
            .and_then(|v| v.as_int().ok())
            .map(|c| c as u32),
        _ => None,
    };
    PaintSnap {
        argb: p.argb,
        style: p.style,
        sw: p.sw,
        clear,
        tint,
        ls: p.letter_spacing,
    }
}

const ENUMS: &[(&str, &[&str])] = &[
    (
        "Landroid/graphics/Paint$Style;",
        &["FILL", "STROKE", "FILL_AND_STROKE"],
    ),
    ("Landroid/graphics/Paint$Cap;", &["BUTT", "ROUND", "SQUARE"]),
    (
        "Landroid/graphics/Paint$Join;",
        &["MITER", "ROUND", "BEVEL"],
    ),
    (
        "Landroid/graphics/Paint$Align;",
        &["LEFT", "CENTER", "RIGHT"],
    ),
    (
        "Landroid/graphics/Path$FillType;",
        &["WINDING", "EVEN_ODD", "INVERSE_WINDING", "INVERSE_EVEN_ODD"],
    ),
    ("Landroid/graphics/Path$Direction;", &["CW", "CCW"]),
    (
        "Landroid/graphics/Path$Op;",
        &[
            "DIFFERENCE",
            "INTERSECT",
            "UNION",
            "XOR",
            "REVERSE_DIFFERENCE",
        ],
    ),
    (
        "Landroid/graphics/Region$Op;",
        &[
            "DIFFERENCE",
            "INTERSECT",
            "UNION",
            "XOR",
            "REVERSE_DIFFERENCE",
            "REPLACE",
        ],
    ),
    (
        "Landroid/graphics/Matrix$ScaleToFit;",
        &["FILL", "START", "CENTER", "END"],
    ),
    ("Landroid/graphics/Canvas$EdgeType;", &["BW", "AA"]),
    (
        "Landroid/graphics/Bitmap$Config;",
        &[
            "ALPHA_8",
            "RGB_565",
            "ARGB_4444",
            "ARGB_8888",
            "RGBA_F16",
            "HARDWARE",
            "RGBA_1010102",
        ],
    ),
    (
        "Landroid/graphics/Bitmap$CompressFormat;",
        &["JPEG", "PNG", "WEBP", "WEBP_LOSSY", "WEBP_LOSSLESS"],
    ),
    (
        "Landroid/graphics/Shader$TileMode;",
        &["CLAMP", "REPEAT", "MIRROR", "DECAL"],
    ),
    (
        "Landroid/text/Layout$Alignment;",
        &[
            "ALIGN_NORMAL",
            "ALIGN_OPPOSITE",
            "ALIGN_CENTER",
            "ALIGN_LEFT",
            "ALIGN_RIGHT",
        ],
    ),
    (
        "Landroid/text/TextUtils$TruncateAt;",
        &["START", "MIDDLE", "END", "MARQUEE", "END_SMALL"],
    ),
    (
        "Landroid/graphics/PorterDuff$Mode;",
        &[
            "CLEAR", "SRC", "DST", "SRC_OVER", "DST_OVER", "SRC_IN", "DST_IN", "SRC_OUT",
            "DST_OUT", "SRC_ATOP", "DST_ATOP", "XOR", "DARKEN", "LIGHTEN", "MULTIPLY", "SCREEN",
            "ADD", "OVERLAY",
        ],
    ),
    (
        "Landroid/graphics/BlendMode;",
        &[
            "CLEAR",
            "SRC",
            "DST",
            "SRC_OVER",
            "DST_OVER",
            "SRC_IN",
            "DST_IN",
            "SRC_OUT",
            "DST_OUT",
            "SRC_ATOP",
            "DST_ATOP",
            "XOR",
            "PLUS",
            "MODULATE",
            "SCREEN",
            "OVERLAY",
            "DARKEN",
            "LIGHTEN",
            "COLOR_DODGE",
            "COLOR_BURN",
            "HARD_LIGHT",
            "SOFT_LIGHT",
            "DIFFERENCE",
            "EXCLUSION",
            "MULTIPLY",
            "HUE",
            "SATURATION",
            "COLOR",
            "LUMINOSITY",
        ],
    ),
];

fn enum_const(vm: &mut Engine, class: &str, idx: usize) -> Result<Option<Value>, VmExit> {
    let Some((c, names)) = ENUMS.iter().find(|(c, _)| *c == class) else {
        return Ok(None);
    };
    let Some(n) = names.get(idx) else {
        return Ok(None);
    };
    let k = (c.to_string(), n.to_string());
    if let Some(v) = vm.statics.get(&k) {
        return Ok(Some(v.clone()));
    }
    let r = vm.heap.alloc_instance(c.to_string(), Vec::new())?;
    put(vm, r, "rd_ord", Value::Int(idx as i32))?;
    let s = st(vm, n.to_string())?;
    put(vm, r, "rd_name", s)?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(Some(Value::Obj(r)))
}
fn ord(vm: &Engine, v: Option<&Value>) -> i32 {
    match obj(v) {
        Some(r) => match vm.heap.get_field(r, "rd_ord") {
            Ok(Value::Int(i)) => i,
            _ => 0,
        },
        None => -1,
    }
}

fn typeface(vm: &mut Engine, style: i32) -> Result<Value, VmExit> {
    let k = (
        "Landroid/graphics/Typeface;".to_string(),
        format!("rd_style_{style}"),
    );
    if let Some(v) = vm.statics.get(&k) {
        return Ok(v.clone());
    }
    let r = vm
        .heap
        .alloc_instance("Landroid/graphics/Typeface;".into(), Vec::new())?;
    put(vm, r, "rd_style", Value::Int(style))?;
    vm.statics.insert(k, Value::Obj(r));
    Ok(Value::Obj(r))
}

/// `sget` on host graphics constants (enum values, `Typeface.DEFAULT`, …).
pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    if owner == PLATFORM_INSETS && field == "NONE" {
        let key = (owner.to_string(), field.to_string());
        if let Some(value) = vm.statics.get(&key) {
            return Ok(Some(value.clone()));
        }
        let value = Value::Obj(alloc_platform_insets(vm, [0; 4])?);
        vm.statics.insert(key, value.clone());
        return Ok(Some(value));
    }
    if let Some((_, names)) = ENUMS.iter().find(|(c, _)| *c == owner) {
        if let Some(i) = names.iter().position(|n| *n == field) {
            return enum_const(vm, owner, i);
        }
        return Ok(None);
    }
    if owner == "Landroid/graphics/Typeface;" {
        return Ok(Some(match field {
            "DEFAULT" | "SANS_SERIF" | "SERIF" | "MONOSPACE" => typeface(vm, 0)?,
            "DEFAULT_BOLD" => typeface(vm, 1)?,
            _ => return Ok(None),
        }));
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)] // mirrors the StaticLayout constructor
fn new_layout(
    vm: &mut Engine,
    cs: Value,
    text: String,
    paint: Value,
    width: i32,
    align: i32,
    max_lines: i32,
    ellipsize: bool,
    mult: f32,
    add: f32,
) -> LayoutS {
    let p = paint_of(vm, Some(&paint));
    let (size, ls) = (p.text_size, p.letter_spacing);
    let chars: Vec<char> = text.chars().collect();
    let mut lines: Vec<(usize, usize, f32)> = Vec::new();
    let maxw = if width <= 0 { f32::MAX } else { width as f32 };
    let mut start = 0;
    while start <= chars.len() {
        // hard line end
        let hard = chars[start..]
            .iter()
            .position(|c| *c == '\n')
            .map(|i| start + i)
            .unwrap_or(chars.len());
        let mut s = start;
        loop {
            let mut w = 0.0;
            let mut end = s;
            let mut last_space: Option<usize> = None;
            while end < hard {
                let a = char_adv(chars[end], size, ls);
                if w + a > maxw + 0.5 && end > s {
                    break;
                }
                if chars[end] == ' ' {
                    last_space = Some(end);
                }
                w += a;
                end += 1;
            }
            if end < hard {
                if let Some(sp) = last_space.filter(|sp| *sp > s) {
                    end = sp + 1;
                }
            }
            let lw: f32 = chars[s..end]
                .iter()
                .rev()
                .skip_while(|c| **c == ' ')
                .map(|c| char_adv(*c, size, ls))
                .sum();
            let incl_nl = if end == hard && hard < chars.len() {
                1
            } else {
                0
            };
            lines.push((s, end + incl_nl, lw));
            s = end;
            if s >= hard {
                break;
            }
        }
        start = hard + 1;
        if hard >= chars.len() {
            break;
        }
    }
    if lines.is_empty() {
        lines.push((0, 0, 0.0));
    }
    let mut ell = Vec::new();
    if max_lines > 0 && lines.len() > max_lines as usize {
        lines.truncate(max_lines as usize);
        if let Some(l) = lines.last_mut() {
            if ellipsize {
                ell.push((l.0, l.1));
            }
            l.1 = chars.len();
        }
    }
    let (asc, desc) = (ascent(size), descent(size));
    let lh = ((desc - asc) * if mult > 0.0 { mult } else { 1.0 } + add).ceil();
    LayoutS {
        text,
        cs,
        paint,
        size,
        argb: p.argb,
        ls,
        width,
        align,
        lines,
        asc,
        desc,
        lh,
        ell,
    }
}

impl LayoutS {
    fn line_left(&self, i: usize) -> f32 {
        let w = self.lines.get(i).map(|l| l.2).unwrap_or(0.0);
        match self.align {
            1 | 4 => (self.width as f32 - w).max(0.0),
            2 => ((self.width as f32 - w) / 2.0).max(0.0),
            _ => 0.0,
        }
    }
    fn line_for_offset(&self, off: i32) -> usize {
        let off = off.max(0) as usize;
        self.lines
            .iter()
            .position(|l| off < l.1)
            .unwrap_or(self.lines.len() - 1)
    }
    fn horiz(&self, off: i32) -> f32 {
        let i = self.line_for_offset(off);
        let (s, _, _) = self.lines[i];
        let o = (off.max(0) as usize).max(s);
        self.line_left(i)
            + self
                .text
                .chars()
                .skip(s)
                .take(o - s)
                .map(|c| char_adv(c, self.size, self.ls))
                .sum::<f32>()
    }
    fn top(&self, i: usize) -> f32 {
        i as f32 * self.lh
    }
    fn baseline(&self, i: usize) -> f32 {
        self.top(i) - self.asc
    }
}

const MAX_BITMAP_DIM: i32 = 16_384;
const MAX_BITMAP_BYTES: i64 = i32::MAX as i64;
const MAX_PATH_POINTS: usize = 65_536;
const MAX_PATH_CONTOURS: usize = 16_384;
const MAX_CANVAS_SAVE_DEPTH: usize = 1_024;
const MAX_DISPLAY_LIST_OPS: usize = 32_768;
const MAX_RASTER_OPS_VISITED: usize = 32_768;
const MAX_SUMMARY_OPS_VISITED: usize = 8_192;

fn path_point_count(p: &[Vec<(f32, f32)>]) -> usize {
    p.iter().map(Vec::len).sum()
}

fn ensure_path_budget(
    p: &[Vec<(f32, f32)>],
    add_pts: usize,
    add_contours: usize,
) -> Result<(), VmExit> {
    if path_point_count(p).saturating_add(add_pts) > MAX_PATH_POINTS
        || p.len().saturating_add(add_contours) > MAX_PATH_CONTOURS
    {
        return Err(exc(
            "Ljava/lang/OutOfMemoryError;",
            "Path exceeds maximum point/contour budget",
        ));
    }
    Ok(())
}

fn record(vm: &mut Engine, canvas: ObjRef, p: PaintSnap, k: OpK) {
    let (target, s) = match vm.gfx.objs.get_mut(&canvas) {
        Some(G::Canvas(c)) => (c.target.clone(), *c.top()),
        _ => return,
    };
    let op = Op {
        m: s.m,
        clip: s.clip,
        alpha: s.alpha,
        p,
        k,
    };
    match target {
        Target::Node(n) => {
            if let Some(G::Node(ns)) = vm.gfx.objs.get_mut(&n) {
                if ns.ops.len() < MAX_DISPLAY_LIST_OPS {
                    ns.ops.push(op);
                }
            }
        }
        Target::Bitmap(bm) => {
            if let Some(G::Bitmap(_, _, ops)) = vm.gfx.objs.get_mut(&bm) {
                if ops.len() < MAX_DISPLAY_LIST_OPS {
                    ops.push(op);
                }
            }
        }
        Target::Frame => {
            if vm.gfx.frame.len() < MAX_DISPLAY_LIST_OPS {
                vm.gfx.frame.push(op);
            }
        }
        Target::None => {}
    }
}

fn path_bounds(p: &[Vec<(f32, f32)>]) -> [f32; 4] {
    let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for sp in p {
        for (x, y) in sp {
            r = [r[0].min(*x), r[1].min(*y), r[2].max(*x), r[3].max(*y)];
        }
    }
    if r[0] > r[2] {
        [0.0; 4]
    } else {
        r
    }
}
fn rrect_poly(l: f32, t: f32, r: f32, bt: f32, rad: [f32; 8]) -> Vec<(f32, f32)> {
    // rad = [tlx, tly, trx, try, brx, bry, blx, bly]
    let mut v = Vec::new();
    let corners = [
        (l + rad[0], t + rad[1], rad[0], rad[1], 180.0f32),
        (r - rad[2], t + rad[3], rad[2], rad[3], 270.0),
        (r - rad[4], bt - rad[5], rad[4], rad[5], 0.0),
        (l + rad[6], bt - rad[7], rad[6], rad[7], 90.0),
    ];
    for (cx, cy, rx, ry, a0) in corners {
        for i in 0..=8 {
            let a = (a0 + 90.0 * i as f32 / 8.0).to_radians();
            v.push((cx + rx * a.cos(), cy + ry * a.sin()));
        }
    }
    v
}
fn oval_poly(l: f32, t: f32, r: f32, bt: f32) -> Vec<(f32, f32)> {
    let (cx, cy, rx, ry) = ((l + r) / 2.0, (t + bt) / 2.0, (r - l) / 2.0, (bt - t) / 2.0);
    (0..48)
        .map(|i| {
            let a = i as f32 / 48.0 * std::f32::consts::TAU;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

fn path_mut(vm: &mut Engine, r: ObjRef) -> &mut Vec<Vec<(f32, f32)>> {
    let e = vm
        .gfx
        .objs
        .entry(r)
        .or_insert_with(|| G::Path(Vec::new(), false));
    if !matches!(e, G::Path(..)) {
        *e = G::Path(Vec::new(), false);
    }
    match e {
        G::Path(p, _) => p,
        _ => unreachable!(),
    }
}
fn path_of(vm: &Engine, r: Option<ObjRef>) -> Vec<Vec<(f32, f32)>> {
    match r.and_then(|r| vm.gfx.objs.get(&r)) {
        Some(G::Path(p, _)) => p.clone(),
        _ => Vec::new(),
    }
}
fn last_pt(p: &[Vec<(f32, f32)>]) -> (f32, f32) {
    p.last()
        .and_then(|s| s.last())
        .copied()
        .unwrap_or((0.0, 0.0))
}
fn matrix_of(vm: &Engine, r: Option<ObjRef>) -> M {
    match r.and_then(|r| vm.gfx.objs.get(&r)) {
        Some(G::Matrix(m)) => *m,
        _ => ID,
    }
}
/// A new host `android.graphics.Matrix` set to identity.
pub(crate) fn new_identity_matrix(vm: &mut Engine) -> Result<ObjRef, VmExit> {
    let r = vm
        .heap
        .alloc_instance("Landroid/graphics/Matrix;".into(), Vec::new())?;
    set_matrix(vm, r, ID);
    Ok(r)
}
fn set_matrix(vm: &mut Engine, r: ObjRef, m: M) {
    vm.gfx.objs.insert(r, G::Matrix(m));
}

fn float_array(vm: &Engine, v: Option<&Value>) -> Vec<f32> {
    match obj(v).and_then(|r| vm.heap.as_array_elems(r).ok()) {
        Some((e, _)) => e.iter().map(|x| fl(Some(x))).collect(),
        None => Vec::new(),
    }
}
fn float_array_len(vm: &Engine, v: Option<&Value>) -> i32 {
    obj(v)
        .and_then(|r| vm.heap.as_array_elems(r).ok())
        .map(|(e, _)| e.len() as i32)
        .unwrap_or(0)
}
fn write_floats(vm: &mut Engine, v: Option<&Value>, xs: &[f32], at: usize) {
    if let Some(r) = obj(v) {
        if let Ok(e) = vm.heap.array_elems_mut(r) {
            for (i, x) in xs.iter().enumerate() {
                if let Some(slot) = e.get_mut(at + i) {
                    *slot = Value::Float(*x);
                }
            }
        }
    }
}

const GFX_PREFIXES: [&str; 2] = ["Landroid/graphics/", "Landroid/text/"];
const PLATFORM_INSETS: &str = "Landroid/graphics/Insets;";

fn insets_values(vm: &Engine, recv: ObjRef) -> [i32; 4] {
    ["left", "top", "right", "bottom"].map(|field| match vm.heap.get_field(recv, field) {
        Ok(Value::Int(value)) => value,
        _ => 0,
    })
}

fn alloc_platform_insets(vm: &mut Engine, values: [i32; 4]) -> Result<ObjRef, VmExit> {
    let recv = vm
        .heap
        .alloc_instance(PLATFORM_INSETS.to_string(), Vec::new())
        .map_err(crate::err::oom_throwable)?;
    for (field, value) in ["left", "top", "right", "bottom"].into_iter().zip(values) {
        put(vm, recv, field, Value::Int(value))?;
    }
    Ok(recv)
}

/// Platform Insets value object shared with AndroidX's Insets adapter.
pub(crate) fn new_platform_insets(vm: &mut Engine, values: [i32; 4]) -> Result<ObjRef, VmExit> {
    alloc_platform_insets(vm, values)
}

fn platform_insets_static(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let values = match (name, sig) {
        ("of", "(IIII)Landroid/graphics/Insets;") => Some([
            it(args.first()),
            it(args.get(1)),
            it(args.get(2)),
            it(args.get(3)),
        ]),
        ("of", "(Landroid/graphics/Rect;)Landroid/graphics/Insets;") => {
            let Some(rect) = obj(args.first()) else {
                return Err(exc(
                    "Ljava/lang/NullPointerException;",
                    "Insets.of(rect): null rect",
                ));
            };
            Some(["left", "top", "right", "bottom"].map(
                |field| match vm.heap.get_field(rect, field) {
                    Ok(Value::Int(value)) => value,
                    _ => 0,
                },
            ))
        }
        (
            "max",
            "(Landroid/graphics/Insets;Landroid/graphics/Insets;)Landroid/graphics/Insets;",
        )
        | (
            "min",
            "(Landroid/graphics/Insets;Landroid/graphics/Insets;)Landroid/graphics/Insets;",
        )
        | (
            "add",
            "(Landroid/graphics/Insets;Landroid/graphics/Insets;)Landroid/graphics/Insets;",
        ) => {
            let (Some(left), Some(right)) = (obj(args.first()), obj(args.get(1))) else {
                return Err(exc(
                    "Ljava/lang/NullPointerException;",
                    format!("Insets.{name}(insets): null argument"),
                ));
            };
            let (a, b) = (insets_values(vm, left), insets_values(vm, right));
            Some(std::array::from_fn(|i| match name {
                "max" => a[i].max(b[i]),
                "min" => a[i].min(b[i]),
                _ => a[i].saturating_add(b[i]),
            }))
        }
        (
            "subtract",
            "(Landroid/graphics/Insets;Landroid/graphics/Insets;)Landroid/graphics/Insets;",
        ) => {
            let (Some(left), Some(right)) = (obj(args.first()), obj(args.get(1))) else {
                return Err(exc(
                    "Ljava/lang/NullPointerException;",
                    "Insets.subtract(insets): null argument",
                ));
            };
            let (a, b) = (insets_values(vm, left), insets_values(vm, right));
            Some(std::array::from_fn(|i| a[i].saturating_sub(b[i])))
        }
        _ => return Ok(None),
    };
    let Some(values) = values else {
        return Ok(None);
    };
    Ok(Some(Value::Obj(alloc_platform_insets(vm, values)?)))
}

pub(crate) fn static_intrinsic(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if !GFX_PREFIXES.iter().any(|p| class.starts_with(p)) {
        return Ok(None);
    }
    if class == PLATFORM_INSETS {
        return platform_insets_static(vm, name, sig, args);
    }
    if let Some((_, names)) = ENUMS.iter().find(|(c, _)| *c == class) {
        return match name {
            "values" => {
                let mut v = Vec::new();
                for i in 0..names.len() {
                    v.push(enum_const(vm, class, i)?.unwrap_or(Value::Null));
                }
                Ok(Some(crate::arrays::new_array(
                    vm,
                    &format!("[{class}"),
                    ElemKind::Obj,
                    v,
                )?))
            }
            "valueOf" => {
                let n = cs_text(vm, args.last().unwrap_or(&Value::Null))?;
                match names.iter().position(|x| *x == n) {
                    Some(i) => enum_const(vm, class, i),
                    None => Err(exc(
                        "Ljava/lang/IllegalArgumentException;",
                        format!("No enum constant {n}"),
                    )),
                }
            }
            _ => Ok(None),
        };
    }
    let a = |i: usize| args.get(i);
    Ok(Some(match (class, name) {
        ("Landroid/graphics/Typeface;", "create" | "defaultFromStyle") => {
            let style = args
                .iter()
                .rev()
                .find_map(|v| {
                    if let Value::Int(i) = v {
                        Some(*i)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            typeface(vm, style & 1)?
        }
        ("Landroid/graphics/Typeface;", "createFromFile" | "createFromAsset") => typeface(vm, 0)?,
        ("Landroid/graphics/Color;", n) => {
            let c = it(a(0)) as u32;
            match (n, sig) {
                ("alpha", "(I)I") => Value::Int((c >> 24) as i32),
                ("red", "(I)I") => Value::Int(((c >> 16) & 0xFF) as i32),
                ("green", "(I)I") => Value::Int(((c >> 8) & 0xFF) as i32),
                ("blue", "(I)I") => Value::Int((c & 0xFF) as i32),
                ("argb", "(IIII)I") => Value::Int(
                    (((it(a(0)) as u32 & 0xFF) << 24)
                        | ((it(a(1)) as u32 & 0xFF) << 16)
                        | ((it(a(2)) as u32 & 0xFF) << 8)
                        | (it(a(3)) as u32 & 0xFF)) as i32,
                ),
                ("rgb", "(III)I") => Value::Int(
                    (0xFF00_0000u32
                        | ((it(a(0)) as u32 & 0xFF) << 16)
                        | ((it(a(1)) as u32 & 0xFF) << 8)
                        | (it(a(2)) as u32 & 0xFF)) as i32,
                ),
                ("argb", "(FFFF)I") => {
                    let q = |i: usize| (fl(a(i)).clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
                    Value::Int(((q(0) << 24) | (q(1) << 16) | (q(2) << 8) | q(3)) as i32)
                }
                ("parseColor", _) => {
                    let s = cs_text(vm, a(0).unwrap_or(&Value::Null))?;
                    let h = s.trim_start_matches('#');
                    let v = u32::from_str_radix(h, 16).map_err(|_| {
                        exc("Ljava/lang/IllegalArgumentException;", "Unknown color")
                    })?;
                    Value::Int(if h.len() == 6 { 0xFF00_0000 | v } else { v } as i32)
                }
                _ => return Ok(None),
            }
        }
        ("Landroid/graphics/Bitmap;", "createBitmap" | "createScaledBitmap") => {
            // Pick the `int` parameters by descriptor: a null reference
            // (e.g. the `DisplayMetrics` of the API 26 overload) also
            // arrives as `Int(0)` and must not be read as a width.
            let types = crate::engine::parse_param_types(sig);
            let ints: Vec<i32> = args
                .iter()
                .enumerate()
                .filter_map(|(i, v)| match (v, &types) {
                    (Value::Int(x), Some(t)) if t.get(i).map(String::as_str) == Some("I") => {
                        Some(*x)
                    }
                    (Value::Int(x), None) => Some(*x),
                    _ => None,
                })
                .collect();
            let src = args.iter().find_map(|v| match v {
                Value::Obj(r) if matches!(vm.gfx.objs.get(r), Some(G::Bitmap(..))) => Some(*r),
                _ => None,
            });
            let (w, h) = match (name, src) {
                ("createScaledBitmap", _) => (
                    ints.first().copied().unwrap_or(1),
                    ints.get(1).copied().unwrap_or(1),
                ),
                (_, Some(_)) if ints.len() >= 4 => (ints[2], ints[3]),
                (_, Some(s)) => match vm.gfx.objs.get(&s) {
                    Some(G::Bitmap(w, h, _)) => (*w, *h),
                    _ => (1, 1),
                },
                _ => (
                    ints.first().copied().unwrap_or(1),
                    ints.get(1).copied().unwrap_or(1),
                ),
            };
            if w <= 0 || h <= 0 {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "width and height must be > 0",
                ));
            }
            let total_bytes = i64::from(w).saturating_mul(i64::from(h)).saturating_mul(4);
            if w > MAX_BITMAP_DIM || h > MAX_BITMAP_DIM || total_bytes > MAX_BITMAP_BYTES {
                return Err(exc(
                    "Ljava/lang/IllegalArgumentException;",
                    "Bitmap dimensions exceed maximum supported bounds",
                ));
            }
            let ops = match src.and_then(|s| vm.gfx.objs.get(&s)) {
                Some(G::Bitmap(_, _, o)) if o.len() <= MAX_DISPLAY_LIST_OPS => o.clone(),
                Some(G::Bitmap(_, _, o)) => o[..MAX_DISPLAY_LIST_OPS].to_vec(),
                _ => Vec::new(),
            };
            let r = vm
                .heap
                .alloc_instance("Landroid/graphics/Bitmap;".into(), Vec::new())?;
            vm.gfx.objs.insert(r, G::Bitmap(w, h, ops));
            Value::Obj(r)
        }
        ("Landroid/text/StaticLayout$Builder;", "obtain") => {
            let r = vm.heap.alloc_instance(class.to_string(), Vec::new())?;
            vm.gfx.objs.insert(
                r,
                G::Builder(BuilderS {
                    cs: a(0).cloned().unwrap_or(Value::Null),
                    start: it(a(1)),
                    end: it(a(2)),
                    paint: a(3).cloned().unwrap_or(Value::Null),
                    width: it(a(4)),
                    spacing_mult: 1.0,
                    align: 0,
                    max_lines: 0,
                    ellipsize: false,
                    spacing_add: 0.0,
                }),
            );
            Value::Obj(r)
        }
        ("Landroid/text/BoringLayout;", "isBoring") => {
            let text = cs_text(vm, a(0).unwrap_or(&Value::Null))?;
            if text.contains('\n') {
                Value::Null
            } else {
                let p = paint_of(vm, a(1));
                let m = match args.iter().skip(2).rev().find_map(|v| {
                    obj(Some(v)).filter(|r| {
                        vm.heap
                            .class_of(*r)
                            .map(|c| c.contains("Metrics"))
                            .unwrap_or(false)
                    })
                }) {
                    Some(m) => m,
                    None => vm
                        .heap
                        .alloc_instance("Landroid/text/BoringLayout$Metrics;".into(), Vec::new())?,
                };
                fill_metrics(vm, m, p.text_size, true)?;
                put(
                    vm,
                    m,
                    "width",
                    Value::Int(measure(&text, p.text_size, p.letter_spacing).ceil() as i32),
                )?;
                Value::Obj(m)
            }
        }
        ("Landroid/text/Layout;" | "Landroid/text/StaticLayout;", "getDesiredWidth") => {
            let text = cs_text(vm, a(0).unwrap_or(&Value::Null))?;
            let (t, p) = if args.len() >= 4 {
                (sub(&text, it(a(1)), it(a(2))), paint_of(vm, a(3)))
            } else {
                (text, paint_of(vm, a(1)))
            };
            Value::Float(
                t.split('\n')
                    .map(|l| measure(l, p.text_size, p.letter_spacing))
                    .fold(0.0, f32::max),
            )
        }
        _ => return Ok(None),
    }))
}

fn fill_metrics(vm: &mut Engine, m: ObjRef, size: f32, int: bool) -> Result<(), VmExit> {
    let (a, d) = (ascent(size), descent(size));
    let v = |x: f32| {
        if int {
            Value::Int(if x < 0.0 { x.floor() } else { x.ceil() } as i32)
        } else {
            Value::Float(x)
        }
    };
    put(vm, m, "ascent", v(a))?;
    put(vm, m, "descent", v(d))?;
    put(vm, m, "top", v(a * 1.1))?;
    put(vm, m, "bottom", v(d * 1.1))?;
    put(vm, m, "leading", v(0.0))
}

/// Instance methods. Gate: host graphics/text class in the method ref, or a
/// receiver that already has native graphics state (DEX subclasses).
pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if let Some(v) = color_filter(vm, recv_class, name, recv, args)? {
        return Ok(Some(v));
    }
    if let Some(v) = xfermode(vm, recv_class, name, recv, args)? {
        return Ok(Some(v));
    }
    let host_ref = GFX_PREFIXES.iter().any(|p| mref_class.starts_with(p));
    let insets_ref = recv_class == PLATFORM_INSETS || mref_class == PLATFORM_INSETS;
    if !host_ref && !insets_ref && !vm.gfx.objs.contains_key(&recv) {
        return Ok(None);
    }
    let class = if insets_ref {
        PLATFORM_INSETS
    } else if host_ref {
        mref_class
    } else {
        recv_class
    };
    let a = |i: usize| args.get(i);
    if class == PLATFORM_INSETS {
        let fields = insets_values(vm, recv);
        return Ok(Some(match name {
            "<init>" => {
                if args.len() == 4 {
                    for (field, value) in ["left", "top", "right", "bottom"].into_iter().zip([
                        it(a(0)),
                        it(a(1)),
                        it(a(2)),
                        it(a(3)),
                    ]) {
                        put(vm, recv, field, Value::Int(value))?;
                    }
                }
                Value::Null
            }
            "equals" => match obj(a(0)) {
                Some(other) => b(insets_values(vm, other) == fields),
                None => b(false),
            },
            "hashCode" => Value::Int(fields.into_iter().fold(1i32, |hash, value| {
                hash.wrapping_mul(31).wrapping_add(value)
            })),
            "toString" => Value::Obj(crate::intrinsics::alloc_string(
                vm,
                format!(
                    "Insets{{left={}, top={}, right={}, bottom={}}}",
                    fields[0], fields[1], fields[2], fields[3]
                ),
            )?),
            _ => return Ok(None),
        }));
    }
    // enums
    if ENUMS.iter().any(|(c, _)| *c == class || *c == recv_class) {
        return Ok(Some(match name {
            "ordinal" => Value::Int(ord(vm, Some(&Value::Obj(recv)))),
            "name" | "toString" => vm.heap.get_field(recv, "rd_name").unwrap_or(Value::Null),
            "hashCode" => Value::Int(recv as i32),
            "equals" => b(obj(a(0)) == Some(recv)),
            "compareTo" => Value::Int(ord(vm, Some(&Value::Obj(recv))) - ord(vm, a(0))),
            _ => return Ok(None),
        }));
    }
    // heap-field structs
    match class {
        "Landroid/graphics/Rect;" | "Landroid/graphics/RectF;" => {
            return rect_method(
                vm,
                class == "Landroid/graphics/Rect;",
                name,
                sig,
                recv,
                args,
            )
        }
        "Landroid/graphics/Point;" | "Landroid/graphics/PointF;" => {
            let int = class.ends_with("Point;");
            let v = |x: f32| {
                if int {
                    Value::Int(x as i32)
                } else {
                    Value::Float(x)
                }
            };
            return Ok(Some(match name {
                "<init>" | "set" => {
                    if args.len() == 2 {
                        put(vm, recv, "x", v(fl(a(0))))?;
                        put(vm, recv, "y", v(fl(a(1))))?;
                    } else if let Some(o) = obj(a(0)) {
                        let (x, y) = (getf(vm, o, "x"), getf(vm, o, "y"));
                        put(vm, recv, "x", v(x))?;
                        put(vm, recv, "y", v(y))?;
                    } else {
                        put(vm, recv, "x", v(0.0))?;
                        put(vm, recv, "y", v(0.0))?;
                    }
                    Value::Null
                }
                "offset" => {
                    let (x, y) = (
                        getf(vm, recv, "x") + fl(a(0)),
                        getf(vm, recv, "y") + fl(a(1)),
                    );
                    put(vm, recv, "x", v(x))?;
                    put(vm, recv, "y", v(y))?;
                    Value::Null
                }
                "equals" => match obj(a(0)) {
                    Some(o) => b(getf(vm, o, "x") == getf(vm, recv, "x")
                        && getf(vm, o, "y") == getf(vm, recv, "y")),
                    None => b(false),
                },
                "hashCode" => Value::Int(
                    (getf(vm, recv, "x") as i32)
                        .wrapping_mul(31)
                        .wrapping_add(getf(vm, recv, "y") as i32),
                ),
                _ => return Ok(None),
            }));
        }
        "Landroid/graphics/Paint$FontMetricsInt;"
        | "Landroid/graphics/Paint$FontMetrics;"
        | "Landroid/text/BoringLayout$Metrics;"
            if name == "<init>" =>
        {
            for f in ["ascent", "descent", "top", "bottom", "leading"] {
                put(
                    vm,
                    recv,
                    f,
                    if class.ends_with("FontMetrics;") {
                        Value::Float(0.0)
                    } else {
                        Value::Int(0)
                    },
                )?;
            }
            if class.ends_with("Metrics;") && class.contains("Boring") {
                put(vm, recv, "width", Value::Int(0))?;
            }
            return Ok(Some(Value::Null));
        }
        "Landroid/graphics/Typeface;" => {
            let s = match vm.heap.get_field(recv, "rd_style") {
                Ok(Value::Int(i)) => i,
                _ => 0,
            };
            return Ok(Some(match name {
                "getStyle" => Value::Int(s),
                "isBold" => b(s & 1 != 0),
                "isItalic" => b(s & 2 != 0),
                "getWeight" => Value::Int(if s & 1 != 0 { 700 } else { 400 }),
                "equals" => b(obj(a(0)) == Some(recv)),
                "hashCode" => Value::Int(s),
                _ => return Ok(None),
            }));
        }
        _ => {}
    }
    // constructors that create native state
    if name == "<init>" {
        let g = match class {
            "Landroid/graphics/Paint;" | "Landroid/text/TextPaint;" => {
                let mut p = match obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)) {
                    Some(G::Paint(p)) => p.clone(),
                    _ => PaintS::default(),
                };
                if let Some(Value::Int(f)) = a(0) {
                    p.flags = *f;
                }
                G::Paint(p)
            }
            "Landroid/graphics/Path;" => G::Path(path_of(vm, obj(a(0))), false),
            "Landroid/graphics/Matrix;" => G::Matrix(matrix_of(vm, obj(a(0)))),
            "Landroid/graphics/Canvas;" => match obj(a(0)) {
                Some(bm) => {
                    let (w, h) = match vm.gfx.objs.get(&bm) {
                        Some(G::Bitmap(w, h, _)) => (*w, *h),
                        _ => (0, 0),
                    };
                    G::Canvas(CanvasS::new(Target::Bitmap(bm), w, h))
                }
                None => G::Canvas(CanvasS::new(Target::None, 0, 0)),
            },
            "Landroid/graphics/RenderNode;" => G::Node(NodeS::default()),
            "Landroid/graphics/Outline;" => G::Outline(None, 0.0, 1.0),
            "Landroid/text/StaticLayout;"
            | "Landroid/text/BoringLayout;"
            | "Landroid/text/DynamicLayout;" => {
                // (CharSequence, TextPaint, int width, Alignment, float mult, float add, …)
                let cs = a(0).cloned().unwrap_or(Value::Null);
                let text = cs_text(vm, &cs)?;
                let paint = a(1).cloned().unwrap_or(Value::Null);
                let al = ord(vm, a(3)).max(0);
                let ell = class.ends_with("BoringLayout;")
                    && args.len() > 8
                    && !matches!(a(8), Some(Value::Null));
                G::Layout(new_layout(
                    vm,
                    cs,
                    text,
                    paint,
                    it(a(2)),
                    al,
                    if ell { 1 } else { 0 },
                    ell,
                    fl(a(4)),
                    fl(a(5)),
                ))
            }
            _ => return Ok(None),
        };
        vm.gfx.objs.insert(recv, g);
        return Ok(Some(Value::Null));
    }
    let Some(g) = vm.gfx.objs.get(&recv).cloned() else {
        // host graphics object created without our constructor (e.g. by reflection)
        return Ok(match (class, name) {
            (
                "Landroid/graphics/Canvas;" | "Landroid/graphics/RecordingCanvas;",
                "isHardwareAccelerated",
            ) => Some(b(false)),
            _ => None,
        });
    };
    match g {
        G::Paint(p) => paint_method(vm, p, name, sig, recv, args),
        G::Path(..) => path_method(vm, name, sig, recv, args),
        G::Matrix(m) => matrix_method(vm, m, name, sig, recv, args),
        G::Canvas(_) => canvas_method(vm, name, sig, recv, args),
        G::Node(n) => node_method(vm, n, name, sig, recv, args),
        G::Bitmap(w, h, _) => Ok(Some(match name {
            "getWidth" => Value::Int(w),
            "getHeight" => Value::Int(h),
            "getRowBytes" => Value::Int((i64::from(w) * 4).clamp(0, i64::from(i32::MAX)) as i32),
            "getByteCount" | "getAllocationByteCount" => Value::Int(
                i64::from(w)
                    .saturating_mul(i64::from(h))
                    .saturating_mul(4)
                    .clamp(0, i64::from(i32::MAX)) as i32,
            ),
            "isMutable" | "hasAlpha" => b(true),
            "isRecycled" | "compress" | "isPremultiplied" => b(name == "isPremultiplied"),
            "getConfig" => {
                enum_const(vm, "Landroid/graphics/Bitmap$Config;", 3)?.unwrap_or(Value::Null)
            }
            "getDensity" => Value::Int(280),
            "getGenerationId" => Value::Int(recv as i32),
            "getColorSpace" => Value::Null,
            "eraseColor" => {
                if let Some(G::Bitmap(_, _, ops)) = vm.gfx.objs.get_mut(&recv) {
                    ops.clear();
                    ops.push(Op {
                        m: ID,
                        clip: [0.0, 0.0, w as f32, h as f32],
                        alpha: 1.0,
                        p: PaintSnap {
                            argb: it(a(0)) as u32,
                            style: 0,
                            sw: 0.0,
                            clear: false,
                            tint: None,
                            ls: 0.0,
                        },
                        k: OpK::Color(it(a(0)) as u32),
                    });
                }
                Value::Null
            }
            "copy" => {
                let r = vm
                    .heap
                    .alloc_instance("Landroid/graphics/Bitmap;".into(), Vec::new())?;
                vm.gfx.objs.insert(r, g.clone());
                Value::Obj(r)
            }
            "getPixels" => {
                let Some(arr) = obj(a(0)) else {
                    return Err(exc("Ljava/lang/NullPointerException;", "pixels == null"));
                };
                let (offset, stride, x, y, width, height) = (
                    i64::from(it(a(1))),
                    i64::from(it(a(2))),
                    i64::from(it(a(3))),
                    i64::from(it(a(4))),
                    i64::from(it(a(5))),
                    i64::from(it(a(6))),
                );
                if x < 0
                    || y < 0
                    || width < 0
                    || height < 0
                    || x + width > i64::from(w)
                    || y + height > i64::from(h)
                    || stride.abs() < width
                {
                    return Err(exc(
                        "Ljava/lang/IllegalArgumentException;",
                        "Bitmap.getPixels out of bitmap bounds",
                    ));
                }
                if width > 0 && height > 0 {
                    let arr_len = vm
                        .heap
                        .as_array_elems(arr)
                        .map(|(e, _)| e.len() as i64)
                        .unwrap_or(0);
                    let last_row = offset.saturating_add((height - 1).saturating_mul(stride));
                    if offset < 0
                        || offset.saturating_add(width) > arr_len
                        || last_row < 0
                        || last_row.saturating_add(width) > arr_len
                    {
                        return Err(exc(
                            "Ljava/lang/ArrayIndexOutOfBoundsException;",
                            "Bitmap.getPixels array bounds",
                        ));
                    }
                    if let Ok(e) = vm.heap.array_elems_mut(arr) {
                        for row in 0..height {
                            let start = (offset + row * stride) as usize;
                            let end = start + width as usize;
                            for slot in &mut e[start..end] {
                                *slot = Value::Int(0);
                            }
                        }
                    }
                }
                Value::Null
            }
            "getPixel" => Value::Int(0),
            _ if sig.ends_with(")V") => Value::Null, // recycle, prepareToDraw, setDensity, setHasAlpha, …
            _ => return Ok(None),
        })),
        G::Outline(r, rad, alpha) => Ok(Some(match name {
            "setRect" | "setRoundRect" => {
                let rr = if let Some(o) = obj(a(0)) {
                    rect_of(vm, Some(o))
                } else {
                    [fl(a(0)), fl(a(1)), fl(a(2)), fl(a(3))]
                };
                let radius = if name == "setRoundRect" {
                    fl(args.last())
                } else {
                    0.0
                };
                vm.gfx
                    .objs
                    .insert(recv, G::Outline(Some(rr), radius, alpha));
                Value::Null
            }
            "setOval" => {
                let rr = if let Some(o) = obj(a(0)) {
                    rect_of(vm, Some(o))
                } else {
                    [fl(a(0)), fl(a(1)), fl(a(2)), fl(a(3))]
                };
                vm.gfx.objs.insert(
                    recv,
                    G::Outline(Some(rr), (rr[2] - rr[0]).min(rr[3] - rr[1]) / 2.0, alpha),
                );
                Value::Null
            }
            "setConvexPath" | "setPath" => {
                let pb = path_bounds(&path_of(vm, obj(a(0))));
                vm.gfx.objs.insert(recv, G::Outline(Some(pb), 0.0, alpha));
                Value::Null
            }
            "setEmpty" => {
                vm.gfx.objs.insert(recv, G::Outline(None, 0.0, alpha));
                Value::Null
            }
            "setAlpha" => {
                vm.gfx.objs.insert(recv, G::Outline(r, rad, fl(a(0))));
                Value::Null
            }
            "set" => {
                if let Some(o @ G::Outline(..)) =
                    obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)).cloned()
                {
                    vm.gfx.objs.insert(recv, o);
                }
                Value::Null
            }
            "getAlpha" => Value::Float(alpha),
            "getRadius" => Value::Float(rad),
            "canClip" => b(r.is_some()),
            "isEmpty" => b(r.is_none()),
            "getRect" => {
                if let (Some(o), Some(rr)) = (obj(a(0)), r) {
                    set_rect(vm, o, rr, true)?;
                }
                b(r.is_some())
            }
            _ => return Ok(None),
        })),
        G::Builder(mut bs) => {
            if name == "build" {
                let text = cs_text(vm, &bs.cs)?;
                let t = sub(
                    &text,
                    bs.start,
                    if bs.end <= 0 {
                        text.chars().count() as i32
                    } else {
                        bs.end
                    },
                );
                let l = new_layout(
                    vm,
                    bs.cs.clone(),
                    t,
                    bs.paint.clone(),
                    bs.width,
                    bs.align,
                    bs.max_lines,
                    bs.ellipsize,
                    bs.spacing_mult,
                    bs.spacing_add,
                );
                let r = vm
                    .heap
                    .alloc_instance("Landroid/text/StaticLayout;".into(), Vec::new())?;
                vm.gfx.objs.insert(r, G::Layout(l));
                return Ok(Some(Value::Obj(r)));
            }
            match name {
                "setAlignment" => bs.align = ord(vm, a(0)).max(0),
                "setMaxLines" => bs.max_lines = it(a(0)),
                "setEllipsize" => bs.ellipsize = obj(a(0)).is_some(),
                "setLineSpacing" => {
                    bs.spacing_add = fl(a(0));
                    bs.spacing_mult = fl(a(1));
                }
                "setWidth" => bs.width = it(a(0)),
                "setText" => {
                    bs.cs = a(0).cloned().unwrap_or(Value::Null);
                    bs.start = it(a(1));
                    bs.end = it(a(2));
                }
                _ => {}
            }
            vm.gfx.objs.insert(recv, G::Builder(bs));
            Ok(Some(Value::Obj(recv)))
        }
        G::Layout(l) => layout_method(vm, &l, name, sig, recv, args),
    }
}

fn rect_method(
    vm: &mut Engine,
    int: bool,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let r = rect_of(vm, Some(recv));
    let num = |x: f32| {
        if int {
            Value::Int(x as i32)
        } else {
            Value::Float(x)
        }
    };
    Ok(Some(match name {
        "<init>" | "set" => {
            let v = match (args.len(), obj(a(0))) {
                (4, _) => [fl(a(0)), fl(a(1)), fl(a(2)), fl(a(3))],
                (1, Some(o)) => rect_of(vm, Some(o)),
                _ => [0.0; 4],
            };
            set_rect(vm, recv, v, int)?;
            Value::Null
        }
        "setEmpty" => {
            set_rect(vm, recv, [0.0; 4], int)?;
            Value::Null
        }
        "width" => num(r[2] - r[0]),
        "height" => num(r[3] - r[1]),
        "centerX" => num(if int {
            ((r[0] + r[2]) / 2.0).floor()
        } else {
            (r[0] + r[2]) / 2.0
        }),
        "centerY" => num(if int {
            ((r[1] + r[3]) / 2.0).floor()
        } else {
            (r[1] + r[3]) / 2.0
        }),
        "exactCenterX" => Value::Float((r[0] + r[2]) / 2.0),
        "exactCenterY" => Value::Float((r[1] + r[3]) / 2.0),
        "isEmpty" => b(r[0] >= r[2] || r[1] >= r[3]),
        "contains" if args.len() == 2 => {
            let (x, y) = (fl(a(0)), fl(a(1)));
            b(r[0] < r[2] && r[1] < r[3] && x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
        }
        "offset" | "offsetTo" | "inset" => {
            let (dx, dy) = (fl(a(0)), fl(a(1)));
            let v = match name {
                "offset" => [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy],
                "offsetTo" => [dx, dy, dx + r[2] - r[0], dy + r[3] - r[1]],
                _ => [r[0] + dx, r[1] + dy, r[2] - dx, r[3] - dy],
            };
            set_rect(vm, recv, v, int)?;
            Value::Null
        }
        "intersect" | "setIntersect" | "intersects" | "union" => {
            let o = if args.len() >= 4 {
                [fl(a(0)), fl(a(1)), fl(a(2)), fl(a(3))]
            } else {
                rect_of(vm, obj(a(0)))
            };
            let o = if name == "setIntersect" {
                let p = rect_of(vm, obj(a(1)));
                let q = o;
                [
                    p[0].max(q[0]),
                    p[1].max(q[1]),
                    p[2].min(q[2]),
                    p[3].min(q[3]),
                ]
            } else {
                o
            };
            if name == "union" {
                set_rect(
                    vm,
                    recv,
                    [
                        r[0].min(o[0]),
                        r[1].min(o[1]),
                        r[2].max(o[2]),
                        r[3].max(o[3]),
                    ],
                    int,
                )?;
                return Ok(Some(Value::Null));
            }
            let i = [
                r[0].max(o[0]),
                r[1].max(o[1]),
                r[2].min(o[2]),
                r[3].min(o[3]),
            ];
            let hit = i[0] < i[2] && i[1] < i[3];
            if hit && name != "intersects" {
                set_rect(vm, recv, if name == "setIntersect" { o } else { i }, int)?;
            }
            b(hit)
        }
        "sort" => {
            set_rect(
                vm,
                recv,
                [
                    r[0].min(r[2]),
                    r[1].min(r[3]),
                    r[0].max(r[2]),
                    r[1].max(r[3]),
                ],
                int,
            )?;
            Value::Null
        }
        "round" | "roundOut" => {
            if let Some(o) = obj(a(0)) {
                let v = if name == "round" {
                    r.map(f32::round)
                } else {
                    [r[0].floor(), r[1].floor(), r[2].ceil(), r[3].ceil()]
                };
                set_rect(vm, o, v, true)?;
            }
            Value::Null
        }
        "equals" => match obj(a(0)) {
            Some(o) if vm.heap.class_of(o).ok() == vm.heap.class_of(recv).ok() => {
                b(rect_of(vm, Some(o)) == r)
            }
            _ => b(false),
        },
        "hashCode" => Value::Int(
            r.iter()
                .fold(0i32, |h, x| h.wrapping_mul(31).wrapping_add(*x as i32)),
        ),
        "toString" | "toShortString" => {
            st(vm, format!("Rect({}, {} - {}, {})", r[0], r[1], r[2], r[3]))?
        }
        _ => {
            let _ = sig;
            return Ok(None);
        }
    }))
}

fn paint_method(
    vm: &mut Engine,
    mut p: PaintS,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let mut changed = true;
    let out = match name {
        "setColor" => {
            p.argb = match a(0) {
                Some(Value::Long(l)) => {
                    // packed ColorLong (sRGB variant): ARGB in the high 32 bits
                    ((*l as u64) >> 32) as u32
                }
                v => it(v) as u32,
            };
            Value::Null
        }
        "getColor" => Value::Int(p.argb as i32),
        "setAlpha" => {
            p.argb = (p.argb & 0x00FF_FFFF) | ((it(a(0)).clamp(0, 255) as u32) << 24);
            Value::Null
        }
        "getAlpha" => Value::Int((p.argb >> 24) as i32),
        "setARGB" => {
            p.argb = ((it(a(0)) as u32 & 0xFF) << 24)
                | ((it(a(1)) as u32 & 0xFF) << 16)
                | ((it(a(2)) as u32 & 0xFF) << 8)
                | (it(a(3)) as u32 & 0xFF);
            Value::Null
        }
        "setStyle" => {
            p.style = ord(vm, a(0)).max(0);
            Value::Null
        }
        "getStyle" => enum_const(vm, "Landroid/graphics/Paint$Style;", p.style as usize)?
            .unwrap_or(Value::Null),
        "setStrokeWidth" => {
            p.sw = fl(a(0));
            Value::Null
        }
        "getStrokeWidth" => Value::Float(p.sw),
        "setTextSize" => {
            p.text_size = fl(a(0)).max(0.0);
            Value::Null
        }
        "getTextSize" => Value::Float(p.text_size),
        "setFlags" => {
            p.flags = it(a(0));
            Value::Null
        }
        "getFlags" => Value::Int(p.flags),
        "setAntiAlias" | "setDither" | "setFilterBitmap" | "setUnderlineText"
        | "setStrikeThruText" | "setFakeBoldText" | "setLinearText" | "setSubpixelText" => {
            let bit = match name {
                "setAntiAlias" => 1,
                "setFilterBitmap" => 2,
                "setDither" => 4,
                "setUnderlineText" => 8,
                "setStrikeThruText" => 16,
                "setFakeBoldText" => 32,
                "setLinearText" => 64,
                _ => 128,
            };
            if it(a(0)) != 0 {
                p.flags |= bit
            } else {
                p.flags &= !bit
            }
            Value::Null
        }
        "isAntiAlias" => b(p.flags & 1 != 0),
        "isFilterBitmap" => b(p.flags & 2 != 0),
        "isDither" => b(p.flags & 4 != 0),
        "isUnderlineText" => b(p.flags & 8 != 0),
        "isStrikeThruText" => b(p.flags & 16 != 0),
        "isFakeBoldText" => b(p.flags & 32 != 0),
        "setLetterSpacing" => {
            p.letter_spacing = fl(a(0));
            Value::Null
        }
        "getLetterSpacing" => Value::Float(p.letter_spacing),
        "setTextScaleX" => {
            p.scale_x = fl(a(0));
            Value::Null
        }
        "getTextScaleX" => Value::Float(p.scale_x),
        "setTypeface" => {
            p.typeface = a(0).cloned().unwrap_or(Value::Null);
            p.typeface.clone()
        }
        "getTypeface" => p.typeface.clone(),
        "set" => {
            if let Some(G::Paint(o)) = obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)) {
                p = o.clone();
            }
            Value::Null
        }
        "reset" => {
            p = PaintS::default();
            Value::Null
        }
        "measureText" => {
            let t = if args.len() >= 3 {
                let is_ca = sig.starts_with("([C");
                checked_text_slice(vm, a(0), is_ca, it(a(1)), it(a(2)), is_ca)?
            } else {
                let Some(v0) = a(0).filter(|v| !matches!(v, Value::Null)) else {
                    return Err(exc("Ljava/lang/NullPointerException;", "text == null"));
                };
                cs_text(vm, v0)?
            };
            changed = false;
            Value::Float(measure(&t, p.text_size, p.letter_spacing) * p.scale_x)
        }
        "getTextWidths" | "getTextRunAdvances" => {
            changed = false;
            Value::Float(0.0)
        }
        "breakText" => {
            changed = false;
            let t = cs_text(vm, a(0).unwrap_or(&Value::Null))?;
            let maxw = fl(args.get(args.len().saturating_sub(2)));
            let mut w = 0.0;
            let mut n = 0;
            for c in t.chars() {
                let ad = char_adv(c, p.text_size, p.letter_spacing);
                if w + ad > maxw {
                    break;
                }
                w += ad;
                n += 1;
            }
            Value::Int(n)
        }
        "ascent" => {
            changed = false;
            Value::Float(ascent(p.text_size))
        }
        "descent" => {
            changed = false;
            Value::Float(descent(p.text_size))
        }
        "getFontSpacing" => {
            changed = false;
            Value::Float(descent(p.text_size) - ascent(p.text_size))
        }
        "getFontMetricsInt" | "getFontMetrics" => {
            changed = false;
            let int = name.ends_with("Int");
            let m = match obj(a(0)) {
                Some(m) => m,
                None => vm.heap.alloc_instance(
                    if int {
                        "Landroid/graphics/Paint$FontMetricsInt;"
                    } else {
                        "Landroid/graphics/Paint$FontMetrics;"
                    }
                    .into(),
                    Vec::new(),
                )?,
            };
            fill_metrics(vm, m, p.text_size, int)?;
            let sp = descent(p.text_size) - ascent(p.text_size);
            if a(0).is_some() {
                if int {
                    Value::Int(sp.ceil() as i32)
                } else {
                    Value::Float(sp)
                }
            } else {
                Value::Obj(m)
            }
        }
        "getTextBounds" => {
            changed = false;
            let is_ca = sig.starts_with("([C");
            let t = checked_text_slice(vm, a(0), is_ca, it(a(1)), it(a(2)), is_ca)?;
            if let Some(r) = obj(args.last()) {
                set_rect(
                    vm,
                    r,
                    [
                        0.0,
                        ascent(p.text_size),
                        measure(&t, p.text_size, p.letter_spacing),
                        descent(p.text_size),
                    ],
                    true,
                )?;
            }
            Value::Null
        }
        "hasGlyph" => {
            changed = false;
            let t = cs_text(vm, a(0).unwrap_or(&Value::Null))?;
            b(t.chars().all(|c| atlas().map.contains_key(&(c as u32))))
        }
        "isElegantTextHeight" => b(false),
        "getHinting" => Value::Int(0),
        "getStrokeMiter" => Value::Float(p.misc.get("miter").map(|v| fl(Some(v))).unwrap_or(4.0)),
        "setStrokeMiter" => {
            p.misc
                .insert("miter".into(), a(0).cloned().unwrap_or(Value::Float(4.0)));
            Value::Null
        }
        "getTextSkewX" => Value::Float(p.misc.get("skew").map(|v| fl(Some(v))).unwrap_or(0.0)),
        n if n.starts_with("set") => {
            // Shader, ColorFilter, Xfermode, BlendMode, PathEffect, MaskFilter,
            // StrokeCap/Join, TextAlign, Locale(s), FontFeatureSettings, … —
            // stored for round-trip getters, not rendered.
            let key = n[3..].to_string();
            let v = a(0).cloned().unwrap_or(Value::Null);
            p.misc.insert(key, v.clone());
            if sig.ends_with(")V") {
                Value::Null
            } else {
                v
            }
        }
        n if n.starts_with("get") || n.starts_with("is") => {
            changed = false;
            let key = n
                .trim_start_matches("get")
                .trim_start_matches("is")
                .to_string();
            match p.misc.get(&key) {
                Some(v) => v.clone(),
                None => match sig.rsplit(')').next().unwrap_or("V") {
                    "I" | "Z" => Value::Int(0),
                    "F" => Value::Float(0.0),
                    _ => match key.as_str() {
                        "StrokeCap" => enum_const(vm, "Landroid/graphics/Paint$Cap;", 0)?
                            .unwrap_or(Value::Null),
                        "StrokeJoin" => enum_const(vm, "Landroid/graphics/Paint$Join;", 0)?
                            .unwrap_or(Value::Null),
                        "TextAlign" => enum_const(vm, "Landroid/graphics/Paint$Align;", 0)?
                            .unwrap_or(Value::Null),
                        "TextLocale" => crate::android_res::static_intrinsic(
                            vm,
                            "Ljava/util/Locale;",
                            "getDefault",
                            "()Ljava/util/Locale;",
                            &[],
                        )?
                        .unwrap_or(Value::Null),
                        "TextLocales" => crate::android_res::static_intrinsic(
                            vm,
                            "Landroid/os/LocaleList;",
                            "getDefault",
                            "()Landroid/os/LocaleList;",
                            &[],
                        )?
                        .unwrap_or(Value::Null),
                        _ => Value::Null,
                    },
                },
            }
        }
        _ => return Ok(None),
    };
    if changed {
        vm.gfx.objs.insert(recv, G::Paint(p));
    }
    Ok(Some(out))
}

fn path_method(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let f = |i: usize| fl(args.get(i));
    let dir_ccw = |vm: &Engine, v: Option<&Value>| ord(vm, v) == 1;
    let add_poly = |vm: &mut Engine, mut poly: Vec<(f32, f32)>, ccw: bool| -> Result<(), VmExit> {
        if ccw {
            poly.reverse();
        }
        let p = path_mut(vm, recv);
        if let Some(f0) = poly.first().copied() {
            poly.push(f0);
        }
        ensure_path_budget(p, poly.len(), 1)?;
        p.push(poly);
        Ok(())
    };
    Ok(Some(match name {
        "moveTo" | "rMoveTo" => {
            let p = path_mut(vm, recv);
            ensure_path_budget(p, 1, 1)?;
            let (x0, y0) = if name == "rMoveTo" {
                last_pt(p)
            } else {
                (0.0, 0.0)
            };
            p.push(vec![(x0 + f(0), y0 + f(1))]);
            Value::Null
        }
        "lineTo" | "rLineTo" => {
            let p = path_mut(vm, recv);
            ensure_path_budget(p, 2, usize::from(p.is_empty()))?;
            let (x0, y0) = last_pt(p);
            let (x, y) = if name == "rLineTo" {
                (x0 + f(0), y0 + f(1))
            } else {
                (f(0), f(1))
            };
            if p.is_empty() {
                p.push(vec![(0.0, 0.0)]);
            }
            if let Some(last) = p.last_mut() {
                last.push((x, y));
            }
            Value::Null
        }
        "quadTo" | "rQuadTo" | "cubicTo" | "rCubicTo" | "conicTo" | "rConicTo" => {
            let p = path_mut(vm, recv);
            ensure_path_budget(p, 17, usize::from(p.is_empty()))?;
            let (x0, y0) = last_pt(p);
            let rel = name.starts_with('r');
            let o = |v: f32, axis: usize| {
                if rel {
                    v + if axis == 0 { x0 } else { y0 }
                } else {
                    v
                }
            };
            if p.is_empty() {
                p.push(vec![(0.0, 0.0)]);
            }
            if let Some(sp) = p.last_mut() {
                let n = 16;
                if name.contains("ubic") {
                    let (x1, y1, x2, y2, x3, y3) = (
                        o(f(0), 0),
                        o(f(1), 1),
                        o(f(2), 0),
                        o(f(3), 1),
                        o(f(4), 0),
                        o(f(5), 1),
                    );
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let u = 1.0 - t;
                        sp.push((
                            u * u * u * x0
                                + 3.0 * u * u * t * x1
                                + 3.0 * u * t * t * x2
                                + t * t * t * x3,
                            u * u * u * y0
                                + 3.0 * u * u * t * y1
                                + 3.0 * u * t * t * y2
                                + t * t * t * y3,
                        ));
                    }
                } else {
                    let (x1, y1, x2, y2) = (o(f(0), 0), o(f(1), 1), o(f(2), 0), o(f(3), 1));
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let u = 1.0 - t;
                        sp.push((
                            u * u * x0 + 2.0 * u * t * x1 + t * t * x2,
                            u * u * y0 + 2.0 * u * t * y1 + t * t * y2,
                        ));
                    }
                }
            }
            Value::Null
        }
        "arcTo" | "addArc" => {
            let (r, s0, sw) = if let Some(o) = obj(a(0)) {
                (rect_of(vm, Some(o)), f(1), f(2))
            } else {
                ([f(0), f(1), f(2), f(3)], f(4), f(5))
            };
            let (cx, cy, rx, ry) = (
                (r[0] + r[2]) / 2.0,
                (r[1] + r[3]) / 2.0,
                (r[2] - r[0]) / 2.0,
                (r[3] - r[1]) / 2.0,
            );
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let t = (s0 + sw * i as f32 / 24.0).to_radians();
                    (cx + rx * t.cos(), cy + ry * t.sin())
                })
                .collect();
            let p = path_mut(vm, recv);
            ensure_path_budget(p, pts.len(), 1)?;
            if name == "arcTo" && !p.is_empty() && !(args.last() == Some(&Value::Int(1))) {
                if let Some(last) = p.last_mut() {
                    last.extend(pts);
                }
            } else {
                p.push(pts);
            }
            Value::Null
        }
        "close" => {
            let p = path_mut(vm, recv);
            ensure_path_budget(p, 2, 1)?;
            if let Some(sp) = p.last_mut() {
                if let Some(f0) = sp.first().copied() {
                    sp.push(f0);
                }
                let f0 = sp[0];
                p.push(vec![f0]);
            }
            Value::Null
        }
        "reset" | "rewind" => {
            vm.gfx.objs.insert(recv, G::Path(Vec::new(), false));
            Value::Null
        }
        "set" => {
            let o = path_of(vm, obj(a(0)));
            vm.gfx.objs.insert(recv, G::Path(o, false));
            Value::Null
        }
        "addRect" => {
            let (r, d) = if let Some(o) = obj(a(0)) {
                (rect_of(vm, Some(o)), a(1))
            } else {
                ([f(0), f(1), f(2), f(3)], a(4))
            };
            let ccw = dir_ccw(vm, d);
            add_poly(
                vm,
                vec![(r[0], r[1]), (r[2], r[1]), (r[2], r[3]), (r[0], r[3])],
                ccw,
            )?;
            Value::Null
        }
        "addOval" | "addCircle" => {
            let (r, d) = match (name, obj(a(0))) {
                ("addCircle", _) => ([f(0) - f(2), f(1) - f(2), f(0) + f(2), f(1) + f(2)], a(3)),
                (_, Some(o)) => (rect_of(vm, Some(o)), a(1)),
                _ => ([f(0), f(1), f(2), f(3)], a(4)),
            };
            let ccw = dir_ccw(vm, d);
            add_poly(vm, oval_poly(r[0], r[1], r[2], r[3]), ccw)?;
            Value::Null
        }
        "addRoundRect" => {
            // (RectF,[F,Dir) (RectF,F,F,Dir) (FFFF,[F,Dir) (FFFF,F,F,Dir)
            let (r, rest) =
                if let Some(o) = obj(a(0)).filter(|o| vm.heap.as_array_elems(*o).is_err()) {
                    (rect_of(vm, Some(o)), 1)
                } else {
                    ([f(0), f(1), f(2), f(3)], 4)
                };
            let rad = if sig.contains("[F") {
                let v = float_array(vm, a(rest));
                let mut q = [0.0f32; 8];
                for (i, x) in v.iter().take(8).enumerate() {
                    q[i] = *x;
                }
                q
            } else {
                let (rx, ry) = (f(rest), f(rest + 1));
                [rx, ry, rx, ry, rx, ry, rx, ry]
            };
            let d = args.last();
            let ccw = dir_ccw(vm, d);
            add_poly(vm, rrect_poly(r[0], r[1], r[2], r[3], rad), ccw)?;
            Value::Null
        }
        "addPath" => {
            if matches!(a(0), None | Some(Value::Null)) {
                return Err(exc("Ljava/lang/NullPointerException;", "src == null"));
            }
            let mut o = path_of(vm, obj(a(0)));
            let add_pts = path_point_count(&o);
            let add_contours = o.len();
            let m = if args.len() == 3 {
                tr(f(1), f(2))
            } else if args.len() == 2 {
                matrix_of(vm, obj(a(1)))
            } else {
                ID
            };
            for sp in o.iter_mut() {
                for pt in sp.iter_mut() {
                    *pt = ap(&m, pt.0, pt.1);
                }
            }
            let p = path_mut(vm, recv);
            ensure_path_budget(p, add_pts, add_contours)?;
            p.extend(o);
            Value::Null
        }
        "offset" | "transform" => {
            let m = if name == "offset" {
                tr(f(0), f(1))
            } else {
                matrix_of(vm, obj(a(0)))
            };
            let mut p = path_of(vm, Some(recv));
            for sp in p.iter_mut() {
                for pt in sp.iter_mut() {
                    *pt = ap(&m, pt.0, pt.1);
                }
            }
            let dst = if name == "transform" {
                obj(a(1))
            } else if args.len() == 3 {
                obj(a(2))
            } else {
                None
            };
            vm.gfx.objs.insert(dst.unwrap_or(recv), G::Path(p, false));
            Value::Null
        }
        "op" => {
            // union approximation: (Path, Op) → this ∪ path ; (Path, Path, Op) → this = p1 ∪ p2
            let (p1, p2) = if args.len() == 3 {
                (path_of(vm, obj(a(0))), path_of(vm, obj(a(1))))
            } else {
                (path_of(vm, Some(recv)), path_of(vm, obj(a(0))))
            };
            let opn = ord(vm, args.last());
            let res = match opn {
                0 => p1, // DIFFERENCE: keep first (approx.)
                1 => p1, // INTERSECT approx.
                4 => p2, // REVERSE_DIFFERENCE approx.
                _ => {
                    ensure_path_budget(&p1, path_point_count(&p2), p2.len())?;
                    p1.into_iter().chain(p2).collect()
                }
            };
            vm.gfx.objs.insert(recv, G::Path(res, false));
            b(true)
        }
        "computeBounds" => {
            let bb = path_bounds(&path_of(vm, Some(recv)));
            if let Some(o) = obj(a(0)) {
                set_rect(vm, o, bb, false)?;
            }
            Value::Null
        }
        "isEmpty" => b(path_of(vm, Some(recv)).iter().all(|s| s.len() < 2)),
        "isConvex" => b(true),
        "isRect" => b(false),
        "isInverseFillType" => b(false),
        "setFillType" => {
            let eo = ord(vm, a(0)) % 2 == 1;
            if let Some(G::Path(_, e)) = vm.gfx.objs.get_mut(&recv) {
                *e = eo;
            }
            Value::Null
        }
        "getFillType" => {
            let eo = matches!(vm.gfx.objs.get(&recv), Some(G::Path(_, true)));
            enum_const(vm, "Landroid/graphics/Path$FillType;", eo as usize)?.unwrap_or(Value::Null)
        }
        "incReserve" | "setLastPoint" | "toggleInverseFillType" => Value::Null,
        _ => return Ok(None),
    }))
}

fn matrix_method(
    vm: &mut Engine,
    m: M,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let f = |i: usize| fl(args.get(i));
    let about = |op: M, n: usize| {
        if args.len() >= n + 2 {
            mul(&tr(f(n), f(n + 1)), &mul(&op, &tr(-f(n), -f(n + 1))))
        } else {
            op
        }
    };
    let op_of = |name: &str| -> M {
        let base = name
            .trim_start_matches("set")
            .trim_start_matches("pre")
            .trim_start_matches("post");
        match base {
            "Translate" => tr(f(0), f(1)),
            "Scale" => about(sc(f(0), f(1)), 2),
            "Rotate" => about(rot(f(0)), 1),
            "Skew" => about([1.0, f(1), f(0), 1.0, 0.0, 0.0], 2),
            _ => ID,
        }
    };
    let set = |vm: &mut Engine, nm: M| {
        set_matrix(vm, recv, nm);
        Value::Int(1)
    };
    Ok(Some(match name {
        "reset" => {
            set(vm, ID);
            Value::Null
        }
        "set" => {
            let o = matrix_of(vm, obj(a(0)));
            set(vm, o);
            Value::Null
        }
        "setTranslate" | "setScale" | "setRotate" | "setSkew" => {
            set(vm, op_of(name));
            Value::Null
        }
        "preTranslate" | "preScale" | "preRotate" | "preSkew" => set(vm, mul(&m, &op_of(name))),
        "postTranslate" | "postScale" | "postRotate" | "postSkew" => set(vm, mul(&op_of(name), &m)),
        "preConcat" => {
            let o = matrix_of(vm, obj(a(0)));
            set(vm, mul(&m, &o))
        }
        "postConcat" => {
            let o = matrix_of(vm, obj(a(0)));
            set(vm, mul(&o, &m))
        }
        "setConcat" => {
            let (p, q) = (matrix_of(vm, obj(a(0))), matrix_of(vm, obj(a(1))));
            set(vm, mul(&p, &q))
        }
        "invert" => match inv(&m) {
            Some(i) => {
                if let Some(o) = obj(a(0)) {
                    set_matrix(vm, o, i);
                }
                b(true)
            }
            None => b(false),
        },
        "isIdentity" => b(m == ID),
        "isAffine" => b(true),
        "rectStaysRect" => b(m[1] == 0.0 && m[2] == 0.0 || m[0] == 0.0 && m[3] == 0.0),
        "getValues" => {
            write_floats(vm, a(0), &to9(&m), 0);
            Value::Null
        }
        "setValues" => {
            let v = float_array(vm, a(0));
            if v.len() < 9 {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "setValues",
                ));
            }
            let mut q = [0.0; 9];
            q.copy_from_slice(&v[..9]);
            set_matrix(vm, recv, from9(&q));
            Value::Null
        }
        "mapPoints" => {
            // mapPoints([F) | mapPoints(dst,src) | mapPoints(dst,dstIndex,src,srcIndex,count)
            // AOSP Matrix.mapPoints validates via checkPointArrays and the
            // two-array overload requires equal lengths (AIOOBE otherwise).
            let (dst, src, di, si) = match args.len() {
                1 => (a(0), a(0), 0i32, 0i32),
                2 => (a(0), a(1), 0, 0),
                _ => (a(0), a(2), it(a(1)), it(a(3))),
            };
            if matches!(dst, None | Some(Value::Null)) || matches!(src, None | Some(Value::Null)) {
                return Err(exc("Ljava/lang/NullPointerException;", "mapPoints"));
            }
            let src_len = float_array_len(vm, src);
            let dst_len = float_array_len(vm, dst);
            let count = if args.len() <= 2 {
                src_len / 2
            } else {
                it(a(4))
            };
            if args.len() == 2 && dst_len != src_len {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "mapPoints",
                ));
            }
            if count < 0 || si < 0 || di < 0 {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "mapPoints",
                ));
            }
            let point_floats = (count as usize).saturating_mul(2);
            let (si, di) = (si as usize, di as usize);
            let (src_len_u, dst_len_u) = (src_len.max(0) as usize, dst_len.max(0) as usize);
            if si > src_len_u
                || point_floats > src_len_u - si
                || di > dst_len_u
                || point_floats > dst_len_u - di
            {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "mapPoints",
                ));
            }
            let v = float_array(vm, src);
            if si > v.len() || point_floats > v.len() - si {
                return Err(exc(
                    "Ljava/lang/ArrayIndexOutOfBoundsException;",
                    "mapPoints",
                ));
            }
            let pts: Vec<f32> = v[si..si + point_floats]
                .chunks(2)
                .flat_map(|c| {
                    let (x, y) = ap(&m, c[0], *c.get(1).unwrap_or(&0.0));
                    [x, y]
                })
                .collect();
            write_floats(vm, dst, &pts, di);
            Value::Null
        }
        "mapRect" => {
            let (dst, src) = if args.len() == 2 {
                (obj(a(0)), obj(a(1)))
            } else {
                (obj(a(0)), obj(a(0)))
            };
            let r = rect_of(vm, src);
            let c = [
                ap(&m, r[0], r[1]),
                ap(&m, r[2], r[1]),
                ap(&m, r[2], r[3]),
                ap(&m, r[0], r[3]),
            ];
            let bb = [
                c.iter().map(|p| p.0).fold(f32::MAX, f32::min),
                c.iter().map(|p| p.1).fold(f32::MAX, f32::min),
                c.iter().map(|p| p.0).fold(f32::MIN, f32::max),
                c.iter().map(|p| p.1).fold(f32::MIN, f32::max),
            ];
            if let Some(d) = dst {
                set_rect(vm, d, bb, false)?;
            }
            b(m[1] == 0.0 && m[2] == 0.0)
        }
        "setRectToRect" => {
            // SkMatrix::setRectToRect (AOSP native semantics): empty src ->
            // identity + false; empty dst -> zero matrix + true; FILL=0,
            // START=1, CENTER=2 (half diff), END=3 (full diff on the larger
            // axis only).
            if obj(a(0)).is_none() || obj(a(1)).is_none() {
                return Err(exc("Ljava/lang/NullPointerException;", "setRectToRect"));
            }
            let (s, d) = (rect_of(vm, obj(a(0))), rect_of(vm, obj(a(1))));
            if s[0] >= s[2] || s[1] >= s[3] {
                set(vm, ID);
                b(false)
            } else if d[0] >= d[2] || d[1] >= d[3] {
                set_matrix(vm, recv, [0.0; 6]);
                b(true)
            } else {
                let align = ord(vm, a(2));
                let (sw, sh) = (s[2] - s[0], s[3] - s[1]);
                let (dw, dh) = (d[2] - d[0], d[3] - d[1]);
                let (mut sx, mut sy) = (dw / sw, dh / sh);
                let mut x_larger = false;
                if align != 0 {
                    if sx > sy {
                        x_larger = true;
                        sx = sy;
                    } else {
                        sy = sx;
                    }
                }
                let mut tx = d[0] - s[0] * sx;
                let mut ty = d[1] - s[1] * sy;
                if align == 2 || align == 3 {
                    let mut diff = if x_larger { dw - sw * sy } else { dh - sh * sy };
                    if align == 2 {
                        diff *= 0.5;
                    }
                    if x_larger {
                        tx += diff;
                    } else {
                        ty += diff;
                    }
                }
                set(vm, mul(&tr(tx, ty), &sc(sx, sy)));
                b(true)
            }
        }
        "equals" => {
            b(matches!(obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)), Some(G::Matrix(o)) if *o == m))
        }
        // AOSP Matrix.hashCode literally returns 44 ("we take the easy way out").
        "hashCode" => Value::Int(44),
        _ => return Ok(None),
    }))
}

fn canvas_method(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let f = |i: usize| fl(args.get(i));
    fn cvf(vm: &mut Engine, recv: ObjRef) -> &mut CanvasS {
        match vm.gfx.objs.get_mut(&recv) {
            Some(G::Canvas(c)) => c,
            _ => unreachable!(),
        }
    }
    let concat = |vm: &mut Engine, m: M| {
        let t = cvf(vm, recv).top();
        t.m = mul(&t.m, &m);
    };
    let paint_last = |vm: &Engine| snap(vm, &paint_of(vm, args.last()));
    let out = match name {
        "save" => {
            let c = cvf(vm, recv);
            if c.stack.len() >= MAX_CANVAS_SAVE_DEPTH {
                return Err(exc(
                    "Ljava/lang/OutOfMemoryError;",
                    "Canvas save stack exceeds maximum depth",
                ));
            }
            let s = *c.top();
            c.stack.push(s);
            Value::Int(c.stack.len() as i32 - 1)
        }
        "saveLayer" | "saveLayerAlpha" => {
            let alpha = if name == "saveLayerAlpha" {
                let ai = args
                    .iter()
                    .filter_map(|v| {
                        if let Value::Int(i) = v {
                            Some(*i)
                        } else {
                            None
                        }
                    })
                    .next()
                    .unwrap_or(255);
                ai as f32 / 255.0
            } else {
                let p = args.iter().find_map(|v| match v {
                    Value::Obj(r) if matches!(vm.gfx.objs.get(r), Some(G::Paint(_))) => Some(*r),
                    _ => None,
                });
                p.map(|p| (paint_of(vm, Some(&Value::Obj(p))).argb >> 24) as f32 / 255.0)
                    .unwrap_or(1.0)
            };
            let c = cvf(vm, recv);
            if c.stack.len() >= MAX_CANVAS_SAVE_DEPTH {
                return Err(exc(
                    "Ljava/lang/OutOfMemoryError;",
                    "Canvas save stack exceeds maximum depth",
                ));
            }
            let mut s = *c.top();
            s.alpha *= alpha;
            c.stack.push(s);
            Value::Int(c.stack.len() as i32 - 1)
        }
        "restore" => {
            let c = cvf(vm, recv);
            if c.stack.len() > 1 {
                c.stack.pop();
            } else {
                return Err(exc(
                    "Ljava/lang/IllegalStateException;",
                    "Underflow in restore - more restores than saves",
                ));
            }
            Value::Null
        }
        "restoreToCount" => {
            let n = it(a(0)).max(1) as usize;
            let c = cvf(vm, recv);
            c.stack.truncate(n.max(1));
            Value::Null
        }
        "getSaveCount" => Value::Int(cvf(vm, recv).stack.len() as i32),
        "translate" => {
            concat(vm, tr(f(0), f(1)));
            Value::Null
        }
        "scale" => {
            let m = if args.len() == 4 {
                mul(&tr(f(2), f(3)), &mul(&sc(f(0), f(1)), &tr(-f(2), -f(3))))
            } else {
                sc(f(0), f(1))
            };
            concat(vm, m);
            Value::Null
        }
        "rotate" => {
            let m = if args.len() == 3 {
                mul(&tr(f(1), f(2)), &mul(&rot(f(0)), &tr(-f(1), -f(2))))
            } else {
                rot(f(0))
            };
            concat(vm, m);
            Value::Null
        }
        "skew" => {
            concat(vm, [1.0, f(1), f(0), 1.0, 0.0, 0.0]);
            Value::Null
        }
        "concat" | "setMatrix" => {
            let m = matrix_of(vm, obj(a(0)));
            if name == "concat" {
                concat(vm, m);
            } else {
                cvf(vm, recv).top().m = m;
            }
            Value::Null
        }
        "getMatrix" => {
            let m = cvf(vm, recv).top().m;
            match obj(a(0)) {
                Some(o) => {
                    set_matrix(vm, o, m);
                    Value::Null
                }
                None => {
                    let r = vm
                        .heap
                        .alloc_instance("Landroid/graphics/Matrix;".into(), Vec::new())?;
                    set_matrix(vm, r, m);
                    Value::Obj(r)
                }
            }
        }
        "clipRect" | "clipPath" | "clipOutRect" | "clipOutPath" | "clipRRect" => {
            let r = match (name, obj(a(0))) {
                ("clipPath" | "clipOutPath", p) => path_bounds(&path_of(vm, p)),
                (_, Some(o)) => rect_of(vm, Some(o)),
                _ => [f(0), f(1), f(2), f(3)],
            };
            let difference = name.starts_with("clipOut")
                || (sig.contains("Region$Op") && ord(vm, args.last()) == 0);
            if !difference {
                let t = cvf(vm, recv).top();
                let c = [ap(&t.m, r[0], r[1]), ap(&t.m, r[2], r[3])];
                let dr = [
                    c[0].0.min(c[1].0),
                    c[0].1.min(c[1].1),
                    c[0].0.max(c[1].0),
                    c[0].1.max(c[1].1),
                ];
                t.clip = [
                    t.clip[0].max(dr[0]),
                    t.clip[1].max(dr[1]),
                    t.clip[2].min(dr[2]),
                    t.clip[3].min(dr[3]),
                ];
                let k = t.clip;
                b(k[0] < k[2] && k[1] < k[3])
            } else {
                b(true)
            }
        }
        "getClipBounds" => {
            let (t, w, h) = {
                let c = cvf(vm, recv);
                (*c.top(), c.w, c.h)
            };
            let i = inv(&t.m).unwrap_or(ID);
            let k = [
                t.clip[0].max(-1e6),
                t.clip[1].max(-1e6),
                t.clip[2].min(1e6),
                t.clip[3].min(1e6),
            ];
            let (p0, p1) = (ap(&i, k[0], k[1]), ap(&i, k[2], k[3]));
            let mut r = [
                p0.0.min(p1.0),
                p0.1.min(p1.1),
                p0.0.max(p1.0),
                p0.1.max(p1.1),
            ];
            if r[2] - r[0] > 1e5 {
                r = [0.0, 0.0, w as f32, h as f32];
            }
            if let Some(o) = obj(a(0)) {
                set_rect(vm, o, r, true)?;
                b(r[0] < r[2] && r[1] < r[3])
            } else {
                let o = vm
                    .heap
                    .alloc_instance("Landroid/graphics/Rect;".into(), Vec::new())?;
                set_rect(vm, o, r, true)?;
                Value::Obj(o)
            }
        }
        "quickReject" => b(false),
        "isHardwareAccelerated" => b(matches!(cvf(vm, recv).target, Target::Node(_))),
        "getWidth" => Value::Int(cvf(vm, recv).w),
        "getHeight" => Value::Int(cvf(vm, recv).h),
        "getDensity" => Value::Int(280),
        "isOpaque" => b(false),
        "getMaximumBitmapWidth" | "getMaximumBitmapHeight" => Value::Int(16384),
        "setBitmap" => {
            let (w, h) = match obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)) {
                Some(G::Bitmap(w, h, _)) => (*w, *h),
                _ => (0, 0),
            };
            let t = obj(a(0)).map(Target::Bitmap).unwrap_or(Target::None);
            vm.gfx.objs.insert(recv, G::Canvas(CanvasS::new(t, w, h)));
            Value::Null
        }
        "drawRect" => {
            let r = if let Some(o) = obj(a(0)) {
                rect_of(vm, Some(o))
            } else {
                [f(0), f(1), f(2), f(3)]
            };
            let p = paint_last(vm);
            record(vm, recv, p, OpK::Rect(r[0], r[1], r[2], r[3]));
            Value::Null
        }
        "drawRoundRect" => {
            let (r, rx, ry) = if let Some(o) = obj(a(0)) {
                (rect_of(vm, Some(o)), f(1), f(2))
            } else {
                ([f(0), f(1), f(2), f(3)], f(4), f(5))
            };
            let p = paint_last(vm);
            record(vm, recv, p, OpK::RRect(r[0], r[1], r[2], r[3], rx, ry));
            Value::Null
        }
        "drawOval" | "drawCircle" => {
            let r = match (name, obj(a(0))) {
                ("drawCircle", _) => [f(0) - f(2), f(1) - f(2), f(0) + f(2), f(1) + f(2)],
                (_, Some(o)) => rect_of(vm, Some(o)),
                _ => [f(0), f(1), f(2), f(3)],
            };
            let p = paint_last(vm);
            record(vm, recv, p, OpK::Oval(r[0], r[1], r[2], r[3]));
            Value::Null
        }
        "drawArc" => {
            let (r, s0, sw, center) = if let Some(o) = obj(a(0)) {
                (rect_of(vm, Some(o)), f(1), f(2), it(a(3)) != 0)
            } else {
                ([f(0), f(1), f(2), f(3)], f(4), f(5), it(a(6)) != 0)
            };
            let (cx, cy, rx, ry) = (
                (r[0] + r[2]) / 2.0,
                (r[1] + r[3]) / 2.0,
                (r[2] - r[0]) / 2.0,
                (r[3] - r[1]) / 2.0,
            );
            let mut pts: Vec<(f32, f32)> = (0..=32)
                .map(|i| {
                    let t = (s0 + sw * i as f32 / 32.0).to_radians();
                    (cx + rx * t.cos(), cy + ry * t.sin())
                })
                .collect();
            if center {
                pts.push((cx, cy));
                pts.push(pts[0]);
            }
            let p = paint_last(vm);
            record(vm, recv, p, OpK::Path(vec![pts], false));
            Value::Null
        }
        "drawLine" => {
            let p = paint_last(vm);
            record(vm, recv, p, OpK::Line(f(0), f(1), f(2), f(3)));
            Value::Null
        }
        "drawLines" | "drawPoints" | "drawPoint" => {
            let p = paint_last(vm);
            if name == "drawPoint" {
                let h = p.sw.max(1.0) / 2.0;
                record(
                    vm,
                    recv,
                    PaintSnap { style: 0, ..p },
                    OpK::Rect(f(0) - h, f(1) - h, f(0) + h, f(1) + h),
                );
            } else {
                let v = float_array(vm, a(0));
                let (off, cnt) = if args.len() >= 4 {
                    (it(a(1)) as usize, it(a(2)) as usize)
                } else {
                    (0, v.len())
                };
                let v: Vec<f32> = v.into_iter().skip(off).take(cnt).collect();
                if name == "drawLines" {
                    for c in v.chunks(4).filter(|c| c.len() == 4) {
                        record(vm, recv, p, OpK::Line(c[0], c[1], c[2], c[3]));
                    }
                } else {
                    let h = p.sw.max(1.0) / 2.0;
                    for c in v.chunks(2).filter(|c| c.len() == 2) {
                        record(
                            vm,
                            recv,
                            PaintSnap { style: 0, ..p },
                            OpK::Rect(c[0] - h, c[1] - h, c[0] + h, c[1] + h),
                        );
                    }
                }
            }
            Value::Null
        }
        "drawPath" => {
            let (pts, eo) = match obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)) {
                Some(G::Path(p, e)) => (p.clone(), *e),
                _ => (Vec::new(), false),
            };
            let p = paint_last(vm);
            record(vm, recv, p, OpK::Path(pts, eo));
            Value::Null
        }
        "drawColor" | "drawARGB" | "drawRGB" | "drawPaint" => {
            let c = match name {
                "drawARGB" => {
                    ((it(a(0)) as u32 & 0xFF) << 24)
                        | ((it(a(1)) as u32 & 0xFF) << 16)
                        | ((it(a(2)) as u32 & 0xFF) << 8)
                        | (it(a(3)) as u32 & 0xFF)
                }
                "drawRGB" => {
                    0xFF00_0000
                        | ((it(a(0)) as u32 & 0xFF) << 16)
                        | ((it(a(1)) as u32 & 0xFF) << 8)
                        | (it(a(2)) as u32 & 0xFF)
                }
                "drawPaint" => paint_of(vm, a(0)).argb,
                _ => match a(0) {
                    Some(Value::Long(l)) => ((*l as u64) >> 32) as u32,
                    v => it(v) as u32,
                },
            };
            // CLEAR mode (PorterDuff/BlendMode ordinal 0) clears to transparent
            let clear = args.len() >= 2 && ord(vm, a(1)) == 0;
            record(
                vm,
                recv,
                PaintSnap {
                    argb: if clear { 0 } else { c },
                    style: 0,
                    sw: 0.0,
                    clear: false,
                    tint: None,
                    ls: 0.0,
                },
                OpK::Color(if clear { 0 } else { c }),
            );
            Value::Null
        }
        "drawText" | "drawTextRun" => {
            // (String,FFP) (String,IIFFP) (CharSequence,IIFFP) ([C,IIFFP) ; drawTextRun(cs, start, end, ctxStart, ctxEnd, x, y, isRtl, paint)
            let Some(Value::Obj(first_ref)) = a(0) else {
                return Err(exc("Ljava/lang/NullPointerException;", "text == null"));
            };
            let ints: Vec<i32> = args[1..]
                .iter()
                .take_while(|v| matches!(v, Value::Int(_)))
                .map(|v| it(Some(v)))
                .collect();
            let floats: Vec<f32> = args
                .iter()
                .filter_map(|v| {
                    if let Value::Float(x) = v {
                        Some(*x)
                    } else {
                        None
                    }
                })
                .collect();
            let is_ca = sig.starts_with("([C");
            let t = if ints.len() >= 2 {
                checked_text_slice(
                    vm,
                    Some(&Value::Obj(*first_ref)),
                    is_ca,
                    ints[0],
                    ints[1],
                    is_ca && name == "drawText",
                )?
            } else {
                cs_text(vm, &Value::Obj(*first_ref))?
            };
            let p = paint_of(vm, args.last());
            let (x, y) = (
                floats.first().copied().unwrap_or(0.0),
                floats.get(1).copied().unwrap_or(0.0),
            );
            let align = p
                .misc
                .get("TextAlign")
                .map(|v| ord(vm, Some(v)))
                .unwrap_or(0);
            let w = measure(&t, p.text_size, p.letter_spacing);
            let x = match align {
                1 => x - w / 2.0,
                2 => x - w,
                _ => x,
            };
            let ps = snap(vm, &p);
            record(vm, recv, ps, OpK::Text(t, x, y, p.text_size));
            Value::Null
        }
        "drawBitmap" => {
            let bm = obj(a(0));
            let (bw, bh) = match bm.and_then(|o| vm.gfx.objs.get(&o)) {
                Some(G::Bitmap(w, h, _)) => (*w as f32, *h as f32),
                _ => return Ok(Some(Value::Null)),
            };
            let dst = if sig.contains("Landroid/graphics/Matrix;") {
                let m = matrix_of(vm, obj(a(1)));
                let (p0, p1) = (ap(&m, 0.0, 0.0), ap(&m, bw, bh));
                [p0.0, p0.1, p1.0, p1.1]
            } else if sig.contains("Rect") {
                rect_of(vm, obj(a(2)))
            } else {
                [f(1), f(2), f(1) + bw, f(2) + bh]
            };
            let p = snap(vm, &paint_of(vm, args.last()));
            record(vm, recv, p, OpK::Bitmap(bm.expect("checked"), dst));
            Value::Null
        }
        "drawRenderNode" => {
            if let Some(n) = obj(a(0)) {
                record(
                    vm,
                    recv,
                    PaintSnap {
                        argb: 0,
                        style: 0,
                        sw: 0.0,
                        clear: false,
                        tint: None,
                        ls: 0.0,
                    },
                    OpK::Node(n),
                );
            }
            Value::Null
        }
        "setDensity"
        | "setDrawFilter"
        | "enableZ"
        | "disableZ"
        | "insertReorderBarrier"
        | "insertInorderBarrier"
        | "drawVertices"
        | "drawPicture"
        | "drawTextOnPath"
        | "drawDoubleRoundRect"
        | "drawPatch"
        | "drawMesh"
        | "drawGlyphs"
        | "drawBitmapMesh"
        | "drawTextRunMeasured" => Value::Null,
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn node_method(
    vm: &mut Engine,
    mut n: NodeS,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let key: Option<&'static str> = match name.trim_start_matches("set").trim_start_matches("get") {
        "TranslationX" => Some("translationX"),
        "TranslationY" => Some("translationY"),
        "TranslationZ" => Some("translationZ"),
        "ScaleX" => Some("scaleX"),
        "ScaleY" => Some("scaleY"),
        "RotationX" => Some("rotationX"),
        "RotationY" => Some("rotationY"),
        "RotationZ" => Some("rotationZ"),
        "PivotX" => Some("pivotX"),
        "PivotY" => Some("pivotY"),
        "Alpha" => Some("alpha"),
        "Elevation" => Some("elevation"),
        "CameraDistance" => Some("cameraDistance"),
        _ => None,
    };
    let defaults = |k: &str| match k {
        "scaleX" | "scaleY" | "alpha" => 1.0,
        _ => 0.0,
    };
    let out = match (name, key) {
        (n2, Some(k)) if n2.starts_with("set") => {
            n.f.insert(k, fl(a(0)));
            b(true)
        }
        (n2, Some(k)) if n2.starts_with("get") => {
            Value::Float(n.f.get(k).copied().unwrap_or(defaults(k)))
        }
        ("beginRecording", _) => {
            let (w, h) = if args.len() == 2 {
                (it(a(0)), it(a(1)))
            } else {
                (n.r.saturating_sub(n.l), n.b.saturating_sub(n.t))
            };
            n.ops.clear();
            let c = vm
                .heap
                .alloc_instance("Landroid/graphics/RecordingCanvas;".into(), Vec::new())?;
            vm.gfx
                .objs
                .insert(c, G::Canvas(CanvasS::new(Target::Node(recv), w, h)));
            n.f.insert("rd_canvas", f32::from_bits(c));
            vm.gfx.objs.insert(recv, G::Node(n));
            return Ok(Some(Value::Obj(c)));
        }
        ("endRecording", _) => {
            // ops were appended while recording; keep them (fetch latest)
            if let Some(G::Node(cur)) = vm.gfx.objs.get(&recv) {
                n.ops = cur.ops.clone();
            }
            n.has_dl = true;
            Value::Null
        }
        ("discardDisplayList", _) => {
            n.ops.clear();
            n.has_dl = false;
            Value::Null
        }
        ("hasDisplayList", _) => b(n.has_dl),
        ("setPosition", _) => {
            let r = if let Some(o) = obj(a(0)) {
                rect_of(vm, Some(o))
            } else {
                [fl(a(0)), fl(a(1)), fl(a(2)), fl(a(3))]
            };
            n.l = r[0] as i32;
            n.t = r[1] as i32;
            n.r = r[2] as i32;
            n.b = r[3] as i32;
            b(true)
        }
        ("offsetLeftAndRight", _) => {
            let d = it(a(0));
            n.l = n.l.saturating_add(d);
            n.r = n.r.saturating_add(d);
            b(true)
        }
        ("offsetTopAndBottom", _) => {
            let d = it(a(0));
            n.t = n.t.saturating_add(d);
            n.b = n.b.saturating_add(d);
            b(true)
        }
        ("getLeft", _) => Value::Int(n.l),
        ("getTop", _) => Value::Int(n.t),
        ("getRight", _) => Value::Int(n.r),
        ("getBottom", _) => Value::Int(n.b),
        ("getWidth", _) => Value::Int(n.r.saturating_sub(n.l)),
        ("getHeight", _) => Value::Int(n.b.saturating_sub(n.t)),
        ("setClipToBounds", _) => {
            n.clip_bounds = it(a(0)) != 0;
            b(true)
        }
        ("getClipToBounds", _) => b(n.clip_bounds),
        ("setClipToOutline", _) => {
            n.clip_outline = it(a(0)) != 0;
            b(true)
        }
        ("getClipToOutline", _) => b(n.clip_outline),
        ("setOutline", _) => {
            n.outline = match obj(a(0)).and_then(|o| vm.gfx.objs.get(&o)) {
                Some(G::Outline(r, _, _)) => *r,
                _ => None,
            };
            b(true)
        }
        ("resetPivot", _) => {
            n.f.remove("pivotX");
            n.f.remove("pivotY");
            b(true)
        }
        ("isPivotExplicitlySet", _) => b(n.f.contains_key("pivotX")),
        ("getMatrix" | "getInverseMatrix", _) => {
            let mut m = n.matrix();
            m[4] -= n.l as f32;
            m[5] -= n.t as f32;
            if name == "getInverseMatrix" {
                m = inv(&m).unwrap_or(ID);
            }
            if let Some(o) = obj(a(0)) {
                set_matrix(vm, o, m);
            }
            Value::Null
        }
        ("getUniqueId", _) => Value::Long(recv as i64),
        ("hasIdentityMatrix", _) => b(n.matrix() == tr(n.l as f32, n.t as f32)),
        ("hasOverlappingRendering", _) => b(true),
        ("getUseCompositingLayer", _) => b(false),
        ("getAmbientShadowColor" | "getSpotShadowColor", _) => Value::Int(0xFF00_0000u32 as i32),
        ("computeApproximateMemoryUsage" | "computeApproximateMemoryAllocated", _) => {
            Value::Long(0)
        }
        (n2, _) if n2.starts_with("set") => b(true), // overlapping, compositing layer, shadow colors, render effect, …
        _ => return Ok(None),
    };
    vm.gfx.objs.insert(recv, G::Node(n));
    Ok(Some(out))
}

fn layout_method(
    vm: &mut Engine,
    l: &LayoutS,
    name: &str,
    _sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let a = |i: usize| args.get(i);
    let line = |i: i32| (i.max(0) as usize).min(l.lines.len() - 1);
    let n = l.lines.len();
    Ok(Some(match name {
        "getLineCount" => Value::Int(n as i32),
        "getHeight" => Value::Int((n as f32 * l.lh).ceil() as i32),
        "getWidth" | "getEllipsizedWidth" => Value::Int(l.width),
        "getText" => l.cs.clone(),
        "getPaint" => l.paint.clone(),
        "getLineTop" => Value::Int(if it(a(0)) as usize >= n {
            (n as f32 * l.lh) as i32
        } else {
            l.top(line(it(a(0)))) as i32
        }),
        "getLineBottom" | "getLineBottomWithoutSpacing" => {
            Value::Int(l.top(line(it(a(0)))) as i32 + l.lh as i32)
        }
        "getLineBaseline" => Value::Int(l.baseline(line(it(a(0)))).round() as i32),
        "getLineAscent" => Value::Int(l.asc.floor() as i32),
        "getLineDescent" => Value::Int(l.desc.ceil() as i32),
        "getLineStart" => Value::Int(if it(a(0)) as usize >= n {
            l.text.chars().count() as i32
        } else {
            l.lines[line(it(a(0)))].0 as i32
        }),
        "getLineEnd" | "getLineVisibleEnd" => {
            let (s, e, _) = l.lines[line(it(a(0)))];
            let mut e = e;
            if name == "getLineVisibleEnd" {
                let cs: Vec<char> = l.text.chars().collect();
                while e > s && matches!(cs.get(e - 1), Some(' ' | '\n')) {
                    e -= 1;
                }
            }
            Value::Int(e as i32)
        }
        "getLineLeft" => Value::Float(l.line_left(line(it(a(0))))),
        "getLineRight" => {
            let i = line(it(a(0)));
            Value::Float(l.line_left(i) + l.lines[i].2)
        }
        "getLineWidth" | "getLineMax" => Value::Float(l.lines[line(it(a(0)))].2),
        "getLineForOffset" => Value::Int(l.line_for_offset(it(a(0))) as i32),
        "getLineForVertical" => {
            Value::Int(((it(a(0)).max(0) as f32 / l.lh.max(1.0)) as usize).min(n - 1) as i32)
        }
        "getOffsetForHorizontal" => {
            let i = line(it(a(0)));
            let x = fl(a(1)) - l.line_left(i);
            let (s, e, _) = l.lines[i];
            let mut acc = 0.0;
            let mut off = s;
            for c in l.text.chars().skip(s).take(e - s) {
                let ad = char_adv(c, l.size, l.ls);
                if acc + ad / 2.0 > x || c == '\n' {
                    break;
                }
                acc += ad;
                off += 1;
            }
            Value::Int(off as i32)
        }
        "getPrimaryHorizontal" | "getSecondaryHorizontal" => Value::Float(l.horiz(it(a(0)))),
        "getParagraphDirection" => Value::Int(1),
        "isRtlCharAt" => b(false),
        "getParagraphAlignment" => {
            enum_const(vm, "Landroid/text/Layout$Alignment;", l.align as usize)?
                .unwrap_or(Value::Null)
        }
        "getAlignment" => enum_const(vm, "Landroid/text/Layout$Alignment;", l.align as usize)?
            .unwrap_or(Value::Null),
        "getEllipsisStart" | "getEllipsisCount" => Value::Int(0),
        "getTopPadding" | "getBottomPadding" => Value::Int(0),
        "getSpacingMultiplier" => Value::Float(1.0),
        "getSpacingAdd" => Value::Float(0.0),
        "isFallbackLineSpacingEnabled" | "getLineContainsTab" => b(false),
        "getLineDirections" => Value::Null,
        "getSelectionPath" => {
            if let Some(p) = obj(a(2)) {
                let (s, e) = (it(a(0)).min(it(a(1))), it(a(0)).max(it(a(1))));
                let (ls_, le) = (l.line_for_offset(s), l.line_for_offset(e));
                let mut polys = Vec::new();
                for i in ls_..=le {
                    let x0 = if i == ls_ { l.horiz(s) } else { l.line_left(i) };
                    let x1 = if i == le {
                        l.horiz(e)
                    } else {
                        l.line_left(i) + l.lines[i].2
                    };
                    let (t, bt) = (l.top(i), l.top(i) + l.lh);
                    polys.push(vec![(x0, t), (x1, t), (x1, bt), (x0, bt), (x0, t)]);
                }
                vm.gfx.objs.insert(p, G::Path(polys, false));
            }
            Value::Null
        }
        "draw" => {
            if let Some(c) = obj(a(0)) {
                let chars: Vec<char> = l.text.chars().collect();
                for i in 0..n {
                    let (s, e, _) = l.lines[i];
                    let mut t: String = chars[s.min(chars.len())..e.min(chars.len())]
                        .iter()
                        .filter(|c| **c != '\n')
                        .collect();
                    if l.ell.iter().any(|(es, _)| *es == s) {
                        let maxw = l.width as f32 - char_adv('…', l.size, l.ls);
                        while measure(&t, l.size, l.ls) > maxw && !t.is_empty() {
                            t.pop();
                        }
                        t.push('…');
                    }
                    // AOSP Layout.draw draws through the canvas' virtual
                    // drawText: a guest Canvas subclass (e.g. Compose's
                    // TextAndroidCanvas) forwards to its wrapped canvas.
                    let guest_canvas = vm
                        .heap
                        .class_of(c)
                        .map(|k| k != "Landroid/graphics/Canvas;")
                        .unwrap_or(false)
                        && vm
                            .cp
                            .resolve_method(
                                vm.heap.class_of(c).unwrap_or(""),
                                "drawText",
                                "(Ljava/lang/String;FFLandroid/graphics/Paint;)V",
                            )
                            .is_some();
                    if guest_canvas {
                        let js = Value::Obj(crate::intrinsics::alloc_string(vm, t)?);
                        crate::jcollections::guest_call(
                            vm,
                            c,
                            "drawText",
                            "(Ljava/lang/String;FFLandroid/graphics/Paint;)V",
                            &[
                                js,
                                Value::Float(l.line_left(i)),
                                Value::Float(l.baseline(i)),
                                l.paint.clone(),
                            ],
                        )?;
                        continue;
                    }
                    let p = paint_of(vm, Some(&l.paint));
                    record(
                        vm,
                        c,
                        PaintSnap {
                            argb: p.argb,
                            style: 0,
                            sw: 0.0,
                            clear: false,
                            tint: None,
                            ls: l.ls,
                        },
                        OpK::Text(t, l.line_left(i), l.baseline(i), l.size),
                    );
                }
            }
            Value::Null
        }
        "getRangeForRect" => Value::Null,
        _ => {
            let _ = recv;
            return Ok(None);
        }
    }))
}

// ── rasterizer ─────────────────────────────────────────────────────────

/// Paint one snapshot text line into a validated packed RGB framebuffer.
/// Shares the real embedded glyph atlas and clipping rasterizer with Canvas;
/// no checkerboard placeholder characters, host fonts or external resources.
/// Returns false for inconsistent/overflowing dimensions and leaves pixels intact.
pub fn rasterize_ui_text_rgb(
    px: &mut [u8],
    dimensions: (usize, usize),
    origin: (i32, i32),
    text: &str,
    rgb: (u8, u8, u8),
) -> bool {
    let (w, h) = dimensions;
    if w == 0
        || h == 0
        || w > i32::MAX as usize
        || h > i32::MAX as usize
        || w.checked_mul(h).and_then(|n| n.checked_mul(3)) != Some(px.len())
    {
        return false;
    }
    let argb = 0xff00_0000 | (u32::from(rgb.0) << 16) | (u32::from(rgb.1) << 8) | u32::from(rgb.2);
    let mut raster = Raster {
        px,
        w,
        h,
        tint: None,
    };
    raster.text(
        text,
        &ID,
        (origin.0 as f32, origin.1.saturating_add(17) as f32),
        (18.0, 0.0),
        argb,
        [0.0, 0.0, w as f32, h as f32],
    );
    true
}

pub(crate) struct Raster<'a> {
    pub px: &'a mut [u8],
    pub w: usize,
    pub h: usize,
    /// Active colour-filter tint while replaying a bitmap's ops.
    pub tint: Option<u32>,
}

impl Raster<'_> {
    fn blend(&mut self, x: i32, y: i32, argb: u32, cov: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let a = ((argb >> 24) as f32 / 255.0) * cov;
        if a <= 0.0 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 3;
        let c = [(argb >> 16) as u8, (argb >> 8) as u8, argb as u8];
        for (k, ck) in c.iter().enumerate() {
            let d = self.px[i + k] as f32;
            self.px[i + k] = (d + (*ck as f32 - d) * a.min(1.0)).round() as u8;
        }
    }
    fn fill_polys(&mut self, polys: &[Vec<(f32, f32)>], argb: u32, clip: [f32; 4], even_odd: bool) {
        let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for p in polys {
            for (x, y) in p {
                bb = [bb[0].min(*x), bb[1].min(*y), bb[2].max(*x), bb[3].max(*y)];
            }
        }
        let x0 = bb[0].max(clip[0]).max(0.0).floor() as i32;
        let x1 = bb[2].min(clip[2]).min(self.w as f32).ceil() as i32;
        let y0 = bb[1].max(clip[1]).max(0.0).floor() as i32;
        let y1 = bb[3].min(clip[3]).min(self.h as f32).ceil() as i32;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        // axis-aligned rect fast path
        if polys.len() == 1 && polys[0].len() <= 5 && is_axis_rect(&polys[0]) {
            let (rx0, ry0, rx1, ry1) = (
                bb[0].max(clip[0]),
                bb[1].max(clip[1]),
                bb[2].min(clip[2]),
                bb[3].min(clip[3]),
            );
            for y in y0..y1 {
                let cy = (ry1.min(y as f32 + 1.0) - ry0.max(y as f32)).clamp(0.0, 1.0);
                for x in x0..x1 {
                    let cx = (rx1.min(x as f32 + 1.0) - rx0.max(x as f32)).clamp(0.0, 1.0);
                    self.blend(x, y, argb, cx * cy);
                }
            }
            return;
        }
        const SS: usize = 4;
        let w = (x1 - x0) as usize;
        let mut cov = vec![0f32; w];
        let mut xs: Vec<(f32, i32)> = Vec::new();
        for y in y0..y1 {
            cov.iter_mut().for_each(|c| *c = 0.0);
            for s in 0..SS {
                let sy = y as f32 + (s as f32 + 0.5) / SS as f32;
                if sy < clip[1] || sy >= clip[3] {
                    continue;
                }
                xs.clear();
                for p in polys {
                    let n = p.len();
                    if n < 2 {
                        continue;
                    }
                    for i in 0..n {
                        let (a, bq) = (p[i], p[(i + 1) % n]);
                        if (a.1 <= sy) != (bq.1 <= sy) {
                            let t = (sy - a.1) / (bq.1 - a.1);
                            xs.push((a.0 + t * (bq.0 - a.0), if bq.1 > a.1 { 1 } else { -1 }));
                        }
                    }
                }
                xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                let mut wind = 0;
                for i in 0..xs.len().saturating_sub(1) {
                    wind += if even_odd { 1 } else { xs[i].1 };
                    let inside = if even_odd { wind % 2 == 1 } else { wind != 0 };
                    if !inside {
                        continue;
                    }
                    let (sa, sb) = (
                        xs[i].0.max(clip[0]).max(x0 as f32),
                        xs[i + 1].0.min(clip[2]).min(x1 as f32),
                    );
                    if sa >= sb {
                        continue;
                    }
                    let (ia, ib) = (sa.floor() as i32, sb.ceil() as i32);
                    for x in ia..ib {
                        let c = (sb.min(x as f32 + 1.0) - sa.max(x as f32)).clamp(0.0, 1.0);
                        if let Some(v) = cov.get_mut((x - x0) as usize) {
                            *v += c / SS as f32;
                        }
                    }
                }
            }
            for (i, c) in cov.iter().enumerate() {
                if *c > 0.0 {
                    self.blend(x0 + i as i32, y, argb, c.min(1.0));
                }
            }
        }
    }
    fn stroke_polyline(&mut self, pts: &[(f32, f32)], width: f32, argb: u32, clip: [f32; 4]) {
        let hw = width.max(1.0) / 2.0;
        let mut quads = Vec::new();
        for s in pts.windows(2) {
            let (a, bq) = (s[0], s[1]);
            let (dx, dy) = (bq.0 - a.0, bq.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-4 {
                continue;
            }
            let (nx, ny) = (-dy / len * hw, dx / len * hw);
            quads.push(vec![
                (a.0 + nx, a.1 + ny),
                (bq.0 + nx, bq.1 + ny),
                (bq.0 - nx, bq.1 - ny),
                (a.0 - nx, a.1 - ny),
            ]);
        }
        for q in quads {
            self.fill_polys(&[q], argb, clip, false);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        s: &str,
        m: &M,
        (x, y): (f32, f32),
        (size, ls): (f32, f32),
        argb: u32,
        clip: [f32; 4],
    ) {
        let at = atlas();
        // effective scale from the matrix (uniform approximation)
        let ms = (m[0] * m[0] + m[1] * m[1]).sqrt().max(1e-3);
        let k = size * ms / at.size;
        if k <= 0.0 {
            return;
        }
        let (ox, oy) = ap(m, x, y);
        let mut pen = ox;
        for c in s.chars() {
            if c == '\n' {
                continue;
            }
            let g = glyph(c);
            let gx0 = pen + g.left as f32 * k;
            let gy0 = oy - g.top as f32 * k;
            let (gw, gh) = (g.w as f32 * k, g.h as f32 * k);
            let (px0, py0) = (
                gx0.max(clip[0]).floor() as i32,
                gy0.max(clip[1]).floor() as i32,
            );
            let (px1, py1) = (
                (gx0 + gw).min(clip[2]).ceil() as i32,
                (gy0 + gh).min(clip[3]).ceil() as i32,
            );
            for py in py0..py1 {
                for pxx in px0..px1 {
                    // bilinear sample at pixel centre
                    let u = (pxx as f32 + 0.5 - gx0) / k - 0.5;
                    let v = (py as f32 + 0.5 - gy0) / k - 0.5;
                    let samp = |xx: i32, yy: i32| -> f32 {
                        if xx < 0 || yy < 0 || xx >= g.w as i32 || yy >= g.h as i32 {
                            0.0
                        } else {
                            at.data[g.off + yy as usize * g.w as usize + xx as usize] as f32 / 255.0
                        }
                    };
                    let (ux, vy) = (u.floor(), v.floor());
                    let (fx, fy) = (u - ux, v - vy);
                    let (ix, iy) = (ux as i32, vy as i32);
                    let a = samp(ix, iy) * (1.0 - fx) * (1.0 - fy)
                        + samp(ix + 1, iy) * fx * (1.0 - fy)
                        + samp(ix, iy + 1) * (1.0 - fx) * fy
                        + samp(ix + 1, iy + 1) * fx * fy;
                    if a > 0.01 {
                        self.blend(pxx, py, argb, a);
                    }
                }
            }
            pen += char_adv(c, size, ls) * ms;
        }
    }
}

fn is_axis_rect(p: &[(f32, f32)]) -> bool {
    let n = if p.len() == 5 && p[0] == p[4] {
        4
    } else {
        p.len()
    };
    n == 4
        && (0..4).all(|i| {
            let (a, b) = (p[i], p[(i + 1) % 4]);
            a.0 == b.0 || a.1 == b.1
        })
}

fn xform(m: &M, poly: &[(f32, f32)]) -> Vec<(f32, f32)> {
    poly.iter().map(|(x, y)| ap(m, *x, *y)).collect()
}
fn mod_alpha(argb: u32, a: f32) -> u32 {
    let na = ((argb >> 24) as f32 * a.clamp(0.0, 1.0)).round() as u32;
    (argb & 0x00FF_FFFF) | (na << 24)
}

impl Engine {
    /// Rasterizes a display list onto an RGB framebuffer.
    pub(crate) fn raster_ops(
        &self,
        r: &mut Raster<'_>,
        ops: &[Op],
        parent: &M,
        pclip: [f32; 4],
        palpha: f32,
        depth: usize,
    ) {
        let mut budget = MAX_RASTER_OPS_VISITED;
        self.raster_ops_budgeted(r, ops, parent, pclip, palpha, depth, &mut budget);
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_ops_budgeted(
        &self,
        r: &mut Raster<'_>,
        ops: &[Op],
        parent: &M,
        pclip: [f32; 4],
        palpha: f32,
        depth: usize,
        budget: &mut usize,
    ) {
        if depth > 64 || *budget == 0 {
            return;
        }
        for op in ops {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let m = mul(parent, &op.m);
            // op.clip is in the recording surface's device space → map by parent
            let cc = [
                ap(parent, op.clip[0].max(-1e6), op.clip[1].max(-1e6)),
                ap(parent, op.clip[2].min(1e6), op.clip[3].min(1e6)),
            ];
            let clip = [
                pclip[0].max(cc[0].0.min(cc[1].0)),
                pclip[1].max(cc[0].1.min(cc[1].1)),
                pclip[2].min(cc[0].0.max(cc[1].0)),
                pclip[3].min(cc[0].1.max(cc[1].1)),
            ];
            if clip[0] >= clip[2] || clip[1] >= clip[3] {
                continue;
            }
            // Bitmaps are replayed straight onto the target, so a CLEAR
            // (e.g. Compose's vector-cache reset) is a no-op here.
            if op.p.clear {
                continue;
            }
            let alpha = palpha * op.alpha;
            let base = match r.tint {
                Some(t) => (t & 0x00ff_ffff) | ((((t >> 24) * (op.p.argb >> 24)) / 255) << 24),
                None => op.p.argb,
            };
            let col = mod_alpha(base, alpha);
            let shape = |r: &mut Raster<'_>, poly: Vec<(f32, f32)>| {
                if op.p.style != 1 {
                    r.fill_polys(&[xform(&m, &poly)], col, clip, false);
                }
                if op.p.style >= 1 {
                    let mut pp = poly.clone();
                    if let Some(f0) = pp.first().copied() {
                        pp.push(f0);
                    }
                    let ms = (m[0] * m[0] + m[1] * m[1]).sqrt();
                    r.stroke_polyline(&xform(&m, &pp), op.p.sw * ms, col, clip);
                }
            };
            match &op.k {
                OpK::Rect(l, t, rr, bt) => {
                    shape(r, vec![(*l, *t), (*rr, *t), (*rr, *bt), (*l, *bt)])
                }
                OpK::RRect(l, t, rr, bt, rx, ry) => {
                    let (rx, ry) = (
                        rx.min((rr - l) / 2.0).max(0.0),
                        ry.min((bt - t) / 2.0).max(0.0),
                    );
                    shape(
                        r,
                        rrect_poly(*l, *t, *rr, *bt, [rx, ry, rx, ry, rx, ry, rx, ry]),
                    )
                }
                OpK::Oval(l, t, rr, bt) => shape(r, oval_poly(*l, *t, *rr, *bt)),
                OpK::Line(x0, y0, x1, y1) => {
                    let ms = (m[0] * m[0] + m[1] * m[1]).sqrt();
                    r.stroke_polyline(
                        &xform(&m, &[(*x0, *y0), (*x1, *y1)]),
                        op.p.sw.max(1.0) * ms,
                        col,
                        clip,
                    )
                }
                OpK::Path(polys, eo) => {
                    let tp: Vec<Vec<(f32, f32)>> = polys.iter().map(|p| xform(&m, p)).collect();
                    if op.p.style != 1 {
                        r.fill_polys(&tp, col, clip, *eo);
                    }
                    if op.p.style >= 1 {
                        let ms = (m[0] * m[0] + m[1] * m[1]).sqrt();
                        for p in &tp {
                            r.stroke_polyline(p, op.p.sw * ms, col, clip);
                        }
                    }
                }
                OpK::Text(s, x, y, size) => r.text(s, &m, (*x, *y), (*size, op.p.ls), col, clip),
                OpK::Color(c) => {
                    let full = [
                        clip[0].max(0.0),
                        clip[1].max(0.0),
                        clip[2].min(r.w as f32),
                        clip[3].min(r.h as f32),
                    ];
                    let c = mod_alpha(*c, alpha);
                    r.fill_polys(
                        &[vec![
                            (full[0], full[1]),
                            (full[2], full[1]),
                            (full[2], full[3]),
                            (full[0], full[3]),
                        ]],
                        c,
                        clip,
                        false,
                    );
                }
                OpK::Node(n) => {
                    if let Some(G::Node(ns)) = self.gfx.objs.get(n) {
                        let nm = mul(&m, &ns.matrix());
                        let mut nclip = clip;
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
                        if let Some(bb) = bounds {
                            let (p0, p1) = (ap(&nm, bb[0], bb[1]), ap(&nm, bb[2], bb[3]));
                            nclip = [
                                nclip[0].max(p0.0.min(p1.0)),
                                nclip[1].max(p0.1.min(p1.1)),
                                nclip[2].min(p0.0.max(p1.0)),
                                nclip[3].min(p0.1.max(p1.1)),
                            ];
                        }
                        self.raster_ops_budgeted(
                            r,
                            &ns.ops,
                            &nm,
                            nclip,
                            alpha * ns.g("alpha", 1.0),
                            depth + 1,
                            budget,
                        );
                    }
                }
                OpK::Bitmap(bm, dst) => {
                    if let Some(G::Bitmap(w, h, bops)) = self.gfx.objs.get(bm) {
                        let s = mul(
                            &m,
                            &mul(
                                &tr(dst[0], dst[1]),
                                &sc(
                                    (dst[2] - dst[0]) / (*w).max(1) as f32,
                                    (dst[3] - dst[1]) / (*h).max(1) as f32,
                                ),
                            ),
                        );
                        let (p0, p1) = (ap(&s, 0.0, 0.0), ap(&s, *w as f32, *h as f32));
                        let bclip = [
                            clip[0].max(p0.0.min(p1.0)),
                            clip[1].max(p0.1.min(p1.1)),
                            clip[2].min(p0.0.max(p1.0)),
                            clip[3].min(p0.1.max(p1.1)),
                        ];
                        let saved = r.tint;
                        if op.p.tint.is_some() {
                            r.tint = op.p.tint;
                        }
                        self.raster_ops_budgeted(r, bops, &s, bclip, alpha, depth + 1, budget);
                        r.tint = saved;
                    }
                }
            }
        }
    }

    /// Debug dump of the recorded window display list (one line per op,
    /// recursing into render nodes and bitmaps); used by `RD_TRACE_DRAW`.
    pub fn frame_summary(&self) -> String {
        let mut out = String::new();
        let mut budget = MAX_SUMMARY_OPS_VISITED;
        self.summ_ops(&self.gfx.frame, 0, &mut out, &mut budget);
        out
    }

    fn summ_ops(&self, ops: &[Op], depth: usize, out: &mut String, budget: &mut usize) {
        use std::fmt::Write;
        if depth > 32 || *budget == 0 {
            return;
        }
        for op in ops {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let pad = "  ".repeat(depth);
            let head = format!(
                "{pad}a={:.2} argb={:08x} clip={:?} ",
                op.alpha,
                op.p.argb,
                op.clip.map(|v| v as i32)
            );
            match &op.k {
                OpK::Node(n) => {
                    let _ = writeln!(out, "{head}Node");
                    if let Some(G::Node(ns)) = self.gfx.objs.get(n) {
                        self.summ_ops(&ns.ops, depth + 1, out, budget);
                    }
                }
                OpK::Bitmap(b, dst) => {
                    let _ = writeln!(out, "{head}Bitmap {:?}", dst.map(|v| v as i32));
                    if let Some(G::Bitmap(w, h, bops)) = self.gfx.objs.get(b) {
                        let _ = writeln!(out, "{pad}  ({w}x{h}, {} ops)", bops.len());
                        self.summ_ops(bops, depth + 1, out, budget);
                    }
                }
                OpK::Text(t, x, y, sz) => {
                    let _ = writeln!(out, "{head}Text {t:?} @{x},{y} sz={sz}");
                }
                OpK::Path(p, _) => {
                    let _ = writeln!(out, "{head}Path {} contours", p.len());
                }
                k => {
                    let _ = writeln!(out, "{head}{k:?}");
                }
            }
        }
    }

    /// True when the host draw pass recorded a window display list.
    pub fn has_frame(&self) -> bool {
        self.gfx.frame_valid && !self.gfx.frame.is_empty()
    }

    /// Paints the last recorded window display list onto `px` (RGB, w×h).
    pub fn rasterize_frame(&self, px: &mut [u8], w: usize, h: usize) {
        let mut r = Raster {
            px,
            w,
            h,
            tint: None,
        };
        let ops = self.gfx.frame.clone();
        self.raster_ops(&mut r, &ops, &ID, [0.0, 0.0, w as f32, h as f32], 1.0, 0);
    }

    /// Draw-op count of the window display list (diagnostics / runtime_health).
    pub fn frame_op_count(&self) -> usize {
        fn count(e: &Engine, ops: &[Op], d: usize, budget: &mut usize) -> usize {
            if d > 64 || *budget == 0 {
                return 0;
            }
            let mut total = 0usize;
            for o in ops {
                if *budget == 0 {
                    break;
                }
                *budget -= 1;
                total = total.saturating_add(1);
                if let OpK::Node(n) = &o.k {
                    if let Some(G::Node(ns)) = e.gfx.objs.get(n) {
                        total = total.saturating_add(count(e, &ns.ops, d + 1, budget));
                    }
                }
            }
            total
        }
        let mut budget = MAX_RASTER_OPS_VISITED;
        count(self, &self.gfx.frame, 0, &mut budget)
    }

    /// Starts recording the window frame; returns a host Canvas that targets it.
    pub(crate) fn begin_frame_canvas(&mut self, w: i32, h: i32) -> Result<ObjRef, VmExit> {
        self.gfx.frame.clear();
        self.gfx.frame_valid = false;
        let c = self
            .heap
            .alloc_instance("Landroid/graphics/Canvas;".into(), Vec::new())?;
        let mut cs = CanvasS::new(Target::Frame, w, h);
        cs.top().clip = [0.0, 0.0, w as f32, h as f32];
        self.gfx.objs.insert(c, G::Canvas(cs));
        Ok(c)
    }

    /// Records host-view backgrounds/text into the frame (for host views
    /// drawn around DEX content).
    pub(crate) fn frame_fill(&mut self, c: ObjRef, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        record(
            self,
            c,
            PaintSnap {
                argb,
                style: 0,
                sw: 0.0,
                clear: false,
                tint: None,
                ls: 0.0,
            },
            OpK::Rect(x, y, x + w, y + h),
        );
    }

    pub(crate) fn canvas_translate(&mut self, c: ObjRef, dx: f32, dy: f32) -> usize {
        if let Some(G::Canvas(cs)) = self.gfx.objs.get_mut(&c) {
            let s = *cs.top();
            cs.stack.push(s);
            let t = cs.top();
            t.m = mul(&t.m, &tr(dx, dy));
            return cs.stack.len() - 1;
        }
        0
    }
    pub(crate) fn canvas_restore_to(&mut self, c: ObjRef, n: usize) {
        if let Some(G::Canvas(cs)) = self.gfx.objs.get_mut(&c) {
            cs.stack.truncate(n.max(1));
        }
    }
    pub(crate) fn end_frame(&mut self) {
        self.gfx.frame_valid = true;
    }
}

/// `ColorFilter` value classes: `BlendModeColorFilter`,
/// `PorterDuffColorFilter`, `LightingColorFilter`, `ColorMatrixColorFilter`.
/// Their parameters are stored as fields (`rd_color`, `rd_mode`, `rd_mul`,
/// `rd_add`, `rd_matrix`).
///
/// **Deviation:** the rasterizer does not apply color filters; a paint's
/// filter is recorded but not rendered.
fn color_filter(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    const FILTERS: [&str; 4] = [
        "Landroid/graphics/BlendModeColorFilter;",
        "Landroid/graphics/PorterDuffColorFilter;",
        "Landroid/graphics/LightingColorFilter;",
        "Landroid/graphics/ColorMatrixColorFilter;",
    ];
    if !FILTERS.contains(&recv_class) {
        return Ok(None);
    }
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let put = |vm: &mut Engine, f: &str, v: Value| {
        crate::err::put_field_result(vm.heap.put_field(recv, f, v))
    };
    Ok(Some(match name {
        "<init>" => {
            if recv_class.contains("Lighting") {
                put(vm, "rd_mul", a(0))?;
                put(vm, "rd_add", a(1))?;
            } else if recv_class.contains("ColorMatrix") {
                put(vm, "rd_matrix", a(0))?;
            } else {
                put(vm, "rd_color", a(0))?;
                put(vm, "rd_mode", a(1))?;
            }
            Value::Null
        }
        "getColor" => vm.heap.get_field(recv, "rd_color").unwrap_or(Value::Int(0)),
        "getMode" => vm.heap.get_field(recv, "rd_mode").unwrap_or(Value::Null),
        "getColorMultiply" => vm.heap.get_field(recv, "rd_mul").unwrap_or(Value::Int(0)),
        "getColorAdd" => vm.heap.get_field(recv, "rd_add").unwrap_or(Value::Int(0)),
        "equals" => Value::Int((args.first() == Some(&Value::Obj(recv))) as i32),
        "hashCode" => {
            let c: i32 = vm
                .heap
                .get_field(recv, "rd_color")
                .ok()
                .and_then(|v| v.as_int().ok())
                .unwrap_or(0);
            Value::Int(c.wrapping_mul(31))
        }
        _ => return Ok(None),
    }))
}

/// `Xfermode` value classes: `PorterDuffXfermode` (API 1) and
/// `BlendModeXfermode` (API 29). Both store the transfer mode in the
/// `rd_mode` heap field; the rasterizer reads it back from the paint's
/// stored "Xfermode" object (`snap`) — CLEAR (ordinal 0) clears to
/// transparent. AOSP exposes no getters (the class is opaque to apps), so
/// only the constructors are modelled.
///
/// **Deviation:** the rasterizer only special-cases CLEAR; other modes are
/// recorded but not rendered.
fn xfermode(
    vm: &mut Engine,
    recv_class: &str,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    const MODES: [&str; 2] = [
        "Landroid/graphics/PorterDuffXfermode;",
        "Landroid/graphics/BlendModeXfermode;",
    ];
    if !MODES.contains(&recv_class) || name != "<init>" {
        return Ok(None);
    }
    if let Some(mode) = args.first() {
        crate::err::put_field_result(vm.heap.put_field(recv, "rd_mode", mode.clone()))?;
    }
    Ok(Some(Value::Null))
}

include!("screen_model.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_loads_and_measures() {
        let w = measure("Comprix", 32.0, 0.0);
        assert!(w > 80.0 && w < 200.0, "{w}");
        assert!(ascent(14.0) < 0.0 && descent(14.0) > 0.0);
        assert!(atlas().map.contains_key(&('ã' as u32)));
    }

    #[test]
    fn matrix_compose_and_invert() {
        let m = mul(&tr(10.0, 5.0), &sc(2.0, 3.0));
        assert_eq!(ap(&m, 1.0, 1.0), (12.0, 8.0));
        let i = inv(&m).unwrap();
        let (x, y) = ap(&i, 12.0, 8.0);
        assert!((x - 1.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
        assert_eq!(from9(&to9(&m)), m);
    }

    #[test]
    fn raster_fills_rect_and_text() {
        let mut px = vec![255u8; 100 * 50 * 3];
        let mut r = Raster {
            px: &mut px,
            w: 100,
            h: 50,
            tint: None,
        };
        r.fill_polys(
            &[vec![(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0)]],
            0xFFFF0000,
            [0.0, 0.0, 100.0, 50.0],
            false,
        );
        r.text(
            "Hi",
            &ID,
            (30.0, 40.0),
            (20.0, 0.0),
            0xFF000000,
            [0.0, 0.0, 100.0, 50.0],
        );
        let at = |x: usize, y: usize| px[(y * 100 + x) * 3];
        assert_eq!((at(15, 15), px[(15 * 100 + 15) * 3 + 1]), (255, 0));
        assert!(
            (30..50).any(|x| (25..40).any(|y| at(x, y) < 128)),
            "text drew ink"
        );
    }
}
