//! Resource-backed immutable ARGB bitmaps. No application identity is consulted.
//! Pixel leases remain charged while either Java or a recorded Canvas owns them.
use super::*;
use core::cell::Cell;
mod png;
const BITMAP: &str = "Landroid/graphics/Bitmap;";
const FACTORY: &str = "Landroid/graphics/BitmapFactory;";
const RECT: &str = "Landroid/graphics/Rect;";
const RECTF: &str = "Landroid/graphics/RectF;";
const OPTIONS: &str = "Landroid/graphics/BitmapFactory$Options;";
const TOTAL_BYTES: usize = 32 << 20;
const WORKING_BYTES: usize = 64 << 20;
struct Pixels {
    width: usize,
    height: usize,
    values: Vec<u32>,
    charged: Rc<Cell<usize>>,
}
impl Drop for Pixels {
    fn drop(&mut self) {
        self.charged.set(self.charged.get() - self.values.len() * 4);
    }
}
struct Bitmap {
    pixels: Option<Rc<Pixels>>,
    width: usize,
    height: usize,
    density: i32,
    alpha: bool,
}
#[derive(Default)]
pub(crate) struct Bitmaps {
    objects: RefCell<BTreeMap<ObjRef, Bitmap>>,
    charged: Rc<Cell<usize>>,
}
#[derive(Clone)]
pub(crate) struct BitmapDraw {
    pixels: Rc<Pixels>,
    source: [i32; 4],
    destination: [f32; 4],
    alpha: u32,
}
impl BitmapDraw {
    pub(crate) fn render(&self, frame: &mut Frame, x: i32, y: i32, width: i32, height: i32) {
        let [left, top, right, bottom] = self.destination;
        if right <= left || bottom <= top {
            return;
        }
        let [sl, st, sr, sb] = self.source;
        for dy in (libm::floorf(top) as i32).max(0)..(libm::ceilf(bottom) as i32).min(height) {
            for dx in (libm::floorf(left) as i32).max(0)..(libm::ceilf(right) as i32).min(width) {
                let sx = sl
                    + libm::floorf((dx as f32 + 0.5 - left) * (sr - sl) as f32 / (right - left))
                        as i32;
                let sy = st
                    + libm::floorf((dy as f32 + 0.5 - top) * (sb - st) as f32 / (bottom - top))
                        as i32;
                if sx >= sl && sx < sr && sy >= st && sy < sb {
                    frame.pixel(
                        x + dx,
                        y + dy,
                        self.pixels.values[sy as usize * self.pixels.width + sx as usize],
                        self.alpha,
                    );
                }
            }
        }
    }
}
fn unavailable(cause: &str) -> VmExit {
    error("API_UNAVAILABLE", cause)
}
fn illegal(cause: &str) -> VmExit {
    Throwable::new("Ljava/lang/IllegalArgumentException;", cause).into()
}
fn recycled() -> VmExit {
    Throwable::new(
        "Ljava/lang/IllegalStateException;",
        "bitmap has been recycled",
    )
    .into()
}
fn missing() -> VmExit {
    error("INVALID_BITMAP", "Bitmap not owned by this runtime")
}
fn resource(apk: &Apk, id: u32, density: i32, depth: usize) -> Result<Option<(String, i32)>> {
    if depth > 8 {
        return Err(error("RESOURCE_CYCLE", format!("{id:#x}")));
    }
    let Some(entries) = apk
        .arsc
        .as_ref()
        .and_then(|table| table.package_for(id))
        .and_then(|package| package.entries.get(&id))
    else {
        return Ok(None);
    };
    if entries.len() > 1024 {
        return Err(error(
            "RESOURCE_BUDGET",
            "1024 variants per bitmap resource",
        ));
    }
    let mut best: Option<&rd_apk::arsc::ResEntry> = None;
    for entry in entries
        .iter()
        .filter(|entry| entry.config.api_level.unwrap_or(0) <= 26)
    {
        if entry.config.unsupported_qualifiers
            || entry.config.language.is_some()
            || entry.config.country.is_some()
        {
            return Err(unavailable(
                "Bitmap resource configuration beyond density/API",
            ));
        }
        if best.is_none_or(|old| {
            let (a, b) = (
                entry.config.density.unwrap_or(0),
                old.config.density.unwrap_or(0),
            );
            if a != b {
                better_density(a, b, density)
            } else {
                entry.config.api_level.unwrap_or(0) > old.config.api_level.unwrap_or(0)
            }
        }) {
            best = Some(entry);
        }
    }
    let Some(entry) = best else {
        return Ok(None);
    };
    if entry.complex {
        return Err(unavailable("Bitmap resource bags/styles"));
    }
    match entry.data_type {
        1 => resource(apk, entry.data, density, depth + 1),
        3 => Ok(Some((
            entry
                .string
                .clone()
                .ok_or_else(|| error("RESOURCE_TYPE", "missing bitmap path"))?,
            match entry.config.density.unwrap_or(0) {
                0 => 160,
                0xffff => 0,
                0xfffe => return Err(unavailable("anydpi bitmap resource")),
                n => n as i32,
            },
        ))),
        _ => Ok(None),
    }
}
// API26 ResTable_config::isBetterThan density ordering, including downscale bias.
fn better_density(a: u16, b: u16, requested: i32) -> bool {
    if a == 0xfffe {
        return true;
    }
    if b == 0xfffe {
        return false;
    }
    let (a, b) = (
        if a == 0 { 160 } else { a as i64 },
        if b == 0 { 160 } else { b as i64 },
    );
    let requested = requested as i64;
    let (h, l, mine) = if a > b { (a, b, true) } else { (b, a, false) };
    if requested >= h {
        mine
    } else if l >= requested || (2 * l - requested) * h > requested * requested {
        !mine
    } else {
        mine
    }
}
fn target_density(android: &Android) -> i32 {
    android.graphics_caps().map_or(160, |caps| caps.density_dpi)
}
impl Bitmaps {
    pub(crate) fn bytes(&self) -> usize {
        self.charged.get()
    }
    pub(crate) fn collect(&self, marked: &[bool]) {
        self.objects
            .borrow_mut()
            .retain(|r, _| marked.get(*r as usize).copied().unwrap_or(false));
    }
    pub(crate) fn attach(&self, vm: &mut Engine, class: &str, r: ObjRef) -> Result<bool> {
        let class = if vm.cp.is_subtype(class, RECTF) {
            RECTF
        } else if vm.cp.is_subtype(class, RECT) {
            RECT
        } else if vm.cp.is_subtype(class, OPTIONS) {
            OPTIONS
        } else {
            class
        };
        if matches!(class, RECT | RECTF) {
            for field in ["left", "top", "right", "bottom"] {
                write(
                    vm,
                    r,
                    field,
                    if class == RECTF {
                        Value::Float(0.)
                    } else {
                        Value::Int(0)
                    },
                )?;
            }
            return Ok(true);
        }
        if class == OPTIONS {
            return Ok(true);
        }
        Ok(false)
    }
    fn decode(
        &self,
        vm: &mut Engine,
        bytes: &[u8],
        source_density: i32,
        density: i32,
    ) -> Result<Value> {
        if self.objects.borrow().len() >= 4096 {
            return Err(error(
                "BITMAP_OBJECT_BUDGET",
                "4096 live/uncollected Bitmap objects",
            ));
        }
        if bytes.len() > png::MAX_INPUT {
            return Err(error(
                "BITMAP_MEMORY_BUDGET",
                "encoded bitmap exceeds 4 MiB",
            ));
        }
        // Header estimate is only an admission bound; strict decoder validates it.
        if bytes.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
            return Err(unavailable(
                "BitmapFactory supports PNG RGB/RGBA/indexed 8-bit, non-interlaced",
            ));
        }
        let Some(header) = bytes.get(16..24) else {
            return Ok(Value::Null);
        };
        let width = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let height = u32::from_be_bytes(header[4..].try_into().unwrap()) as usize;
        let scale = |n: usize| -> Result<usize> {
            if source_density == 0 {
                Ok(n)
            } else {
                n.checked_mul(density as usize)
                    .and_then(|n| n.checked_add(source_density as usize / 2))
                    .map(|n| n / source_density as usize)
                    .ok_or_else(|| error("BITMAP_MEMORY_BUDGET", "density scaling overflow"))
            }
        };
        let (tw, th) = (scale(width)?, scale(height)?);
        let count = tw
            .checked_mul(th)
            .ok_or_else(|| error("BITMAP_MEMORY_BUDGET", "pixel count overflow"))?;
        let scratch = width
            .saturating_mul(height)
            .saturating_mul(9)
            .saturating_add(count.saturating_mul(4))
            .saturating_add(bytes.len() * 2);
        if width > 4096
            || height > 4096
            || count > png::MAX_PIXELS
            || self.bytes().saturating_add(count.saturating_mul(4)) > TOTAL_BYTES
            || self.bytes().saturating_add(scratch) > WORKING_BYTES
        {
            return Err(error(
                "BITMAP_MEMORY_BUDGET",
                "16 MiB per bitmap, 32 MiB retained pixels, 64 MiB decode working set",
            ));
        }
        let mut image = match png::decode(bytes) {
            Ok(image) => image,
            Err(png::Failure::Malformed) => return Ok(Value::Null),
            Err(png::Failure::Unsupported) => {
                return Err(unavailable("PNG encoding/chunk not implemented"))
            }
            Err(png::Failure::Budget) => {
                return Err(error("BITMAP_MEMORY_BUDGET", "PNG dimensions/input budget"))
            }
        };
        if tw == 0 || th == 0 {
            return Ok(Value::Null);
        }
        if tw != image.width || th != image.height {
            image.pixels = resize(&image.pixels, image.width, image.height, tw, th);
            image.width = tw;
            image.height = th;
        }
        let r = vm.heap.alloc_instance(BITMAP.into(), vec![])?;
        self.charged.set(self.bytes() + image.pixels.len() * 4);
        let pixels = Rc::new(Pixels {
            width: image.width,
            height: image.height,
            values: image.pixels,
            charged: self.charged.clone(),
        });
        self.objects.borrow_mut().insert(
            r,
            Bitmap {
                width: tw,
                height: th,
                pixels: Some(pixels),
                // API26 nativeDecode creates with density -1 (the display
                // default). nodpi suppresses scaling; it does not overwrite
                // that default with Bitmap.DENSITY_NONE.
                density,
                alpha: image.has_alpha,
            },
        );
        Ok(Value::Obj(r))
    }
    pub(crate) fn call_static(
        &self,
        android: &Android,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if class != FACTORY || name != "decodeResource" {
            return Ok(None);
        }
        if !matches!(sig,"(Landroid/content/res/Resources;I)Landroid/graphics/Bitmap;"|"(Landroid/content/res/Resources;ILandroid/graphics/BitmapFactory$Options;)Landroid/graphics/Bitmap;") {return Ok(None)}
        let Some(resources) = args[0].as_ref()? else {
            // API26 decodeResource catches the resource-open exception.
            return Ok(Some(Value::Null));
        };
        if android.state.borrow().resources != Some(resources) {
            return Err(error(
                "INVALID_RESOURCES",
                "BitmapFactory resources identity",
            ));
        }
        if args.len() == 3 && args[2].as_ref()?.is_some() {
            return Err(unavailable("BitmapFactory non-null Options"));
        }
        let density = target_density(android);
        let Some((path, source)) = resource(&android.apk, int(args, 1)? as u32, density, 0)? else {
            return Ok(Some(Value::Null));
        };
        let Some(entry) = android.apk.zip.find(&path) else {
            return Ok(Some(Value::Null));
        };
        if entry.uncompressed_size > png::MAX_INPUT as u64 {
            return Err(error(
                "BITMAP_MEMORY_BUDGET",
                "encoded resource exceeds 4 MiB",
            ));
        }
        let data = android
            .apk
            .zip
            .read(&path)
            .map_err(|e| error(&e.code, e.cause))?;
        self.decode(vm, &data, source, density).map(Some)
    }
    pub(crate) fn call(
        &self,
        vm: &mut Engine,
        class: &str,
        name: &str,
        sig: &str,
        r: ObjRef,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let actual = vm.heap.class_of(r)?.to_string();
        let class = if vm.cp.is_subtype(&actual, RECTF) {
            RECTF
        } else if vm.cp.is_subtype(&actual, RECT) {
            RECT
        } else if vm.cp.is_subtype(&actual, OPTIONS) {
            OPTIONS
        } else {
            class
        };
        if matches!(class, RECT | RECTF) {
            return match (name, sig) {
                ("<init>", "()V") => Ok(Some(Value::Null)),
                ("<init>" | "set", "(FFFF)V" | "(IIII)V")
                    if (class == RECTF) == sig.starts_with("(F") =>
                {
                    for (field, value) in ["left", "top", "right", "bottom"].into_iter().zip(args) {
                        write(vm, r, field, value.clone())?;
                    }
                    Ok(Some(Value::Null))
                }
                ("width" | "height", "()F" | "()I") if (class == RECTF) == sig.ends_with('F') => {
                    let (a, b) = if name == "width" {
                        ("right", "left")
                    } else {
                        ("bottom", "top")
                    };
                    let (a, b) = (vm.heap.get_field(r, a)?, vm.heap.get_field(r, b)?);
                    Ok(Some(if class == RECTF {
                        Value::Float(a.as_float()? - b.as_float()?)
                    } else {
                        Value::Int(a.as_int()?.wrapping_sub(b.as_int()?))
                    }))
                }
                _ => Ok(None),
            };
        }
        if class == OPTIONS && name == "<init>" && sig == "()V" {
            write(vm, r, "inScaled", Value::Int(1))?;
            write(vm, r, "inPremultiplied", Value::Int(1))?;
            return Ok(Some(Value::Null));
        }
        if class != BITMAP {
            return Ok(None);
        }
        let mut objects = self.objects.borrow_mut();
        let bitmap = objects.get_mut(&r).ok_or_else(missing)?;
        let value = match (name, sig) {
            ("isRecycled", "()Z") => Value::Int(i32::from(bitmap.pixels.is_none())),
            ("recycle", "()V") => {
                bitmap.pixels = None;
                Value::Null
            }
            ("getWidth", "()I") => Value::Int(bitmap.width as i32),
            ("getHeight", "()I") => Value::Int(bitmap.height as i32),
            ("getDensity", "()I") => Value::Int(bitmap.density),
            ("setDensity", "(I)V") => {
                let density = int(args, 0)?;
                bitmap.density = density;
                Value::Null
            }
            ("isMutable", "()Z") => Value::Int(0),
            ("hasAlpha", "()Z") => Value::Int(i32::from(bitmap.alpha)),
            ("getByteCount" | "getAllocationByteCount", "()I") => {
                Value::Int(if bitmap.pixels.is_some() {
                    (bitmap.width * bitmap.height * 4) as i32
                } else {
                    0
                })
            }
            ("getRowBytes", "()I") => Value::Int((bitmap.width * 4) as i32),
            ("getPixel", "(II)I") => {
                let pixels = bitmap.pixels.as_ref().ok_or_else(recycled)?;
                let (x, y) = (int(args, 0)?, int(args, 1)?);
                if x < 0 || y < 0 || x as usize >= bitmap.width || y as usize >= bitmap.height {
                    return Err(illegal("bitmap pixel outside bounds"));
                }
                Value::Int(pixels.values[y as usize * bitmap.width + x as usize] as i32)
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }
    pub(crate) fn draw(
        &self,
        android: &Android,
        vm: &Engine,
        name: &str,
        sig: &str,
        args: &[Value],
    ) -> Result<Option<BitmapDraw>> {
        if name != "drawBitmap" {
            return Ok(None);
        }
        let rectf=sig=="(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/RectF;Landroid/graphics/Paint;)V";
        let rect=sig=="(Landroid/graphics/Bitmap;Landroid/graphics/Rect;Landroid/graphics/Rect;Landroid/graphics/Paint;)V";
        let position = sig == "(Landroid/graphics/Bitmap;FFLandroid/graphics/Paint;)V";
        if !rectf && !rect && !position {
            return Ok(None);
        }
        let objects = self.objects.borrow();
        let bitmap = objects.get(&obj(&args[0])?).ok_or_else(missing)?;
        let pixels = bitmap.pixels.as_ref().ok_or_else(recycled)?.clone();
        let source = if position || args[1].as_ref()?.is_none() {
            [0, 0, pixels.width as i32, pixels.height as i32]
        } else {
            let r = obj(&args[1])?;
            let mut source = [0; 4];
            for (i, field) in ["left", "top", "right", "bottom"].iter().enumerate() {
                source[i] = vm.heap.get_field(r, field)?.as_int()?;
            }
            source
        };
        if source[0] < 0
            || source[1] < 0
            || source[2] > pixels.width as i32
            || source[3] > pixels.height as i32
            || source[2] < source[0]
            || source[3] < source[1]
        {
            return Err(illegal("bitmap source rectangle outside bounds"));
        }
        let destination = if position {
            let (x, y) = (args[1].as_float()?, args[2].as_float()?);
            let scale = if bitmap.density == 0 {
                1.
            } else {
                target_density(android) as f32 / bitmap.density as f32
            };
            [
                x,
                y,
                x + pixels.width as f32 * scale,
                y + pixels.height as f32 * scale,
            ]
        } else {
            let r = obj(&args[2])?;
            let mut destination = [0.; 4];
            for (i, field) in ["left", "top", "right", "bottom"].iter().enumerate() {
                let value = vm.heap.get_field(r, field)?;
                destination[i] = if rectf {
                    value.as_float()?
                } else {
                    value.as_int()? as f32
                };
            }
            destination
        };
        if destination
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err(illegal("non-finite or out-of-range bitmap destination"));
        }
        if args[3].as_ref()?.is_some() {
            // The current Paint adapter does not expose filter/shader/xfermode
            // state. Silently accepting it would misrepresent those effects.
            return Err(unavailable("Canvas.drawBitmap non-null Paint effects"));
        }
        Ok(Some(BitmapDraw {
            pixels,
            source,
            destination,
            alpha: 255,
        }))
    }
}
fn resize(source: &[u32], width: usize, height: usize, tw: usize, th: usize) -> Vec<u32> {
    // Bilinear density scaling in premultiplied alpha, returning straight ARGB
    // for getPixel and the software SRC_OVER raster adapter.
    let mut out = vec![0; tw * th];
    for y in 0..th {
        for x in 0..tw {
            let fx = ((x as f32 + 0.5) * width as f32 / tw as f32 - 0.5)
                .max(0.)
                .min((width - 1) as f32);
            let fy = ((y as f32 + 0.5) * height as f32 / th as f32 - 0.5)
                .max(0.)
                .min((height - 1) as f32);
            let (ix, iy) = (libm::floorf(fx) as usize, libm::floorf(fy) as usize);
            let (dx, dy) = (fx - ix as f32, fy - iy as f32);
            let samples = [
                (source[iy * width + ix], (1. - dx) * (1. - dy)),
                (source[iy * width + (ix + 1).min(width - 1)], dx * (1. - dy)),
                (
                    source[(iy + 1).min(height - 1) * width + ix],
                    (1. - dx) * dy,
                ),
                (
                    source[(iy + 1).min(height - 1) * width + (ix + 1).min(width - 1)],
                    dx * dy,
                ),
            ];
            let alpha = samples
                .iter()
                .map(|(p, w)| (p >> 24) as f32 * w)
                .sum::<f32>();
            let channel = |shift: u32| -> u32 {
                if alpha == 0. {
                    0
                } else {
                    libm::roundf(
                        samples
                            .iter()
                            .map(|(p, w)| ((p >> shift) & 255u32) as f32 * (p >> 24) as f32 * w)
                            .sum::<f32>()
                            / alpha,
                    )
                    .clamp(0., 255.) as u32
                }
            };
            out[y * tw + x] = (libm::roundf(alpha) as u32) << 24
                | channel(16) << 16
                | channel(8) << 8
                | channel(0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recorded_draw_lease_keeps_pixel_allocation_charged_after_java_collection() {
        let layer = Bitmaps::default();
        layer.charged.set(4);
        let pixels = Rc::new(Pixels {
            width: 1,
            height: 1,
            values: vec![0xff35a968],
            charged: layer.charged.clone(),
        });
        layer.objects.borrow_mut().insert(
            7,
            Bitmap {
                pixels: Some(pixels.clone()),
                width: 1,
                height: 1,
                density: 0,
                alpha: false,
            },
        );
        let draw = BitmapDraw {
            pixels,
            source: [0, 0, 1, 1],
            destination: [0., 0., 2., 2.],
            alpha: 255,
        };
        layer.collect(&[]);
        assert!(layer.objects.borrow().is_empty());
        assert_eq!(layer.bytes(), 4);
        let mut frame = Frame::new(2, 2);
        draw.render(&mut frame, 0, 0, 2, 2);
        assert_eq!(frame.pixels, vec![0xff35a968; 4]);
        drop(draw);
        assert_eq!(layer.bytes(), 0);
    }
    #[test]
    fn density_order_matches_android_downscale_bias_and_exact_bucket() {
        assert!(better_density(320, 160, 320));
        assert!(better_density(320, 160, 240));
        assert!(!better_density(480, 160, 170));
        assert!(!better_density(160, 320, 320));
        assert!(better_density(0xfffe, 320, 320));
    }
    #[test]
    fn density_scaling_filters_premultiplied_alpha_without_transparent_color_halo() {
        assert_eq!(
            resize(&[0xffff0000, 0x000000ff], 2, 1, 3, 1),
            vec![0xffff0000, 0x80ff0000, 0]
        );
    }
}
