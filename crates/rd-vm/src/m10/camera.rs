//! `MEDIA-04`: Virtual Camera2 pipeline (`CameraManager`, `CameraCharacteristics`,
//! `CameraDevice`, `CameraCaptureSession`, `CaptureRequest`, `TotalCaptureResult`,
//! `ImageReader`, `Image`, `Image$Plane`, `Surface`) and `java.nio.ByteBuffer`.

use super::{
    alloc, alloc_with, array_len, bool_arg, byte_array, call_dex, check_perm, get_field,
    get_int_field, get_long_field, get_str_field, illegal, illegal_state, int_arg, obj_arg,
    obj_array, put_field, req_str_arg, str_val,
};
use crate::{Engine, ObjRef, Value, VmExit};
use std::collections::BTreeMap;

const CAMERA_MANAGER: &str = "Landroid/hardware/camera2/CameraManager;";
const CAMERA_CHARS: &str = "Landroid/hardware/camera2/CameraCharacteristics;";
const CAMERA_CHARS_KEY: &str = "Landroid/hardware/camera2/CameraCharacteristics$Key;";
const STREAM_CONFIG_MAP: &str = "Landroid/hardware/camera2/params/StreamConfigurationMap;";
const SESSION_CONFIG: &str = "Landroid/hardware/camera2/params/SessionConfiguration;";
const OUTPUT_CONFIG: &str = "Landroid/hardware/camera2/params/OutputConfiguration;";
const CAMERA_DEVICE: &str = "Landroid/hardware/camera2/CameraDevice;";
const CAPTURE_SESSION: &str = "Landroid/hardware/camera2/CameraCaptureSession;";
const CAPTURE_REQUEST: &str = "Landroid/hardware/camera2/CaptureRequest;";
const CAPTURE_REQUEST_KEY: &str = "Landroid/hardware/camera2/CaptureRequest$Key;";
const CAPTURE_REQUEST_BUILDER: &str = "Landroid/hardware/camera2/CaptureRequest$Builder;";
const CAPTURE_RESULT: &str = "Landroid/hardware/camera2/CaptureResult;";
const CAPTURE_RESULT_KEY: &str = "Landroid/hardware/camera2/CaptureResult$Key;";
const TOTAL_CAPTURE_RESULT: &str = "Landroid/hardware/camera2/TotalCaptureResult;";
const IMAGE_READER: &str = "Landroid/media/ImageReader;";
const IMAGE: &str = "Landroid/media/Image;";
const IMAGE_PLANE: &str = "Landroid/media/Image$Plane;";
const IMAGE_FORMAT: &str = "Landroid/graphics/ImageFormat;";
const PIXEL_FORMAT: &str = "Landroid/graphics/PixelFormat;";
const SIZE_CLS: &str = "Landroid/util/Size;";
const SURFACE: &str = "Landroid/view/Surface;";
const BYTE_BUFFER: &str = "Ljava/nio/ByteBuffer;";
const NIO_BUFFER: &str = "Ljava/nio/Buffer;";
const BYTE_ORDER: &str = "Ljava/nio/ByteOrder;";

