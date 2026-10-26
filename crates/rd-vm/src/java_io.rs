//! Dispatch/metadata for the original character and byte-stream implementation.
//! The algorithms and typed GC edges live in jio_chars on both VM profiles.
use crate::{framework_host::HostMethodKind, Engine, ObjRef, Value, VmExit};
use alloc::{format, string::ToString, vec};
pub(crate) fn supports(class: &str) -> bool {
    crate::jio_chars::supports(class)
}
pub(crate) fn call(
    vm: &mut Engine,
    actual: &str,
    declaring: &str,
    name: &str,
    sig: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    let mut roots = vec![Value::Obj(receiver)];
    roots.extend_from_slice(args);
    crate::shared_call::with_values(vm, &roots, |vm| {
        if let Some(value) =
            crate::jio_chars::instance_intrinsic(vm, actual, declaring, name, sig, receiver, args)?
        {
            return Ok(Some(value));
        }
        // Constructors belong to their declared class. A DEX subclass must
        // execute its field initializers before invoking the host super ctor.
        if name == "<init>" {
            return Ok(None);
        }
        // Host-issued calls can name a DEX subclass whose inherited template
        // method has no DEX body. Walk its actual superclass declarations;
        // jio_chars keeps a guest override authoritative during template calls.
        let mut current = declaring.to_string();
        for _ in 0..64 {
            let Some(parent) = vm.cp.superclass_of(&current) else {
                break;
            };
            current = parent;
            if supports(&current) {
                if let Some(value) = crate::jio_chars::instance_intrinsic(
                    vm, actual, &current, name, sig, receiver, args,
                )? {
                    return Ok(Some(value));
                }
            }
        }
        Ok(None)
    })
}
pub fn method_metadata(class: &str, name: &str, sig: &str) -> Option<HostMethodKind> {
    use HostMethodKind::{Constructor, Instance};
    if !supports(class) {
        return None;
    }
    let c = class.strip_prefix("Ljava/io/")?.strip_suffix(';')?;
    if name == "<init>" {
        let yes = match c {
            "StringWriter" | "ByteArrayOutputStream" => matches!(sig, "()V" | "(I)V"),
            "Writer" | "Reader" => matches!(sig, "()V" | "(Ljava/lang/Object;)V"),
            "InputStream" | "OutputStream" => sig == "()V",
            "FileWriter" => matches!(
                sig,
                "(Ljava/lang/String;)V"
                    | "(Ljava/io/File;)V"
                    | "(Ljava/lang/String;Z)V"
                    | "(Ljava/io/File;Z)V"
            ),
            "OutputStreamWriter" => matches!(
                sig,
                "(Ljava/io/OutputStream;)V"
                    | "(Ljava/io/OutputStream;Ljava/lang/String;)V"
                    | "(Ljava/io/OutputStream;Ljava/nio/charset/Charset;)V"
            ),
            "InputStreamReader" => matches!(
                sig,
                "(Ljava/io/InputStream;)V"
                    | "(Ljava/io/InputStream;Ljava/lang/String;)V"
                    | "(Ljava/io/InputStream;Ljava/nio/charset/Charset;)V"
            ),
            "BufferedWriter" => matches!(sig, "(Ljava/io/Writer;)V" | "(Ljava/io/Writer;I)V"),
            "BufferedReader" => matches!(sig, "(Ljava/io/Reader;)V" | "(Ljava/io/Reader;I)V"),
            "StringReader" => sig == "(Ljava/lang/String;)V",
            "FileReader" => matches!(sig, "(Ljava/lang/String;)V" | "(Ljava/io/File;)V"),
            "ByteArrayInputStream" => matches!(sig, "([B)V" | "([BII)V"),
            "BufferedOutputStream" => matches!(
                sig,
                "(Ljava/io/OutputStream;)V" | "(Ljava/io/OutputStream;I)V"
            ),
            "FilterOutputStream" => sig == "(Ljava/io/OutputStream;)V",
            "PrintWriter" => matches!(
                sig,
                "(Ljava/io/Writer;)V"
                    | "(Ljava/io/Writer;Z)V"
                    | "(Ljava/io/OutputStream;)V"
                    | "(Ljava/io/OutputStream;Z)V"
                    | "(Ljava/lang/String;)V"
                    | "(Ljava/lang/String;Ljava/lang/String;)V"
                    | "(Ljava/io/File;)V"
                    | "(Ljava/io/File;Ljava/lang/String;)V"
            ),
            "PrintStream" => matches!(
                sig,
                "(Ljava/io/OutputStream;)V"
                    | "(Ljava/io/OutputStream;Z)V"
                    | "(Ljava/io/OutputStream;ZLjava/lang/String;)V"
                    | "(Ljava/lang/String;)V"
                    | "(Ljava/lang/String;Ljava/lang/String;)V"
                    | "(Ljava/io/File;)V"
                    | "(Ljava/io/File;Ljava/lang/String;)V"
            ),
            _ => false,
        };
        return yes.then_some(Constructor);
    }
    let writer = matches!(
        c,
        "StringWriter"
            | "OutputStreamWriter"
            | "BufferedWriter"
            | "PrintWriter"
            | "FileWriter"
            | "PrintStream"
            | "Writer"
    );
    let reader = matches!(
        c,
        "StringReader" | "InputStreamReader" | "BufferedReader" | "FileReader" | "Reader"
    );
    let bytes_out = matches!(c, "ByteArrayOutputStream" | "BufferedOutputStream");
    let bytes_in = c == "ByteArrayInputStream";
    let yes = match (name, sig) {
        ("close", "()V") => {
            (writer && c != "Writer") || (reader && c != "Reader") || bytes_out || bytes_in
        }
        ("flush", "()V") => (writer && c != "Writer") || bytes_out,
        ("write", "(I)V" | "([B)V" | "([BII)V") if bytes_out || c == "PrintStream" => true,
        (
            "write",
            "(I)V" | "([C)V" | "([CII)V" | "(Ljava/lang/String;)V" | "(Ljava/lang/String;II)V",
        ) => writer,
        ("read", "()I") => reader || bytes_in,
        ("read", "([C)I" | "([CII)I") => reader,
        ("read", "([B)I" | "([BII)I") => bytes_in,
        ("readAllBytes", "()[B") | ("available", "()I") => bytes_in,
        ("readLine", "()Ljava/lang/String;") => c == "BufferedReader",
        ("ready", "()Z") => reader && c != "Reader",
        ("mark", "(I)V") | ("markSupported", "()Z") => (reader && c != "Reader") || bytes_in,
        ("reset", "()V") => (reader && c != "Reader") || bytes_in || c == "ByteArrayOutputStream",
        ("skip", "(J)J") => (reader && c != "Reader") || bytes_in,
        ("toByteArray", "()[B") | ("size", "()I") | ("writeTo", "(Ljava/io/OutputStream;)V") => {
            c == "ByteArrayOutputStream"
        }
        ("toString", "()Ljava/lang/String;") => {
            matches!(c, "StringWriter" | "ByteArrayOutputStream")
        }
        (
            "toString",
            "(Ljava/lang/String;)Ljava/lang/String;"
            | "(Ljava/nio/charset/Charset;)Ljava/lang/String;",
        ) => c == "ByteArrayOutputStream",
        ("getBuffer", "()Ljava/lang/StringBuffer;") => c == "StringWriter",
        ("getEncoding", "()Ljava/lang/String;") => matches!(
            c,
            "InputStreamReader" | "OutputStreamWriter" | "FileReader" | "FileWriter"
        ),
        ("newLine", "()V") => c == "BufferedWriter",
        ("checkError", "()Z") => matches!(c, "PrintWriter" | "PrintStream"),
        _ if writer && name == "append" => [
            "Ljava/io/Writer;",
            "Ljava/io/StringWriter;",
            "Ljava/io/PrintWriter;",
            "Ljava/io/PrintStream;",
            "Ljava/lang/Appendable;",
        ]
        .iter()
        .any(|ret| {
            [
                "C",
                "Ljava/lang/CharSequence;",
                "Ljava/lang/CharSequence;II",
            ]
            .iter()
            .any(|arg| sig == format!("({arg}){ret}"))
        }),
        _ if matches!(c, "PrintWriter" | "PrintStream") && matches!(name, "print" | "println") => {
            sig == "()V" && name == "println"
                || [
                    "Z",
                    "C",
                    "I",
                    "J",
                    "F",
                    "D",
                    "[C",
                    "Ljava/lang/String;",
                    "Ljava/lang/Object;",
                ]
                .iter()
                .any(|arg| sig == format!("({arg})V"))
        }
        _ if matches!(c, "PrintWriter" | "PrintStream") && matches!(name, "format" | "printf") => [
            "Ljava/lang/String;[Ljava/lang/Object;",
            "Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;",
        ]
        .iter()
        .any(|arg| sig == format!("({arg}){class}")),
        _ => false,
    };
    yes.then_some(Instance)
}
