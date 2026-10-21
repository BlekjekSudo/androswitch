//! APK-backed Java classpath resources (`ClassLoader`, `URL`, `URLConnection`).
//!
//! The headless VM has one APK class loader. A resource URL uses the explicit
//! virtual `apk:/...` scheme; it is not a host filesystem path and does not
//! access the network. URL streams read the verified ZIP entry from the APK
//! already attached to the Engine.

use crate::engine::Engine;
use crate::err::{Throwable, VmExit};
use crate::heap::ObjRef;
use crate::value::Value;

const CLASS_LOADER: &str = "Ljava/lang/ClassLoader;";
const URL: &str = "Ljava/net/URL;";
const URL_CONNECTION: &str = "Ljava/net/URLConnection;";
const ENUMERATION: &str = "Ljava/util/Enumeration;";

/// ClassLoader resource APIs and the URL/URLConnection objects they return.
pub(crate) fn instance_intrinsic(
    vm: &mut Engine,
    recv_class: &str,
    mref_class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    if mref_class == CLASS_LOADER && class_loader_supported(name, sig) {
        // Keep DEX overrides in front of the host fallback for subclasses.
        if vm.cp.resolve_method(recv_class, name, sig).is_some() {
            return Ok(None);
        }
        return class_loader_intrinsic(vm, name, sig, args);
    }

    match (recv_class, mref_class, name, sig) {
        (URL, URL, "openConnection", "()Ljava/net/URLConnection;") => {
            let connection = vm
                .heap
                .alloc_instance(URL_CONNECTION.to_string(), Vec::new())?;
            crate::err::put_field_result(vm.heap.put_field(
                connection,
                "rd_resource_url",
                Value::Obj(recv),
            ))?;
            Ok(Some(Value::Obj(connection)))
        }
        (URL, URL, "openStream", "()Ljava/io/InputStream;") => {
            let bytes = resource_bytes(vm, recv)?;
            crate::jio_chars::new_byte_input_stream(vm, bytes).map(Some)
        }
        (URL, URL, "toString" | "toExternalForm", "()Ljava/lang/String;") => Ok(Some(
            vm.heap
                .get_field(recv, "rd_url_spec")
                .unwrap_or(Value::Null),
        )),
        (URL, URL, "getProtocol", "()Ljava/lang/String;") => Ok(Some(Value::Obj(
            crate::intrinsics::alloc_string(vm, "apk".to_string())?,
        ))),
        (URL, URL, "getPath" | "getFile", "()Ljava/lang/String;") => {
            let path = match vm.heap.get_field(recv, "rd_resource_name") {
                Ok(Value::Obj(r)) => format!("/{}", vm.heap.as_str(r)?),
                _ => String::new(),
            };
            Ok(Some(Value::Obj(crate::intrinsics::alloc_string(vm, path)?)))
        }
        (URL_CONNECTION, URL_CONNECTION, "setUseCaches", "(Z)V") => Ok(Some(Value::Null)),
        (URL_CONNECTION, URL_CONNECTION, "getURL", "()Ljava/net/URL;") => Ok(Some(
            vm.heap
                .get_field(recv, "rd_resource_url")
                .unwrap_or(Value::Null),
        )),
        (URL_CONNECTION, URL_CONNECTION, "getInputStream", "()Ljava/io/InputStream;") => {
            let url = match vm.heap.get_field(recv, "rd_resource_url") {
                Ok(Value::Obj(r)) => r,
                _ => {
                    return Err(exception(
                        "Ljava/io/IOException;",
                        "URLConnection has no APK resource URL",
                    ));
                }
            };
            let bytes = resource_bytes(vm, url)?;
            crate::jio_chars::new_byte_input_stream(vm, bytes).map(Some)
        }
        _ => Ok(None),
    }
}

fn class_loader_supported(name: &str, sig: &str) -> bool {
    matches!(
        (name, sig),
        ("getResource", "(Ljava/lang/String;)Ljava/net/URL;")
            | (
                "getResourceAsStream",
                "(Ljava/lang/String;)Ljava/io/InputStream;"
            )
            | (
                "getResources",
                "(Ljava/lang/String;)Ljava/util/Enumeration;"
            )
    )
}

