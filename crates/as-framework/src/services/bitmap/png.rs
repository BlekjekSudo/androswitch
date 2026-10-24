//! Strict, bounded PNG decoder. The filter reconstruction follows the existing
//! AndroSwitch launcher decoder; indexed PNG adds real PLTE/tRNS pixels.
use alloc::{vec, vec::Vec};

pub(super) const MAX_INPUT: usize = 4 << 20;
pub(super) const MAX_PIXELS: usize = 4 << 20;
#[derive(Debug, PartialEq)]
pub(super) enum Failure {
    Malformed,
    Unsupported,
    Budget,
}
#[derive(Debug)]
pub(super) struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub has_alpha: bool,
}
fn be(data: &[u8]) -> Result<usize, Failure> {
    Ok(u32::from_be_bytes(data.get(..4).ok_or(Failure::Malformed)?.try_into().unwrap()) as usize)
}
pub(super) fn decode(data: &[u8]) -> Result<Image, Failure> {
    use Failure::*;
    if data.len() > MAX_INPUT {
        return Err(Budget);
    }
    if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err(Unsupported);
    }
    let (mut width, mut height, mut channels, mut kind, mut offset) =
        (0usize, 0usize, 0usize, 0u8, 8usize);
    let mut palette = Vec::new();
    let mut transparency = Vec::new();
    let mut compressed = Vec::new();
    let (mut ended, mut idat_ended) = (false, false);
    while offset < data.len() {
        let length = be(data.get(offset..).ok_or(Malformed)?)?;
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .ok_or(Malformed)?;
        let chunk = data
            .get(offset + 4..end.checked_sub(4).ok_or(Malformed)?)
            .ok_or(Malformed)?;
        let crc = be(data.get(end - 4..end).ok_or(Malformed)?)? as u32;
        if chunk.len() < 4 || rd_apk::zip::crc32(chunk) != crc {
            return Err(Malformed);
        }
        let tag = &chunk[..4];
        let payload = &chunk[4..];
        if offset == 8 && tag != b"IHDR" {
            return Err(Malformed);
        }
        if !compressed.is_empty() && tag != b"IDAT" {
            idat_ended = true;
        }
        match tag {
            b"IHDR" => {
                if width != 0 || offset != 8 || length != 13 {
                    return Err(Malformed);
                }
                width = be(payload)?;
                height = be(&payload[4..])?;
                if width == 0 || height == 0 {
                    return Err(Malformed);
                }
                if width > 4096
                    || height > 4096
                    || width.checked_mul(height).ok_or(Budget)? > MAX_PIXELS
                {
                    return Err(Budget);
                }
                if payload[8] != 8 || payload[10..13] != [0, 0, 0] {
                    return Err(Unsupported);
                }
                kind = payload[9];
                channels = match kind {
                    2 => 3,
                    3 => 1,
                    6 => 4,
                    _ => return Err(Unsupported),
                };
            }
            b"PLTE" => {
                if !palette.is_empty()
                    || !compressed.is_empty()
                    || length == 0
                    || length > 768
                    || length % 3 != 0
                {
                    return Err(Malformed);
                }
                palette.extend_from_slice(payload);
            }
            b"tRNS" => {
                if !transparency.is_empty() || !compressed.is_empty() {
                    return Err(Malformed);
                }
                if kind != 3 {
                    return Err(Unsupported);
                }
                if palette.is_empty() || length == 0 || length > palette.len() / 3 {
                    return Err(Malformed);
                }
                transparency.extend_from_slice(payload);
            }
            b"IDAT" => {
                if channels == 0 || idat_ended || (kind == 3 && palette.is_empty()) {
                    return Err(Malformed);
                }
                if compressed.len().checked_add(length).ok_or(Budget)? > MAX_INPUT {
                    return Err(Budget);
                }
                compressed.extend_from_slice(payload);
            }
            b"IEND" => {
                if length != 0 || compressed.is_empty() || end != data.len() {
                    return Err(Malformed);
                }
                ended = true;
                break;
            }
            b"iCCP" | b"cICP" | b"acTL" => return Err(Unsupported),
            b"sRGB" => {
                if payload.len() != 1 || payload[0] > 3 {
                    return Err(Malformed);
                }
            }
            b"gAMA" => {
                if be(payload)? != 45455 || payload.len() != 4 {
                    return Err(Unsupported);
                }
            }
            tag if tag[0] & 32 == 0 => return Err(Unsupported),
            _ => {}
        }
        offset = end;
    }
    if !ended || channels == 0 {
        return Err(Malformed);
    }
    let stride = width * channels;
    let expected = (stride + 1) * height;
    let mut raw = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&compressed, expected)
        .map_err(|_| Malformed)?;
    if raw.len() != expected {
        return Err(Malformed);
    }
    drop(compressed);
    // Reconstruct in place, leaving the row filter byte in its original slot.
    for y in 0..height {
        let row = y * (stride + 1);
        let filter = raw[row];
        if filter > 4 {
            return Err(Malformed);
        }
        for x in 0..stride {
            let at = row + 1 + x;
            let a = if x >= channels { raw[at - channels] } else { 0 };
            let b = if y > 0 { raw[at - stride - 1] } else { 0 };
            let c = if y > 0 && x >= channels {
                raw[at - stride - 1 - channels]
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
                _ => unreachable!(),
            };
            raw[at] = raw[at].wrapping_add(predictor);
        }
    }
    let mut pixels = vec![0; width * height];
    for y in 0..height {
        for x in 0..width {
            let p = &raw[y * (stride + 1) + 1 + x * channels..];
            let (r, g, b, a) = match kind {
                2 => (p[0], p[1], p[2], 255),
                6 => (p[0], p[1], p[2], p[3]),
                3 => {
                    let index = p[0] as usize;
                    let rgb = palette.get(index * 3..index * 3 + 3).ok_or(Malformed)?;
                    (
                        rgb[0],
                        rgb[1],
                        rgb[2],
                        transparency.get(index).copied().unwrap_or(255),
                    )
                }
                _ => unreachable!(),
            };
            // Default BitmapFactory pixels are premultiplied. Fully transparent
            // colors have no RGB information in that representation/getPixel.
            pixels[y * width + x] = if a == 0 {
                0
            } else {
                (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
            };
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
        has_alpha: kind == 6 || !transparency.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(out: &mut Vec<u8>, tag: &[u8], payload: &[u8]) {
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(tag);
        out.extend_from_slice(payload);
        let crc = rd_apk::zip::crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    fn image(kind: u8, samples: &[u8], palette: &[u8]) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = vec![];
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&[8, kind, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        if !palette.is_empty() {
            chunk(&mut out, b"PLTE", palette);
        }
        let mut raw = vec![0];
        raw.extend_from_slice(samples);
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        chunk(&mut out, b"IDAT", &compressed);
        chunk(&mut out, b"IEND", &[]);
        out
    }
    #[test]
    fn rgb_pixels_and_crc_palette_indices_truncation_and_critical_chunks_are_checked() {
        let valid = image(2, &[11, 72, 243], &[]);
        assert_eq!(decode(&valid).unwrap().pixels, vec![0xff0b48f3]);
        assert_eq!(
            decode(&image(6, &[255, 72, 243, 0], &[])).unwrap().pixels,
            vec![0]
        );
        let mut bad = valid.clone();
        bad[29] ^= 1;
        assert_eq!(decode(&bad).err(), Some(Failure::Malformed));
        assert_eq!(
            decode(&valid[..valid.len() - 1]).err(),
            Some(Failure::Malformed)
        );
        assert_eq!(
            decode(&image(3, &[1], &[255, 0, 0])).err(),
            Some(Failure::Malformed)
        );
        let mut unsupported = valid[..33].to_vec();
        chunk(&mut unsupported, b"WHAT", &[]);
        unsupported.extend_from_slice(&valid[33..]);
        assert_eq!(decode(&unsupported).err(), Some(Failure::Unsupported));
    }
}
