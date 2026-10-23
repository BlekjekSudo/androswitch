//! Bounded PNG icons (8-bit RGB/RGBA, non-interlaced). Unsupported formats use
//! the launcher's visible fallback icon. CRC and decompression size are checked.
use alloc::{vec, vec::Vec};
use as_framework::Frame;
pub struct Icon {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}
fn be(bytes: &[u8]) -> Option<usize> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize)
}
pub fn decode(bytes: &[u8]) -> Option<Icon> {
    if bytes.len() > 4 << 20 || bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let (mut width, mut height, mut channels, mut offset) = (0usize, 0usize, 0usize, 8usize);
    let mut compressed = Vec::new();
    let mut end = false;
    while offset + 12 <= bytes.len() {
        let length = be(&bytes[offset..])?;
        let chunk = bytes.get(offset + 4..offset + 8 + length)?;
        let crc = be(bytes.get(offset + 8 + length..)?)? as u32;
        if rd_apk::zip::crc32(chunk) != crc {
            return None;
        }
        match &chunk[..4] {
            b"IHDR" => {
                if width != 0 || length != 13 {
                    return None;
                }
                width = be(&chunk[4..])?;
                height = be(&chunk[8..])?;
                channels = match chunk[13] {
                    2 => 3,
                    6 => 4,
                    _ => return None,
                };
                if width == 0
                    || height == 0
                    || width > 512
                    || height > 512
                    || chunk[12] != 8
                    || chunk[14..17] != [0, 0, 0]
                {
                    return None;
                }
            }
            b"IDAT" => compressed.extend_from_slice(&chunk[4..]),
            b"IEND" => {
                end = true;
                break;
            }
            tag if tag[0] & 32 == 0 => return None,
            _ => {}
        }
        offset = offset.checked_add(length + 12)?;
    }
    if !end || channels == 0 {
        return None;
    }
    let stride = width * channels;
    let expected = (stride + 1) * height;
    let raw =
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&compressed, expected).ok()?;
    if raw.len() != expected {
        return None;
    }
    let mut pixels = vec![0u8; stride * height];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        if filter > 4 {
            return None;
        }
        for x in 0..stride {
            let a = if x >= channels {
                pixels[y * stride + x - channels]
            } else {
                0
            };
            let b = if y > 0 {
                pixels[(y - 1) * stride + x]
            } else {
                0
            };
            let c = if y > 0 && x >= channels {
                pixels[(y - 1) * stride + x - channels]
            } else {
                0
            };
            let predictor = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => {
                    let p = a as i32 + b as i32 - c as i32;
                    let (pa, pb, pc) = (
                        (p - a as i32).abs(),
                        (p - b as i32).abs(),
                        (p - c as i32).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => return None,
            };
            pixels[y * stride + x] = raw[y * (stride + 1) + 1 + x].wrapping_add(predictor);
        }
    }
    Some(Icon {
        width,
        height,
        pixels: pixels
            .chunks_exact(channels)
            .map(|p| {
                ((if channels == 4 { p[3] } else { 255 }) as u32) << 24
                    | (p[0] as u32) << 16
                    | (p[1] as u32) << 8
                    | p[2] as u32
            })
            .collect(),
    })
}
impl Icon {
    pub fn draw(&self, frame: &mut Frame, x: i32, y: i32, size: i32) {
        for dy in 0..size {
            for dx in 0..size {
                frame.pixel(
                    x + dx,
                    y + dy,
                    self.pixels[(dy as usize * self.height / size as usize) * self.width
                        + dx as usize * self.width / size as usize],
                    255,
                );
            }
        }
    }
}