fn class_loader_intrinsic(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let path = resource_name(vm, args)?;
    match (name, sig) {
        ("getResource", "(Ljava/lang/String;)Ljava/net/URL;") => {
            Ok(Some(match resource_exists(vm, &path) {
                true => Value::Obj(new_resource_url(vm, &path)?),
                false => Value::Null,
            }))
        }
        ("getResourceAsStream", "(Ljava/lang/String;)Ljava/io/InputStream;") => {
            match read_resource(vm, &path)? {
                Some(bytes) => crate::jio_chars::new_byte_input_stream(vm, bytes).map(Some),
                None => Ok(Some(Value::Null)),
            }
        }
        ("getResources", "(Ljava/lang/String;)Ljava/util/Enumeration;") => {
            let elems = if resource_exists(vm, &path) {
                vec![Value::Obj(new_resource_url(vm, &path)?)]
            } else {
                Vec::new()
            };
            let enumeration = vm
                .heap
                .alloc_instance(ENUMERATION.to_string(), Vec::new())?;
            vm.fw.objects.insert(
                enumeration,
                crate::framework::HostObj::Iter { elems, pos: 0 },
            );
            Ok(Some(Value::Obj(enumeration)))
        }
        _ => Ok(None),
    }
}

fn resource_name(vm: &Engine, args: &[Value]) -> Result<String, VmExit> {
    let Some(value) = args.first() else {
        return Err(
            crate::err::vm_error("INVALID_ARG", "ClassLoader resource method sem nome").into(),
        );
    };
    let Some(reference) = value.as_ref()? else {
        return Err(exception(
            "Ljava/lang/NullPointerException;",
            "ClassLoader resource name == null",
        ));
    };
    vm.heap
        .as_str(reference)
        .map(str::to_string)
        .map_err(|e| crate::err::vm_error("INVALID_ARG", e).into())
}

fn resource_exists(vm: &Engine, path: &str) -> bool {
    vm.fw
        .resources
        .as_ref()
        .is_some_and(|apk| apk.zip.find(path).is_some())
}

fn new_resource_url(vm: &mut Engine, path: &str) -> Result<ObjRef, VmExit> {
    let url = vm.heap.alloc_instance(URL.to_string(), Vec::new())?;
    let name = crate::intrinsics::alloc_string(vm, path.to_string())?;
    let spec = crate::intrinsics::alloc_string(vm, format!("apk:/{path}"))?;
    crate::err::put_field_result(vm.heap.put_field(url, "rd_resource_name", Value::Obj(name)))?;
    crate::err::put_field_result(vm.heap.put_field(url, "rd_url_spec", Value::Obj(spec)))?;
    Ok(url)
}

fn read_resource(vm: &Engine, path: &str) -> Result<Option<Vec<u8>>, VmExit> {
    let Some(apk) = &vm.fw.resources else {
        return Ok(None);
    };
    let Some(entry) = apk.zip.find(path) else {
        return Ok(None);
    };
    apk.zip.read_entry(entry).map(Some).map_err(|e| {
        exception(
            "Ljava/io/IOException;",
            &format!("reading APK resource {path}: {}", e.cause),
        )
    })
}

fn resource_bytes(vm: &Engine, url: ObjRef) -> Result<Vec<u8>, VmExit> {
    let path = match vm.heap.get_field(url, "rd_resource_name") {
        Ok(Value::Obj(r)) => vm.heap.as_str(r)?.to_string(),
        _ => {
            return Err(exception(
                "Ljava/io/FileNotFoundException;",
                "URL is not backed by an APK resource",
            ));
        }
    };
    read_resource(vm, &path)?.ok_or_else(|| {
        exception(
            "Ljava/io/FileNotFoundException;",
            &format!("APK resource not found: {path}"),
        )
    })
}

fn exception(class: &str, message: &str) -> VmExit {
    VmExit::Exception(Throwable::new(class, message))
}
