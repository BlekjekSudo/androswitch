//! Screenshot headless (M4 DoD: "screenshot correto").
//!
//! Software renderer determinístico: framebuffer RGB 720×1440 (viewport do
//! M3.2), pinta a árvore de views (retângulo por classe/estado + texto legível
//! pelo mesmo atlas tipográfico da Canvas da VM; não é pixel-perfect Android), e codifica PNG com filtros de linha adaptativos
//! (None/Sub/Up) e deflate rápido pelo flate2 puro-Rust já usado no rd-apk
//! (antes: blocos STORED, ~3,1 MB por tela). Mesmo input → mesmos bytes.

use rd_vm::framework::{UiNode, Vis};

pub const VIEW_W: usize = 720;
pub const VIEW_H: usize = 1440;

pub type Rgb = (u8, u8, u8);

pub struct Framebuffer {
    pub w: usize,
    pub h: usize,
    /// RGB compacto (w * h * 3).
    pub px: Vec<u8>,
}

impl Framebuffer {
    pub fn new(bg: Rgb) -> Self {
        let mut px = Vec::with_capacity(VIEW_W * VIEW_H * 3);
        for _ in 0..VIEW_W * VIEW_H {
            px.extend_from_slice(&[bg.0, bg.1, bg.2]);
        }
        Framebuffer {
            w: VIEW_W,
            h: VIEW_H,
            px,
        }
    }

