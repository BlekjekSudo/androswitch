//! Screenshot headless (M4 DoD: "screenshot correto").
//!
//! Software renderer determinístico: framebuffer RGB 720×1440 (viewport do
//! M3.2), pinta a árvore de views (retângulo por classe/estado + texto via
//! glifos de bloco 3×7 — fonte tipográfica real é M8/M9, o Android real usa
//! skia/FreeType), e codifica PNG sem dependências externas (zlib com blocos
//! STORED + crc32/adler32 próprios). Mesmo input → mesmos bytes.

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
        self.rect(x, y, w, 1, c);
        self.rect(x, y.saturating_add(h.saturating_sub(1)), w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x.saturating_add(w.saturating_sub(1)), y, 1, h, c);
    }

    /// Texto com glifos de bloco 3×7 (determinístico; fonte tipográfica real
    /// é M8/M9). Caracteres fora do ASCII imprimável viram bloco sólido —
    /// acentos do português incluídos.
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: Rgb) {
        let mut cx = x;
        for ch in s.chars() {
            draw_glyph_block(self, cx, y, ch, c);
            // issue #62: text hostil gigante não pode overflowar o cursor
            cx = cx.saturating_add(6); // 3 px de glifo + 3 de espaçamento
        }
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
        // raw scanlines com filter byte 0
        let mut raw = Vec::with_capacity(self.h * (1 + self.w * 3));
        for y in 0..self.h {
            raw.push(0);
            let start = y * self.w * 3;
            raw.extend_from_slice(&self.px[start..start + self.w * 3]);
        }
        chunk(&mut out, b"IDAT", zlib_stored(&raw));
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

/// zlib stream com blocos STORED (BTYPE=00) — válido e sem dependências.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut i = 0usize;
    while i < raw.len() {
        let n = (raw.len() - i).min(65535);
        let last = if i + n >= raw.len() { 1u8 } else { 0u8 };
        z.push(last);
        z.extend((n as u16).to_le_bytes());
        z.extend((!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[i..i + n]);
        i += n;
    }
    z.extend(adler32(raw).to_be_bytes());
    z
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

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn draw_glyph_block(fb: &mut Framebuffer, x: i32, y: i32, ch: char, c: Rgb) {
    // bloco 3×7; ASCII imprimável desenha meia-altura com padrão alternado
    // por char (parece texto); não-ASCII (acentos) vira bloco sólido 3×7
    let ascii = ch.is_ascii_graphic();
    let h = if ascii { 4 } else { 7 };
    for dy in 0..h {
        for dx in 0..3i32 {
            let solid = if ascii {
                (dx as u32 + dy as u32 + ch as u32) % 2 == 0
            } else {
                true
            };
            if solid {
                // issue #62: cursor saturado (texto hostil gigante) não panic
                fb.rect(x.saturating_add(dx), y.saturating_add(dy), 1, 1, c);
            }
        }
    }
}

/// Renderiza a árvore num framebuffer (determinístico).
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
        let ty = y.saturating_add(h.saturating_sub(8).max(0) / 2);
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
    fn zlib_stored_roundtrips_with_adler() {
        let raw = b"hello rustydroid hello rustydroid";
        let z = zlib_stored(raw);
        assert_eq!(z[0], 0x78);
        assert_eq!(&z[z.len() - 4..], &adler32(raw).to_be_bytes());
        // bloco stored: primeiro byte = BFINAL|BTYPE=00 → 1 (único bloco final)
        assert_eq!(z[2] & 0x01, 1);
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
}
