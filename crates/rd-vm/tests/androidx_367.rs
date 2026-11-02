//! Regression test for issue #367 (`crates/rd-vm/src/androidx.rs`):
//! `FileProvider.getUriForFile` in a loop must not grow the host-side URI
//! history without limit. The list keeps the newest 4096 entries.

use rd_vm::androidx::*;
use rd_vm::value::Value;
use rd_vm::{Engine, VmConfig};

#[test]
fn issue_367_file_provider_uri_history_is_capped_and_keeps_newest() {
    let mut e = Engine::new(Vec::new(), VmConfig::default());
    let file = e
        .heap
        .alloc_instance("Ljava/io/File;".to_string(), Vec::new())
        .unwrap();
    for i in 0..5000 {
        let p = Value::Obj(e.heap.alloc_string(format!("/data/f{i}.jpg")).unwrap());
        e.heap.put_field(file, "path", p).unwrap();
        let auth = Value::Obj(e.heap.alloc_string("com.app.provider".to_string()).unwrap());
        call_static(
            &mut e,
            FILE_PROVIDER,
            "getUriForFile",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;",
            &[Value::Null, auth, Value::Obj(file)],
        )
        .unwrap();
    }
    let uris = e.androidx_file_provider_uris();
    assert!(uris.len() <= 4096, "history grew to {}", uris.len());
    assert_eq!(
        uris.last().map(String::as_str),
        Some("content://com.app.provider/rustydroid/data/f4999.jpg")
    );
}
