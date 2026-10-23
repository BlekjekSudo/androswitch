//! Portable raster output using RustyDroid's retained DejaVu glyph atlas.
use alloc::{collections::BTreeMap, vec, vec::Vec};
struct Glyph {
    advance: f32,
    left: i16,
    top: i16,
    width: usize,
    height: usize,
    offset: usize,
}
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    glyphs: BTreeMap<u32, Glyph>,
}
static FONT: &[u8] = include_bytes!("../../rd-vm/assets/glyphs.bin");
fn u16_at(p: usize) -> u16 {
    u16::from_le_bytes([FONT[p], FONT[p + 1]])
}
impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        let mut glyphs = BTreeMap::new();
        let mut p = 12;
        for _ in 0..u16_at(10) {
            let width = u16_at(p + 10) as usize;
            let height = u16_at(p + 12) as usize;
            glyphs.insert(
                u32::from_le_bytes(FONT[p..p + 4].try_into().unwrap()),
                Glyph {
                    advance: u16_at(p + 4) as f32 / 64.0,
                    left: u16_at(p + 6) as i16,
                    top: u16_at(p + 8) as i16,
                    width,
                    height,
                    offset: p + 14,
                },
            );
            p += 14 + width * height;
        }
        Self {
            width,
            height,
            pixels: vec![0xff101419; width * height],
            glyphs,
        }
    }
    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }
    pub fn pixel(&mut self, x: i32, y: i32, color: u32, coverage: u32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let at = y as usize * self.width + x as usize;
        let alpha = (color >> 24) * coverage / 255;
        let old = self.pixels[at];
        let channel = |shift: u32| {
            (((color >> shift) & 255u32) * alpha + ((old >> shift) & 255u32) * (255 - alpha)) / 255
        };
        self.pixels[at] = 0xff000000 | (channel(16) << 16) | (channel(8) << 8) | channel(0);
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        for yy in y.max(0)..(y.saturating_add(h)).min(self.height as i32) {
            for xx in x.max(0)..(x.saturating_add(w)).min(self.width as i32) {
                self.pixel(xx, yy, color, 255);
            }
        }
    }
    pub fn circle(&mut self, x: i32, y: i32, r: i32, color: u32) {
        let r = r.clamp(0, 4096);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.pixel(x + dx, y + dy, color, 255);
                }
            }
        }
    }
    pub fn text(&mut self, text: &str, x: i32, baseline: i32, size: f32, color: u32) {
        let scale = (size / u16_at(4) as f32).clamp(0.1, 8.0);
        let mut pen = x as f32;
        for c in text.chars() {
            let Some(g) = self
                .glyphs
                .get(&(c as u32))
                .or_else(|| self.glyphs.get(&0xfffd))
            else {
                continue;
            };
            let (left, top, width, height, offset, advance) =
                (g.left, g.top, g.width, g.height, g.offset, g.advance);
            let ox = pen + left as f32 * scale;
            let oy = baseline as f32 - top as f32 * scale;
            let w = libm::ceilf(width as f32 * scale) as usize;
            let h = libm::ceilf(height as f32 * scale) as usize;
            for yy in 0..h {
                for xx in 0..w {
                    let sx = ((xx as f32 / scale) as usize).min(width.saturating_sub(1));
                    let sy = ((yy as f32 / scale) as usize).min(height.saturating_sub(1));
                    if width > 0 && height > 0 {
                        self.pixel(
                            ox as i32 + xx as i32,
                            oy as i32 + yy as i32,
                            color,
                            FONT[offset + sy * width + sx] as u32,
                        );
                    }
                }
            }
            pen += advance * scale;
        }
    }
    pub fn checksum(&self) -> u64 {
        self.pixels.iter().fold(0xcbf29ce484222325, |hash, pixel| {
            (hash ^ *pixel as u64).wrapping_mul(0x100000001b3)
        })
    }
}