    pub fn pixel_mut(&mut self, x: i32, y: i32) -> Option<&mut [u8]> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        let i = ((y as usize) * self.w + x as usize) * 3;
        self.px.get_mut(i..i + 3)
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        // issue #62: bounds hostis da API pública (UiNode de qualquer origem)
        // não podem panicar em debug (x+w com i32::MAX) — saturating em ambos
        // os extremos; retângulo fora do framebuffer simplesmente não pinta.
        for yy in y.max(0)..y.saturating_add(h).min(self.h as i32) {
            for xx in x.max(0)..x.saturating_add(w).min(self.w as i32) {
                if let Some(p) = self.pixel_mut(xx, yy) {
                    p[0] = c.0;
                    p[1] = c.1;
                    p[2] = c.2;
                }
            }
        }
    }

    pub fn rect_border(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.rect(x, y, w, 1, c);
        self.rect(x, y.saturating_add(h.saturating_sub(1)), w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x.saturating_add(w.saturating_sub(1)), y, 1, h, c);
    }

    /// Single-line snapshot text using the VM's embedded, antialiased font.
    /// Dimensions and hostile coordinates are validated/clipped in the rasterizer.
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: Rgb) {
        rd_vm::graphics::rasterize_ui_text_rgb(&mut self.px, (self.w, self.h), (x, y), s, c);
    }

    /// Codifica o framebuffer como PNG RGB 8-bit (zlib STORED — sem deps).
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        chunk(&mut out, b"IHDR", {
            let mut h = Vec::new();
            h.extend((self.w as u32).to_be_bytes());
            h.extend((self.h as u32).to_be_bytes());
            h.push(8); // bit depth
            h.push(2); // color type RGB
            h.push(0); // compression
            h.push(0); // filter
            h.push(0); // interlace
            h
        });
        chunk(
            &mut out,
            b"IDAT",
            zlib_fast(&filtered_scanlines(&self.px, self.w, self.h)),
        );
        chunk(&mut out, b"IEND", Vec::new());
        out
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: Vec<u8>) {
    out.extend((data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(&data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(&data);
    out.extend(crc32(&crc_input).to_be_bytes());
}

/// PNG scanlines with a per-row adaptive filter (None/Sub/Up, smallest sum
/// of absolute residuals — the libpng heuristic). Flat UI rows become runs
/// of zeros that deflate to almost nothing.
fn filtered_scanlines(px: &[u8], w: usize, h: usize) -> Vec<u8> {
    let stride = w * 3;
    let mut raw = Vec::with_capacity(h * (1 + stride));
    let mut cand = [vec![0u8; stride], vec![0u8; stride], vec![0u8; stride]];
    for y in 0..h {
        let row = &px[y * stride..(y + 1) * stride];
        let up = if y > 0 {
            Some(&px[(y - 1) * stride..y * stride])
        } else {
            None
        };
        // ADR 008 fast path: a row identical to the one above filters to an
        // all-zero Up row — the adaptive heuristic below would pick exactly
        // that (score 0), so emitting it directly is byte-identical output
        // in O(stride) memcmp instead of 3 candidate builds + scoring.
        if let Some(prev) = up {
            if prev == row {
                raw.push(2); // Up
                raw.resize(raw.len() + stride, 0);
                continue;
            }
        }
        for i in 0..stride {
            let a = if i >= 3 { row[i - 3] } else { 0 };
            let b = up.map(|u| u[i]).unwrap_or(0);
            cand[0][i] = row[i];
            cand[1][i] = row[i].wrapping_sub(a);
            cand[2][i] = row[i].wrapping_sub(b);
        }
        let score = |v: &[u8]| -> u64 { v.iter().map(|&x| (x as i8).unsigned_abs() as u64).sum() };
        let (best, _) = cand
            .iter()
            .enumerate()
            .map(|(k, v)| (k, score(v)))
            .min_by_key(|&(_, sc)| sc)
            .unwrap_or((0, 0));
        raw.push(best as u8); // 0 None · 1 Sub · 2 Up
        raw.extend_from_slice(&cand[best]);
    }
    raw
}

/// zlib stream (deflate, fast level) via flate2's pure-Rust backend — the
/// same one rd-apk uses for ZIP entries. Screenshots went from ~3.1 MB
/// (stored blocks) to tens of KB, deterministic for identical frames.
fn zlib_fast(raw: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(
        Vec::with_capacity(raw.len() / 16),
        flate2::Compression::fast(),
    );
    // writing into a Vec cannot fail
    let _ = e.write_all(raw);
    e.finish().unwrap_or_default()
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub fn render_snapshot(root: &UiNode) -> Framebuffer {
    let mut fb = Framebuffer::new((0xF6, 0xF6, 0xF6)); // fundo da window
    paint(root, &mut fb, 0);
    fb
}

fn paint(n: &UiNode, fb: &mut Framebuffer, depth: usize) {
    // mesmo cap do dump (issue #40)
    if depth > 512 {
        return;
    }
    // INVISIBLE não desenha (nem a subárvore — Android real); GONE nem existe
    if n.visibility != Vis::Visible {
        return;
    }
    let (x, y, w, h) = n.bounds;
    let is_button = n.class.ends_with("Button");
    let color = if is_button {
        (0xD6, 0xD7, 0xD8)
    } else if n.children.is_empty() {
        (0xFF, 0xFF, 0xFF)
    } else {
        (0xE8, 0xE8, 0xE8)
    };
    fb.rect(x, y, w, h, color);
    if n.clickable {
        fb.rect_border(x, y, w, h, (0x60, 0x60, 0x60));
    }
    if !n.text.is_empty() {
        // issue #62: offsets/centralização saturados (bounds hostis)
        let ty = y.saturating_add(h.saturating_sub(18).max(0) / 2);
        fb.text(x.saturating_add(4), ty, &n.text, (0x1B, 0x1B, 0x1B));
    }
    for c in &n.children {
        paint(c, fb, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(class: &str, text: &str, bounds: (i32, i32, i32, i32)) -> UiNode {
        UiNode {
            class: class.to_string(),
            resource_id: None,
            text: text.to_string(),
            bounds,
            visibility: Vis::Visible,
            enabled: true,
            clickable: false,
            focusable: false,
            truncated: false,
            children: Vec::new(),
        }
    }

    #[test]
    fn real_letters_and_digits_have_distinct_glyphs_not_parity_placeholders() {
        let mut a = Framebuffer::new((255, 255, 255));
        let mut b = Framebuffer::new((255, 255, 255));
        a.text(8, 8, "A 1", (0, 0, 0));
        b.text(8, 8, "C 3", (0, 0, 0));
        assert_ne!(
            a.px, b.px,
            "old parity checkerboard produced identical text"
        );
        assert!(
            a.px.iter().any(|v| *v > 0 && *v < 255),
            "font is antialiased"
        );
    }
    #[test]
    fn text_rejects_inconsistent_dimensions_without_writing_or_panicking() {
        let mut pixels = vec![255; 12];
        let before = pixels.clone();
        assert!(!rd_vm::graphics::rasterize_ui_text_rgb(
            &mut pixels,
            (usize::MAX, 2),
            (0, 0),
            "test",
            (0, 0, 0)
        ));
        assert_eq!(pixels, before);
        assert!(rd_vm::graphics::rasterize_ui_text_rgb(
            &mut pixels,
            (2, 2),
            (i32::MAX, i32::MIN),
            "test",
            (0, 0, 0)
        ));
        assert_eq!(pixels, before);
    }
    #[test]
    fn png_is_valid_and_deterministic() {
        let mut root = node("android.widget.LinearLayout", "", (0, 0, 720, 96));
        root.children
            .push(node("android.widget.TextView", "Olá", (0, 0, 720, 48)));
        let png1 = render_snapshot(&root).to_png();
        let png2 = render_snapshot(&root).to_png();
        assert_eq!(png1, png2, "mesmo input → mesmos bytes");
        assert_eq!(
            &png1[..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
        // IHDR: width=720 height=1440 @ 16..24
        let w = u32::from_be_bytes([png1[16], png1[17], png1[18], png1[19]]);
        let h = u32::from_be_bytes([png1[20], png1[21], png1[22], png1[23]]);
        assert_eq!((w, h), (720, 1440));
    }

    #[test]
    fn view_changes_pixels_in_its_bounds() {
        let mut fb = Framebuffer::new((0xF6, 0xF6, 0xF6));
        let before = fb.px.clone();
        fb.rect(10, 10, 100, 40, (0xFF, 0x00, 0x00));
        assert_ne!(fb.px, before, "rect pinta");
        // dentro/fora
        assert_eq!(
            (fb.px[(15 * 720 + 20) * 3], (0xFF, 0x00, 0x00).0),
            (0xFF, 0xFF)
        );
    }

    #[test]
    fn invisible_subtree_is_not_painted() {
        let mut root = node("android.widget.LinearLayout", "", (0, 0, 720, 96));
        let mut child = node("android.widget.TextView", "x", (0, 0, 720, 48));
        child.visibility = Vis::Invisible;
        root.children.push(child);
        let fb = render_snapshot(&root);
        // pixel do centro do filho = cor do PAI (contêiner 0xE8) — o filho
        // INVISIBLE não pinta por cima (seria branco de TextView)
        let i = (24 * 720 + 360) * 3;
        assert_eq!((fb.px[i], fb.px[i + 1], fb.px[i + 2]), (0xE8, 0xE8, 0xE8));
    }

    #[test]
    fn png_idat_roundtrips_and_is_compressed() {
        use std::io::Read;
        let mut root = node("android.widget.LinearLayout", "", (0, 0, 720, 400));
        root.children
            .push(node("android.widget.TextView", "olá", (0, 0, 720, 96)));
        let fb = render_snapshot(&root);
        let png = fb.to_png();
        // IDAT payload: after signature (8) + IHDR chunk (25) + IDAT len/type (8)
        let len = u32::from_be_bytes([png[33], png[34], png[35], png[36]]) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(&png[41..41 + len])
            .read_to_end(&mut raw)
            .expect("valid zlib");
        let stride = fb.w * 3;
        assert_eq!(raw.len(), fb.h * (1 + stride));
        // undo the filters and compare with the framebuffer
        let mut prev = vec![0u8; stride];
        for y in 0..fb.h {
            let f = raw[y * (1 + stride)];
            let mut row = raw[y * (1 + stride) + 1..(y + 1) * (1 + stride)].to_vec();
            for i in 0..stride {
                let a = if i >= 3 { row[i - 3] } else { 0 };
                row[i] = match f {
                    0 => row[i],
                    1 => row[i].wrapping_add(a),
                    2 => row[i].wrapping_add(prev[i]),
                    _ => panic!("filter {f}"),
                };
            }
            assert_eq!(&row[..], &fb.px[y * stride..(y + 1) * stride], "row {y}");
            prev = row;
        }
        assert!(
            png.len() < fb.px.len() / 20,
            "flat UI compresses: {} bytes",
            png.len()
        );
    }

    /// issue #62: UiNode hostil (bounds i32::MAX/MIN) NÃO panic no render —
    /// saturating; o rect fora do framebuffer simplesmente não pinta.
    #[test]
    fn hostile_bounds_do_not_panic_in_render() {
        let n = node("C", "x", (i32::MAX, 0, 1, 1));
        let _ = render_snapshot(&n); // sem panic = sucesso
        let n = node("C", "x", (0, i32::MIN, i32::MAX, i32::MAX));
        let _ = render_snapshot(&n);
        let n = node("C", "x", (i32::MIN, i32::MIN, i32::MAX, i32::MAX));
        let _ = render_snapshot(&n);
        let mut root = node("Root", "", (0, 0, 720, 96));
        root.children
            .push(node("C", "y", (i32::MAX, i32::MAX, i32::MAX, i32::MAX)));
        let _ = render_snapshot(&root);
    }

    #[test]
    fn zero_or_negative_border_size_paints_nothing() {
        let mut fb = Framebuffer::new((0xF6, 0xF6, 0xF6));
        let before = fb.px.clone();
        fb.rect_border(10, 20, 100, 0, (0xFF, 0, 0));
        fb.rect_border(10, 20, 0, 100, (0xFF, 0, 0));
        fb.rect_border(10, 20, -5, 100, (0xFF, 0, 0));
        fb.rect_border(10, 20, 100, -5, (0xFF, 0, 0));
        assert!(fb.px == before, "collapsed border must paint nothing");
    }
}