#[derive(Debug, Clone)]
pub struct VirtualCamera {
    pub id: String,
    pub lens_facing: i32, // 0 = FRONT, 1 = BACK, 2 = EXTERNAL
    pub sensor_orientation: i32,
    pub flash_available: bool,
    pub torch_enabled: bool,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
pub struct CameraFrameSpec {
    pub width: i32,
    pub height: i32,
    pub format: i32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CameraSessionState {
    pub device: ObjRef,
    pub camera_id: String,
    pub surfaces: Vec<ObjRef>,
    pub repeating: bool,
    pub closed: bool,
    pub sequence_id: i32,
}

#[derive(Debug, Clone)]
pub struct ImageReaderState {
    pub width: i32,
    pub height: i32,
    pub format: i32,
    pub max_images: i32,
    pub surface: ObjRef,
    pub listener: Option<ObjRef>,
    pub queue: Vec<ObjRef>,
    pub closed: bool,
}

pub fn default_cameras() -> BTreeMap<String, VirtualCamera> {
    let mut m = BTreeMap::new();
    m.insert(
        "0".into(),
        VirtualCamera {
            id: "0".into(),
            lens_facing: 1, // LENS_FACING_BACK
            sensor_orientation: 90,
            flash_available: true,
            torch_enabled: false,
            width: 1280,
            height: 720,
        },
    );
    m.insert(
        "1".into(),
        VirtualCamera {
            id: "1".into(),
            lens_facing: 0, // LENS_FACING_FRONT
            sensor_orientation: 270,
            flash_available: false,
            torch_enabled: false,
            width: 1280,
            height: 720,
        },
    );
    m
}

pub(crate) fn new_byte_buffer_from_bytes(vm: &mut Engine, data: &[u8]) -> Result<ObjRef, VmExit> {
    let arr = byte_array(vm, data)?;
    let len = data.len() as i32;
    alloc_with(
        vm,
        BYTE_BUFFER,
        vec![
            ("rd_buf".into(), Value::Obj(arr)),
            ("rd_pos".into(), Value::Int(0)),
            ("rd_limit".into(), Value::Int(len)),
            ("rd_cap".into(), Value::Int(len)),
        ],
    )
}

fn synthetic_frame_bytes(
    camera_id: &str,
    width: i32,
    height: i32,
    format: i32,
    seq: i32,
) -> Vec<u8> {
    let w = width.clamp(1, 1920) as usize;
    let h = height.clamp(1, 1080) as usize;
    if format == 256 {
        // ImageFormat.JPEG (0x100): valid minimal JFIF/JPEG header + deterministic payload + EOI
        let mut jpg = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
        ];
        let tag = format!("RD-CAM{camera_id}-{w}x{h}-#{seq}");
        jpg.extend_from_slice(tag.as_bytes());
        jpg.extend_from_slice(&[0xFF, 0xD9]);
        return jpg;
    }
    // RGBA_8888 (1) or YUV_420_888 (35): cap synthetic buffer for large preview sizes so VM heap stays fast
    let capped_pixels = (w * h).min(16_384);
    let bpp = if format == 1 { 4 } else { 1 };
    let total = (capped_pixels * bpp).max(64);
    let seed = camera_id
        .bytes()
        .next()
        .unwrap_or(b'0')
        .wrapping_add(seq as u8);
    let mut out = Vec::with_capacity(total);
    for i in 0..total {
        out.push(seed.wrapping_add((i & 0xFF) as u8));
    }
    out
}

pub(crate) fn enqueue_frame_to_surface(
    vm: &mut Engine,
    surface: ObjRef,
    width: i32,
    height: i32,
    format: i32,
    bytes: &[u8],
) -> Result<bool, VmExit> {
    let Some(reader_ref) = vm.fw.vdev.surface_to_reader.get(&surface).copied() else {
        return Ok(false);
    };
    let Some(reader_meta) = vm.fw.vdev.image_readers.get(&reader_ref).cloned() else {
        return Ok(false);
    };
    if reader_meta.closed {
        return Ok(false);
    }
    let w = if width > 0 { width } else { reader_meta.width };
    let h = if height > 0 {
        height
    } else {
        reader_meta.height
    };
    let fmt = if format > 0 {
        format
    } else {
        reader_meta.format
    };
    let ts = vm.fw.vdev.next_timestamp_ns();

    let bb = new_byte_buffer_from_bytes(vm, bytes)?;
    let pixel_stride = if fmt == 1 { 4 } else { 1 };
    let row_stride = w.saturating_mul(pixel_stride);
    let plane = alloc_with(
        vm,
        IMAGE_PLANE,
        vec![
            ("rd_buffer".into(), Value::Obj(bb)),
            ("rd_pixel_stride".into(), Value::Int(pixel_stride)),
            ("rd_row_stride".into(), Value::Int(row_stride)),
        ],
    )?;
    let planes_arr = obj_array(vm, "[Landroid/media/Image$Plane;", vec![Value::Obj(plane)])?;
    let img = alloc_with(
        vm,
        IMAGE,
        vec![
            ("rd_width".into(), Value::Int(w)),
            ("rd_height".into(), Value::Int(h)),
            ("rd_format".into(), Value::Int(fmt)),
            ("rd_timestamp".into(), Value::Long(ts)),
            ("rd_planes".into(), planes_arr),
            ("rd_closed".into(), Value::Int(0)),
        ],
    )?;

    let listener = if let Some(st) = vm.fw.vdev.image_readers.get_mut(&reader_ref) {
        let max_q = (st.max_images.max(1)) as usize;
        if st.queue.len() >= max_q {
            st.queue.remove(0);
        }
        st.queue.push(img);
        st.listener
    } else {
        None
    };

    if let Some(l) = listener {
        let _ = call_dex(
            vm,
            l,
            "onImageAvailable",
            "(Landroid/media/ImageReader;)V",
            vec![Value::Obj(reader_ref)],
        )?;
    }
    Ok(true)
}

fn alloc_key_obj(vm: &mut Engine, cls: &str, key_name: &str) -> Result<Value, VmExit> {
    let s = str_val(vm, key_name)?;
    let r = alloc_with(vm, cls, vec![("rd_key".into(), s)])?;
    Ok(Value::Obj(r))
}

fn boxed_int(vm: &mut Engine, v: i32) -> Result<Value, VmExit> {
    let r = alloc_with(
        vm,
        "Ljava/lang/Integer;",
        vec![("value".into(), Value::Int(v))],
    )?;
    Ok(Value::Obj(r))
}

fn boxed_bool(vm: &mut Engine, v: bool) -> Result<Value, VmExit> {
    let r = alloc_with(
        vm,
        "Ljava/lang/Boolean;",
        vec![("value".into(), Value::Int(i32::from(v)))],
    )?;
    Ok(Value::Obj(r))
}

pub(super) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (CAMERA_CHARS, "LENS_FACING")
        | (CAMERA_CHARS, "SENSOR_ORIENTATION")
        | (CAMERA_CHARS, "FLASH_INFO_AVAILABLE")
        | (CAMERA_CHARS, "INFO_SUPPORTED_HARDWARE_LEVEL")
        | (CAMERA_CHARS, "SCALER_STREAM_CONFIGURATION_MAP")
        | (CAMERA_CHARS, "SENSOR_INFO_ACTIVE_ARRAY_SIZE") => {
            alloc_key_obj(vm, CAMERA_CHARS_KEY, name).map(Some)
        }
        (CAMERA_CHARS, "LENS_FACING_FRONT") => Ok(Some(Value::Int(0))),
        (CAMERA_CHARS, "LENS_FACING_BACK") => Ok(Some(Value::Int(1))),
        (CAMERA_CHARS, "LENS_FACING_EXTERNAL") => Ok(Some(Value::Int(2))),
        (CAMERA_CHARS, "INFO_SUPPORTED_HARDWARE_LEVEL_LEGACY") => Ok(Some(Value::Int(2))),
        (CAMERA_CHARS, "INFO_SUPPORTED_HARDWARE_LEVEL_LIMITED") => Ok(Some(Value::Int(0))),
        (CAMERA_CHARS, "INFO_SUPPORTED_HARDWARE_LEVEL_FULL") => Ok(Some(Value::Int(1))),
        (CAMERA_DEVICE, "TEMPLATE_PREVIEW") => Ok(Some(Value::Int(1))),
        (CAMERA_DEVICE, "TEMPLATE_STILL_CAPTURE") => Ok(Some(Value::Int(2))),
        (CAMERA_DEVICE, "TEMPLATE_RECORD") => Ok(Some(Value::Int(3))),
        (CAPTURE_REQUEST, "CONTROL_AF_MODE")
        | (CAPTURE_REQUEST, "CONTROL_AE_MODE")
        | (CAPTURE_REQUEST, "FLASH_MODE")
        | (CAPTURE_REQUEST, "JPEG_ORIENTATION") => {
            alloc_key_obj(vm, CAPTURE_REQUEST_KEY, name).map(Some)
        }
        (CAPTURE_RESULT, "CONTROL_AF_STATE")
        | (CAPTURE_RESULT, "CONTROL_AE_STATE")
        | (CAPTURE_RESULT, "FLASH_STATE")
        | (TOTAL_CAPTURE_RESULT, "CONTROL_AF_STATE") => {
            alloc_key_obj(vm, CAPTURE_RESULT_KEY, name).map(Some)
        }
        (IMAGE_FORMAT, "JPEG") => Ok(Some(Value::Int(256))),
        (IMAGE_FORMAT, "YUV_420_888") => Ok(Some(Value::Int(35))),
        (IMAGE_FORMAT, "NV21") => Ok(Some(Value::Int(17))),
        (PIXEL_FORMAT, "RGBA_8888") => Ok(Some(Value::Int(1))),
        (BYTE_ORDER, "BIG_ENDIAN") | (BYTE_ORDER, "LITTLE_ENDIAN") => {
            let s = str_val(vm, name)?;
            let r = alloc_with(vm, BYTE_ORDER, vec![("rd_order".into(), s)])?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (IMAGE_READER, "newInstance") => {
            let w = int_arg(args, 0);
            let h = int_arg(args, 1);
            let fmt = int_arg(args, 2);
            let max_images = int_arg(args, 3);
            if w <= 0 || h <= 0 || max_images <= 0 {
                return Err(illegal("ImageReader dimensions and maxImages must be > 0"));
            }
            let surface = alloc(vm, SURFACE)?;
            let reader = alloc(vm, IMAGE_READER)?;
            vm.fw.vdev.surface_to_reader.insert(surface, reader);
            vm.fw.vdev.image_readers.insert(
                reader,
                ImageReaderState {
                    width: w,
                    height: h,
                    format: fmt,
                    max_images,
                    surface,
                    listener: None,
                    queue: Vec::new(),
                    closed: false,
                },
            );
            Ok(Some(Value::Obj(reader)))
        }
        (BYTE_BUFFER, "allocate" | "allocateDirect") => {
            let cap = int_arg(args, 0);
            if cap < 0 {
                return Err(illegal("negative ByteBuffer capacity"));
            }
            let arr = vm
                .heap
                .alloc_array_zeroed(crate::heap::ElemKind::Int, cap as usize, "[B")
                .map_err(crate::err::oom_throwable)?;
            let bb = vm
                .heap
                .alloc_instance(
                    BYTE_BUFFER.into(),
                    vec![
                        ("rd_buf".into(), Value::Obj(arr)),
                        ("rd_pos".into(), Value::Int(0)),
                        ("rd_limit".into(), Value::Int(cap)),
                        ("rd_cap".into(), Value::Int(cap)),
                    ],
                )
                .map_err(crate::err::oom_throwable)?;
            Ok(Some(Value::Obj(bb)))
        }
        (BYTE_BUFFER, "wrap") => {
            let Some(arr) = obj_arg(args, 0) else {
                return Err(super::npe("ByteBuffer.wrap null array"));
            };
            let cap_usize = array_len(vm, arr)?;
            let cap = cap_usize.min(i32::MAX as usize) as i32;
            let (pos, limit) = if args.len() >= 3 {
                let off = int_arg(args, 1);
                let len = int_arg(args, 2);
                if off < 0
                    || len < 0
                    || (off as usize) > cap_usize
                    || (len as usize) > cap_usize - (off as usize)
                {
                    return Err(super::error(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "ByteBuffer.wrap bounds",
                    ));
                }
                let end = (off as usize + len as usize) as i32;
                (off, end)
            } else {
                (0, cap)
            };
            let bb = alloc_with(
                vm,
                BYTE_BUFFER,
                vec![
                    ("rd_buf".into(), Value::Obj(arr)),
                    ("rd_pos".into(), Value::Int(pos)),
                    ("rd_limit".into(), Value::Int(limit)),
                    ("rd_cap".into(), Value::Int(cap)),
                ],
            )?;
            Ok(Some(Value::Obj(bb)))
        }
        (BYTE_ORDER, "nativeOrder") => {
            let s = str_val(vm, "LITTLE_ENDIAN")?;
            let r = alloc_with(vm, BYTE_ORDER, vec![("rd_order".into(), s)])?;
            Ok(Some(Value::Obj(r)))
        }
        _ => Ok(None),
    }
}

pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        "Landroid/hardware/camera2/CameraDevice$StateCallback;"
        | "Landroid/hardware/camera2/CameraCaptureSession$StateCallback;"
        | "Landroid/hardware/camera2/CameraCaptureSession$CaptureCallback;"
        | "Landroid/hardware/camera2/CameraManager$AvailabilityCallback;"
        | "Landroid/hardware/camera2/CameraManager$TorchCallback;" => {
            if name == "<init>" {
                return Ok(Some(Value::Null));
            }
            Ok(None)
        }
        CAMERA_MANAGER => match name {
            "getCameraIdList" => {
                let ids: Vec<String> = vm.fw.vdev.cameras.keys().cloned().collect();
                let mut vals = Vec::with_capacity(ids.len());
                for id in ids {
                    vals.push(str_val(vm, id)?);
                }
                Ok(Some(obj_array(vm, "[Ljava/lang/String;", vals)?))
            }
            "getCameraCharacteristics" => {
                let id = req_str_arg(vm, args, 0)?;
                if !vm.fw.vdev.cameras.contains_key(&id) {
                    return Err(illegal(format!("unknown camera id: {id}")));
                }
                let id_v = str_val(vm, id)?;
                let r = alloc_with(vm, CAMERA_CHARS, vec![("rd_cam_id".into(), id_v)])?;
                Ok(Some(Value::Obj(r)))
            }
            "setTorchMode" => {
                let id = req_str_arg(vm, args, 0)?;
                let on = bool_arg(args, 1);
                let Some(cam) = vm.fw.vdev.cameras.get_mut(&id) else {
                    return Err(illegal(format!("unknown camera id: {id}")));
                };
                if !cam.flash_available {
                    return Err(illegal("camera has no flash unit"));
                }
                cam.torch_enabled = on;
                Ok(Some(Value::Null))
            }
            "openCamera" => {
                check_perm(vm, "android.permission.CAMERA")?;
                let id = req_str_arg(vm, args, 0)?;
                if !vm.fw.vdev.cameras.contains_key(&id) {
                    return Err(illegal(format!("unknown camera id: {id}")));
                }
                // (String, StateCallback, Handler) or (String, Executor, StateCallback)
                let cb = if args.len() >= 3 {
                    let c1 = obj_arg(args, 1);
                    let c2 = obj_arg(args, 2);
                    match (c1, c2) {
                        (Some(r1), _)
                            if vm.heap.class_of(r1).ok().unwrap_or("")
                                != "Landroid/os/Handler;" =>
                        {
                            // In (String, StateCallback, Handler), arg1 is StateCallback;
                            // in (String, Executor, StateCallback), arg2 is StateCallback.
                            if let Some(r2) = c2 {
                                let cls2 = vm.heap.class_of(r2).ok().unwrap_or("");
                                if cls2 != "Landroid/os/Handler;" {
                                    Some(r2)
                                } else {
                                    Some(r1)
                                }
                            } else {
                                Some(r1)
                            }
                        }
                        (_, c2_opt) => c2_opt,
                    }
                } else {
                    obj_arg(args, 1)
                };
                let id_v = str_val(vm, id.clone())?;
                let dev = alloc_with(
                    vm,
                    CAMERA_DEVICE,
                    vec![
                        ("rd_cam_id".into(), id_v),
                        ("rd_closed".into(), Value::Int(0)),
                    ],
                )?;
                vm.fw.vdev.camera_devices.insert(dev, id);
                if let Some(cb_ref) = cb {
                    vm.fw.vdev.camera_callbacks.insert(dev, cb_ref);
                    let _ = call_dex(
                        vm,
                        cb_ref,
                        "onOpened",
                        "(Landroid/hardware/camera2/CameraDevice;)V",
                        vec![Value::Obj(dev)],
                    )?;
                }
                Ok(Some(Value::Null))
            }
            "registerAvailabilityCallback"
            | "unregisterAvailabilityCallback"
            | "registerTorchCallback"
            | "unregisterTorchCallback" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        CAMERA_CHARS => match name {
            "get" => {
                let id = get_str_field(vm, recv, "rd_cam_id").unwrap_or_else(|| "0".into());
                let Some(cam) = vm.fw.vdev.cameras.get(&id).cloned() else {
                    return Ok(Some(Value::Null));
                };
                let key_name = obj_arg(args, 0)
                    .and_then(|k| get_str_field(vm, k, "rd_key"))
                    .unwrap_or_default();
                match key_name.as_str() {
                    "LENS_FACING" => Ok(Some(boxed_int(vm, cam.lens_facing)?)),
                    "SENSOR_ORIENTATION" => Ok(Some(boxed_int(vm, cam.sensor_orientation)?)),
                    "FLASH_INFO_AVAILABLE" => Ok(Some(boxed_bool(vm, cam.flash_available)?)),
                    "INFO_SUPPORTED_HARDWARE_LEVEL" => Ok(Some(boxed_int(vm, 1)?)),
                    "SCALER_STREAM_CONFIGURATION_MAP" => {
                        let m = alloc_with(
                            vm,
                            STREAM_CONFIG_MAP,
                            vec![
                                ("rd_w".into(), Value::Int(cam.width)),
                                ("rd_h".into(), Value::Int(cam.height)),
                            ],
                        )?;
                        Ok(Some(Value::Obj(m)))
                    }
                    "SENSOR_INFO_ACTIVE_ARRAY_SIZE" => {
                        let rect = alloc_with(
                            vm,
                            "Landroid/graphics/Rect;",
                            vec![
                                ("left".into(), Value::Int(0)),
                                ("top".into(), Value::Int(0)),
                                ("right".into(), Value::Int(cam.width)),
                                ("bottom".into(), Value::Int(cam.height)),
                            ],
                        )?;
                        Ok(Some(Value::Obj(rect)))
                    }
                    _ => Ok(Some(Value::Null)),
                }
            }
            _ => Ok(None),
        },
        STREAM_CONFIG_MAP => match name {
            "getOutputSizes" => {
                let w = get_int_field(vm, recv, "rd_w", 1280);
                let h = get_int_field(vm, recv, "rd_h", 720);
                let s1 = alloc_with(
                    vm,
                    SIZE_CLS,
                    vec![
                        ("rd_w".into(), Value::Int(w)),
                        ("rd_h".into(), Value::Int(h)),
                    ],
                )?;
                let s2 = alloc_with(
                    vm,
                    SIZE_CLS,
                    vec![
                        ("rd_w".into(), Value::Int(640)),
                        ("rd_h".into(), Value::Int(480)),
                    ],
                )?;
                Ok(Some(obj_array(
                    vm,
                    "[Landroid/util/Size;",
                    vec![Value::Obj(s1), Value::Obj(s2)],
                )?))
            }
            "getOutputFormats" => {
                let r = vm.heap.alloc_array(crate::heap::ElemKind::Int, 3, "[I")?;
                *vm.heap.array_elems_mut(r)? = vec![Value::Int(256), Value::Int(35), Value::Int(1)];
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        },
        SIZE_CLS => match name {
            "<init>" => {
                put_field(vm, recv, "rd_w", Value::Int(int_arg(args, 0)))?;
                put_field(vm, recv, "rd_h", Value::Int(int_arg(args, 1)))?;
                Ok(Some(Value::Null))
            }
            "getWidth" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_w", 0)))),
            "getHeight" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_h", 0)))),
            "toString" => {
                let w = get_int_field(vm, recv, "rd_w", 0);
                let h = get_int_field(vm, recv, "rd_h", 0);
                Ok(Some(str_val(vm, format!("{w}x{h}"))?))
            }
            _ => Ok(None),
        },
        CAMERA_DEVICE => match name {
            "getId" => Ok(Some(
                get_field(vm, recv, "rd_cam_id").unwrap_or(Value::Null),
            )),
            "createCaptureRequest" => {
                if get_int_field(vm, recv, "rd_closed", 0) != 0 {
                    return Err(illegal_state("CameraDevice is closed"));
                }
                let tmpl = int_arg(args, 0);
                let targets = super::array_list(vm, Vec::new())?;
                let b = alloc_with(
                    vm,
                    CAPTURE_REQUEST_BUILDER,
                    vec![
                        ("rd_template".into(), Value::Int(tmpl)),
                        ("rd_targets".into(), targets),
                    ],
                )?;
                Ok(Some(Value::Obj(b)))
            }
            "createCaptureSession" => {
                if get_int_field(vm, recv, "rd_closed", 0) != 0 {
                    return Err(illegal_state("CameraDevice is closed"));
                }
                let cam_id = get_str_field(vm, recv, "rd_cam_id").unwrap_or_else(|| "0".into());
                let mut surfaces = Vec::new();
                let mut cb = None;
                if let Some(first) = obj_arg(args, 0) {
                    let cls0 = vm.heap.class_of(first).ok().unwrap_or("");
                    if cls0 == SESSION_CONFIG {
                        if let Some(Value::Obj(list_ref)) = get_field(vm, first, "rd_outputs") {
                            if let Some(crate::framework::HostObj::List { elems }) =
                                vm.fw.objects.get(&list_ref).cloned()
                            {
                                for e in elems {
                                    if let Value::Obj(oc) = e {
                                        if let Some(Value::Obj(surf)) =
                                            get_field(vm, oc, "rd_surface")
                                        {
                                            surfaces.push(surf);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(Value::Obj(c)) = get_field(vm, first, "rd_callback") {
                            cb = Some(c);
                        }
                    } else {
                        if let Some(crate::framework::HostObj::List { elems }) =
                            vm.fw.objects.get(&first).cloned()
                        {
                            for e in elems {
                                if let Value::Obj(surf) = e {
                                    surfaces.push(surf);
                                }
                            }
                        }
                        cb = obj_arg(args, 1);
                    }
                }
                let session = alloc(vm, CAPTURE_SESSION)?;
                vm.fw.vdev.camera_sessions.insert(
                    session,
                    CameraSessionState {
                        device: recv,
                        camera_id: cam_id,
                        surfaces,
                        repeating: false,
                        closed: false,
                        sequence_id: 0,
                    },
                );
                if let Some(cb_ref) = cb {
                    let _ = call_dex(
                        vm,
                        cb_ref,
                        "onConfigured",
                        "(Landroid/hardware/camera2/CameraCaptureSession;)V",
                        vec![Value::Obj(session)],
                    )?;
                }
                Ok(Some(Value::Null))
            }
            "close" => {
                put_field(vm, recv, "rd_closed", Value::Int(1))?;
                vm.fw.vdev.camera_devices.remove(&recv);
                let closed_sessions: Vec<ObjRef> = vm
                    .fw
                    .vdev
                    .camera_sessions
                    .iter()
                    .filter(|(_, s)| s.device == recv)
                    .map(|(k, _)| *k)
                    .collect();
                for s in closed_sessions {
                    vm.fw.vdev.camera_sessions.remove(&s);
                }
                if let Some(cb) = vm.fw.vdev.camera_callbacks.remove(&recv) {
                    let _ = call_dex(
                        vm,
                        cb,
                        "onClosed",
                        "(Landroid/hardware/camera2/CameraDevice;)V",
                        vec![Value::Obj(recv)],
                    )?;
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        OUTPUT_CONFIG => match name {
            "<init>" => {
                let s = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_surface", s)?;
                Ok(Some(Value::Null))
            }
            "getSurface" => Ok(Some(
                get_field(vm, recv, "rd_surface").unwrap_or(Value::Null),
            )),
            _ => Ok(None),
        },
        SESSION_CONFIG => match name {
            "<init>" => {
                // (int sessionType, List<OutputConfiguration> outputs, Executor, StateCallback)
                let outs = args.get(1).cloned().unwrap_or(Value::Null);
                let cb = args.get(3).cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_outputs", outs)?;
                put_field(vm, recv, "rd_callback", cb)?;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        CAPTURE_REQUEST_BUILDER => match name {
            "addTarget" => {
                if let (Some(Value::Obj(list_ref)), Some(surf)) =
                    (get_field(vm, recv, "rd_targets"), obj_arg(args, 0))
                {
                    if let Some(crate::framework::HostObj::List { elems }) =
                        vm.fw.objects.get_mut(&list_ref)
                    {
                        if !elems.contains(&Value::Obj(surf)) {
                            elems.push(Value::Obj(surf));
                        }
                    }
                }
                Ok(Some(Value::Null))
            }
            "removeTarget" => {
                if let (Some(Value::Obj(list_ref)), Some(surf)) =
                    (get_field(vm, recv, "rd_targets"), obj_arg(args, 0))
                {
                    if let Some(crate::framework::HostObj::List { elems }) =
                        vm.fw.objects.get_mut(&list_ref)
                    {
                        elems.retain(|v| *v != Value::Obj(surf));
                    }
                }
                Ok(Some(Value::Null))
            }
            "set" => {
                if let Some(k) = obj_arg(args, 0) {
                    if let Some(kname) = get_str_field(vm, k, "rd_key") {
                        let v = args.get(1).cloned().unwrap_or(Value::Null);
                        put_field(vm, recv, &format!("rd_kv_{kname}"), v)?;
                    }
                }
                Ok(Some(Value::Null))
            }
            "get" => {
                if let Some(k) = obj_arg(args, 0) {
                    if let Some(kname) = get_str_field(vm, k, "rd_key") {
                        return Ok(Some(
                            get_field(vm, recv, &format!("rd_kv_{kname}")).unwrap_or(Value::Null),
                        ));
                    }
                }
                Ok(Some(Value::Null))
            }
            "build" => {
                let targets = get_field(vm, recv, "rd_targets").unwrap_or(Value::Null);
                let tmpl = get_int_field(vm, recv, "rd_template", 1);
                let req = alloc_with(
                    vm,
                    CAPTURE_REQUEST,
                    vec![
                        ("rd_template".into(), Value::Int(tmpl)),
                        ("rd_targets".into(), targets),
                    ],
                )?;
                Ok(Some(Value::Obj(req)))
            }
            _ => Ok(None),
        },
        CAPTURE_REQUEST => match name {
            "get" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        CAPTURE_SESSION => match name {
            "getDevice" => {
                let dev = vm
                    .fw
                    .vdev
                    .camera_sessions
                    .get(&recv)
                    .map(|s| Value::Obj(s.device))
                    .unwrap_or(Value::Null);
                Ok(Some(dev))
            }
            "capture"
            | "setRepeatingRequest"
            | "captureSingleRequest"
            | "setSingleRepeatingRequest" => {
                let repeating = name.contains("Repeating");
                let Some(req) = obj_arg(args, 0) else {
                    return Err(illegal("CaptureRequest is null"));
                };
                let (cam_id, seq, default_surfaces) = {
                    let Some(sess) = vm.fw.vdev.camera_sessions.get_mut(&recv) else {
                        return Err(illegal_state("CameraCaptureSession is closed"));
                    };
                    if sess.closed {
                        return Err(illegal_state("CameraCaptureSession is closed"));
                    }
                    sess.repeating = repeating;
                    sess.sequence_id += 1;
                    (
                        sess.camera_id.clone(),
                        sess.sequence_id,
                        sess.surfaces.clone(),
                    )
                };
                let mut targets = Vec::new();
                if let Some(Value::Obj(list_ref)) = get_field(vm, req, "rd_targets") {
                    if let Some(crate::framework::HostObj::List { elems }) =
                        vm.fw.objects.get(&list_ref).cloned()
                    {
                        for e in elems {
                            if let Value::Obj(s) = e {
                                targets.push(s);
                            }
                        }
                    }
                }
                if targets.is_empty() {
                    targets = default_surfaces;
                }
                let injected = vm.fw.vdev.injected_camera_frames.get(&cam_id).cloned();
                let cam_dims = vm
                    .fw
                    .vdev
                    .cameras
                    .get(&cam_id)
                    .map(|c| (c.width, c.height))
                    .unwrap_or((1280, 720));

                for surf in targets {
                    let reader_fmt = vm
                        .fw
                        .vdev
                        .surface_to_reader
                        .get(&surf)
                        .and_then(|r| vm.fw.vdev.image_readers.get(r))
                        .map(|ir| (ir.width, ir.height, ir.format));
                    let (w, h, fmt, data) = if let Some(spec) = &injected {
                        (spec.width, spec.height, spec.format, spec.bytes.clone())
                    } else {
                        let (rw, rh, rfmt) = reader_fmt.unwrap_or((cam_dims.0, cam_dims.1, 256));
                        (
                            rw,
                            rh,
                            rfmt,
                            synthetic_frame_bytes(&cam_id, rw, rh, rfmt, seq),
                        )
                    };
                    let _ = enqueue_frame_to_surface(vm, surf, w, h, fmt, &data)?;
                }

                // Invoke CaptureCallback.onCaptureCompleted if non-null
                let cap_cb = if name.contains("Single") {
                    obj_arg(args, 2)
                } else {
                    obj_arg(args, 1)
                };
                if let Some(cb) = cap_cb {
                    let total_res = alloc_with(
                        vm,
                        TOTAL_CAPTURE_RESULT,
                        vec![("rd_seq".into(), Value::Int(seq))],
                    )?;
                    let _ = call_dex(
                        vm,
                        cb,
                        "onCaptureCompleted",
                        "(Landroid/hardware/camera2/CameraCaptureSession;Landroid/hardware/camera2/CaptureRequest;Landroid/hardware/camera2/TotalCaptureResult;)V",
                        vec![Value::Obj(recv), Value::Obj(req), Value::Obj(total_res)],
                    )?;
                }
                Ok(Some(Value::Int(seq)))
            }
            "stopRepeating" | "abortCaptures" => {
                if let Some(sess) = vm.fw.vdev.camera_sessions.get_mut(&recv) {
                    sess.repeating = false;
                }
                Ok(Some(Value::Null))
            }
            "close" => {
                vm.fw.vdev.camera_sessions.remove(&recv);
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        TOTAL_CAPTURE_RESULT | CAPTURE_RESULT => match name {
            "getFrameNumber" => Ok(Some(Value::Long(i64::from(get_int_field(
                vm, recv, "rd_seq", 1,
            ))))),
            "getSequenceId" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_seq", 1)))),
            "get" => Ok(Some(boxed_int(vm, 4)?)), // CONTROL_AF_STATE_FOCUSED_LOCKED
            _ => Ok(None),
        },
        IMAGE_READER => match name {
            "getWidth" => Ok(Some(Value::Int(
                vm.fw
                    .vdev
                    .image_readers
                    .get(&recv)
                    .map(|r| r.width)
                    .unwrap_or(0),
            ))),
            "getHeight" => Ok(Some(Value::Int(
                vm.fw
                    .vdev
                    .image_readers
                    .get(&recv)
                    .map(|r| r.height)
                    .unwrap_or(0),
            ))),
            "getImageFormat" => Ok(Some(Value::Int(
                vm.fw
                    .vdev
                    .image_readers
                    .get(&recv)
                    .map(|r| r.format)
                    .unwrap_or(0),
            ))),
            "getMaxImages" => Ok(Some(Value::Int(
                vm.fw
                    .vdev
                    .image_readers
                    .get(&recv)
                    .map(|r| r.max_images)
                    .unwrap_or(0),
            ))),
            "getSurface" => Ok(Some(
                vm.fw
                    .vdev
                    .image_readers
                    .get(&recv)
                    .map(|r| Value::Obj(r.surface))
                    .unwrap_or(Value::Null),
            )),
            "setOnImageAvailableListener" => {
                let l = obj_arg(args, 0);
                if let Some(st) = vm.fw.vdev.image_readers.get_mut(&recv) {
                    st.listener = l;
                }
                Ok(Some(Value::Null))
            }
            "acquireLatestImage" => {
                let img = if let Some(st) = vm.fw.vdev.image_readers.get_mut(&recv) {
                    let last = st.queue.pop();
                    st.queue.clear();
                    last
                } else {
                    None
                };
                Ok(Some(img.map(Value::Obj).unwrap_or(Value::Null)))
            }
            "acquireNextImage" => {
                let img = if let Some(st) = vm.fw.vdev.image_readers.get_mut(&recv) {
                    if st.queue.is_empty() {
                        None
                    } else {
                        Some(st.queue.remove(0))
                    }
                } else {
                    None
                };
                Ok(Some(img.map(Value::Obj).unwrap_or(Value::Null)))
            }
            "close" => {
                if let Some(st) = vm.fw.vdev.image_readers.remove(&recv) {
                    vm.fw.vdev.surface_to_reader.remove(&st.surface);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        IMAGE => match name {
            "getWidth" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_width", 0)))),
            "getHeight" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_height", 0)))),
            "getFormat" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_format", 0)))),
            "getTimestamp" => Ok(Some(Value::Long(get_long_field(
                vm,
                recv,
                "rd_timestamp",
                0,
            )))),
            "getPlanes" => Ok(Some(
                get_field(vm, recv, "rd_planes").unwrap_or(Value::Null),
            )),
            "close" => {
                put_field(vm, recv, "rd_closed", Value::Int(1))?;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        IMAGE_PLANE => match name {
            "getBuffer" => Ok(Some(
                get_field(vm, recv, "rd_buffer").unwrap_or(Value::Null),
            )),
            "getPixelStride" => Ok(Some(Value::Int(get_int_field(
                vm,
                recv,
                "rd_pixel_stride",
                1,
            )))),
            "getRowStride" => Ok(Some(Value::Int(get_int_field(
                vm,
                recv,
                "rd_row_stride",
                0,
            )))),
            _ => Ok(None),
        },
        SURFACE => match name {
            "isValid" => Ok(Some(Value::Int(1))),
            "release" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        BYTE_BUFFER | NIO_BUFFER => byte_buffer_instance(vm, name, sig, args, recv),
        _ => Ok(None),
    }
}

fn byte_buffer_instance(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
    recv: ObjRef,
) -> Result<Option<Value>, VmExit> {
    let pos = get_int_field(vm, recv, "rd_pos", 0);
    let limit = get_int_field(vm, recv, "rd_limit", 0);
    let cap = get_int_field(vm, recv, "rd_cap", 0);
    let buf_ref = match get_field(vm, recv, "rd_buf") {
        Some(Value::Obj(r)) => Some(r),
        _ => None,
    };
    match name {
        "capacity" => Ok(Some(Value::Int(cap))),
        "position" => {
            if args.is_empty() {
                Ok(Some(Value::Int(pos)))
            } else {
                let np = int_arg(args, 0);
                if np < 0 || np > limit {
                    return Err(illegal("ByteBuffer.position out of bounds"));
                }
                put_field(vm, recv, "rd_pos", Value::Int(np))?;
                Ok(Some(Value::Obj(recv)))
            }
        }
        "limit" => {
            if args.is_empty() {
                Ok(Some(Value::Int(limit)))
            } else {
                let nl = int_arg(args, 0);
                if nl < 0 || nl > cap {
                    return Err(illegal("ByteBuffer.limit out of bounds"));
                }
                put_field(vm, recv, "rd_limit", Value::Int(nl))?;
                if pos > nl {
                    put_field(vm, recv, "rd_pos", Value::Int(nl))?;
                }
                Ok(Some(Value::Obj(recv)))
            }
        }
        "remaining" => Ok(Some(Value::Int(
            i64::from(limit)
                .saturating_sub(i64::from(pos))
                .clamp(0, i64::from(i32::MAX)) as i32,
        ))),
        "hasRemaining" => Ok(Some(Value::Int(i32::from(pos < limit)))),
        "rewind" => {
            put_field(vm, recv, "rd_pos", Value::Int(0))?;
            Ok(Some(Value::Obj(recv)))
        }
        "clear" => {
            put_field(vm, recv, "rd_pos", Value::Int(0))?;
            put_field(vm, recv, "rd_limit", Value::Int(cap))?;
            Ok(Some(Value::Obj(recv)))
        }
        "flip" => {
            put_field(vm, recv, "rd_limit", Value::Int(pos.max(0)))?;
            put_field(vm, recv, "rd_pos", Value::Int(0))?;
            Ok(Some(Value::Obj(recv)))
        }
        "hasArray" => Ok(Some(Value::Int(i32::from(buf_ref.is_some())))),
        "arrayOffset" => Ok(Some(Value::Int(0))),
        "isDirect" => Ok(Some(Value::Int(0))),
        "order" => {
            if args.is_empty() {
                let s = str_val(vm, "LITTLE_ENDIAN")?;
                let r = alloc_with(vm, BYTE_ORDER, vec![("rd_order".into(), s)])?;
                Ok(Some(Value::Obj(r)))
            } else {
                Ok(Some(Value::Obj(recv)))
            }
        }
        "array" => Ok(Some(buf_ref.map(Value::Obj).unwrap_or(Value::Null))),
        "get" => {
            let Some(arr) = buf_ref else {
                return Err(illegal_state("ByteBuffer has no backing array"));
            };
            if args.is_empty() {
                if pos < 0 || pos >= limit {
                    return Err(super::error(
                        "Ljava/nio/BufferUnderflowException;",
                        "underflow",
                    ));
                }
                let (elems, _) = vm.heap.as_array_elems(arr)?;
                let b = elems
                    .get(pos as usize)
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(0);
                put_field(vm, recv, "rd_pos", Value::Int(pos + 1))?;
                Ok(Some(Value::Int(b)))
            } else if sig.starts_with("([B") || matches!(args.first(), Some(Value::Obj(_))) {
                let Some(dst) = obj_arg(args, 0) else {
                    return Err(super::npe("ByteBuffer.get null array"));
                };
                let dst_cap = array_len(vm, dst)?;
                let (off, len) = if args.len() >= 3 {
                    let off_i = int_arg(args, 1);
                    let len_i = int_arg(args, 2);
                    if off_i < 0
                        || len_i < 0
                        || (off_i as usize) > dst_cap
                        || (len_i as usize) > dst_cap - (off_i as usize)
                    {
                        return Err(super::error(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            "ByteBuffer.get bounds",
                        ));
                    }
                    (off_i as usize, len_i as usize)
                } else {
                    (0usize, dst_cap)
                };
                let rem = if pos >= 0 && limit >= pos {
                    (limit - pos) as usize
                } else {
                    0
                };
                let src_len = array_len(vm, arr)?;
                if pos < 0
                    || len > rem
                    || (pos as usize) > src_len
                    || len > src_len - (pos as usize)
                {
                    return Err(super::error(
                        "Ljava/nio/BufferUnderflowException;",
                        "underflow",
                    ));
                }
                let pos_u = pos as usize;
                let slice_bytes: Vec<i8> = {
                    let (src_elems, _) = vm.heap.as_array_elems(arr)?;
                    src_elems[pos_u..pos_u + len]
                        .iter()
                        .map(|v| v.as_int().unwrap_or(0) as i8)
                        .collect()
                };
                let dst_elems = vm.heap.array_elems_mut(dst)?;
                for (slot, b) in dst_elems[off..off + len].iter_mut().zip(slice_bytes) {
                    *slot = Value::Int(i32::from(b));
                }
                put_field(vm, recv, "rd_pos", Value::Int((pos_u + len) as i32))?;
                Ok(Some(Value::Obj(recv)))
            } else {
                let idx = int_arg(args, 0);
                let src_len = array_len(vm, arr)?;
                if idx < 0 || idx >= limit || (idx as usize) >= src_len {
                    return Err(super::error(
                        "Ljava/lang/IndexOutOfBoundsException;",
                        "ByteBuffer.get(index)",
                    ));
                }
                let (elems, _) = vm.heap.as_array_elems(arr)?;
                let b = elems
                    .get(idx as usize)
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(0);
                Ok(Some(Value::Int(b)))
            }
        }
        "put" => {
            let Some(arr) = buf_ref else {
                return Err(illegal_state("ByteBuffer has no backing array"));
            };
            if sig.starts_with("([B") || matches!(args.first(), Some(Value::Obj(_))) {
                let Some(src) = obj_arg(args, 0) else {
                    return Err(super::npe("ByteBuffer.put null array"));
                };
                let src_len = array_len(vm, src)?;
                let (off, len) = if args.len() >= 3 {
                    let off_i = int_arg(args, 1);
                    let len_i = int_arg(args, 2);
                    if off_i < 0
                        || len_i < 0
                        || (off_i as usize) > src_len
                        || (len_i as usize) > src_len - (off_i as usize)
                    {
                        return Err(super::error(
                            "Ljava/lang/IndexOutOfBoundsException;",
                            "ByteBuffer.put bounds",
                        ));
                    }
                    (off_i as usize, len_i as usize)
                } else {
                    (0usize, src_len)
                };
                let rem = if pos >= 0 && limit >= pos {
                    (limit - pos) as usize
                } else {
                    0
                };
                let dst_len = array_len(vm, arr)?;
                if pos < 0
                    || len > rem
                    || (pos as usize) > dst_len
                    || len > dst_len - (pos as usize)
                {
                    return Err(super::error(
                        "Ljava/nio/BufferOverflowException;",
                        "overflow",
                    ));
                }
                let slice_bytes: Vec<i8> = {
                    let (src_elems, _) = vm.heap.as_array_elems(src)?;
                    src_elems[off..off + len]
                        .iter()
                        .map(|v| v.as_int().unwrap_or(0) as i8)
                        .collect()
                };
                let pos_u = pos as usize;
                let dst_elems = vm.heap.array_elems_mut(arr)?;
                for (slot, b) in dst_elems[pos_u..pos_u + len].iter_mut().zip(slice_bytes) {
                    *slot = Value::Int(i32::from(b));
                }
                put_field(vm, recv, "rd_pos", Value::Int((pos_u + len) as i32))?;
                Ok(Some(Value::Obj(recv)))
            } else {
                let dst_len = array_len(vm, arr)?;
                if pos < 0 || pos >= limit || (pos as usize) >= dst_len {
                    return Err(super::error(
                        "Ljava/nio/BufferOverflowException;",
                        "overflow",
                    ));
                }
                let b = int_arg(args, 0) as i8;
                let dst_elems = vm.heap.array_elems_mut(arr)?;
                if let Some(slot) = dst_elems.get_mut(pos as usize) {
                    *slot = Value::Int(i32::from(b));
                }
                put_field(vm, recv, "rd_pos", Value::Int(pos + 1))?;
                Ok(Some(Value::Obj(recv)))
            }
        }
        _ => Ok(None),
    }
}

impl Engine {
    /// Inject a custom camera frame (`RGBA_8888 = 1` or `JPEG = 256`) for `camera_id`
    /// and deliver immediately to any active repeating `CameraCaptureSession` surfaces.
    pub fn m10_inject_camera_frame(
        &mut self,
        camera_id: &str,
        width: i32,
        height: i32,
        format: i32,
        bytes: Vec<u8>,
    ) -> Result<usize, VmExit> {
        if !self.fw.vdev.cameras.contains_key(camera_id) {
            return Err(illegal(format!("unknown camera id: {camera_id}")));
        }
        if width <= 0 || height <= 0 || bytes.is_empty() {
            return Err(illegal(
                "camera frame width/height and bytes must be non-empty",
            ));
        }
        self.fw.vdev.injected_camera_frames.insert(
            camera_id.to_string(),
            CameraFrameSpec {
                width,
                height,
                format,
                bytes: bytes.clone(),
            },
        );
        let active_surfaces: Vec<ObjRef> = self
            .fw
            .vdev
            .camera_sessions
            .values()
            .filter(|s| !s.closed && s.repeating && s.camera_id == camera_id)
            .flat_map(|s| s.surfaces.iter().copied())
            .collect();
        if active_surfaces.is_empty() {
            return Ok(0);
        }
        self.with_host_roots(|vm| {
            let mut delivered = 0usize;
            for surf in active_surfaces {
                if enqueue_frame_to_surface(vm, surf, width, height, format, &bytes)? {
                    delivered += 1;
                }
            }
            Ok(delivered)
        })
    }
}
