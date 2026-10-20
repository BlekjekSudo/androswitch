//! Real-APK native gate: `native_gate <apk> <lib> ['Lcls;->name(sig)' args...]...`
//! Each call is `--call DESCRIPTOR [ARG...]`; an ARG of `@static:Lcls;->FIELD`
//! passes that static field's value and `@ret:N` passes the value returned by
//! the N-th earlier call (e.g. a native handle). `@new:Lcls;` constructs an
//! instance, `@obj:Lcls;` reuses it, `@floats:1,2` passes a float[]. Use
//! `--call-on @obj:Lcls; DESC ARGS...` for instance methods. Prints one JSON report; never claims
//! success for a phase that returned an error.
#[cfg(feature = "native")]
fn main() {
    use rd_vm::{Engine, Value, VmConfig, VmExit};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (apk_path, lib) = (&args[0], &args[1]);
    let bytes = std::fs::read(apk_path).expect("read apk");
    let sha = {
        let mut h = std::process::Command::new("sha256sum")
            .arg(apk_path)
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        h.truncate(64);
        h
    };
    let apk = rd_apk::Apk::from_bytes(bytes).expect("parse apk");
    let dexes = apk
        .dex_files
        .iter()
        .map(|n| rd_dex::Dex::parse(apk.zip.read(n).expect("dex")).expect("parse dex"))
        .collect();
    let package = apk.manifest.package.clone();
    let mut e = Engine::new(dexes, VmConfig::default());
    e.set_resources(apk);
    let mut phases = Vec::new();
    let show = |r: Result<Value, VmExit>, e: &Engine| -> serde_json::Value {
        match r {
            Ok(v) => {
                serde_json::json!({"status":"ok","value":match v {Value::Int(i)=>serde_json::json!(i),Value::Long(l)=>serde_json::json!(l),Value::Obj(o)=>serde_json::json!(e.heap.as_str(o).map(str::to_string).unwrap_or_else(|_|format!("object#{o}"))),Value::Null=>serde_json::Value::Null,other=>serde_json::json!(format!("{other:?}"))}})
            }
            Err(VmExit::Exception(t)) => {
                serde_json::json!({"status":"java-exception","class":t.class,"message":t.message})
            }
            Err(VmExit::Error(x)) => {
                serde_json::json!({"status":"error","code":x.code,"cause":x.cause})
            }
        }
    };
    let r = rd_vm::native::load(&mut e, lib).map(|_| Value::Null);
    phases.push(serde_json::json!({"phase":format!("System.loadLibrary({lib})"),"result":show(r,&e),"native":e.native_report()}));
    let mut i = 2;
    let mut returned: Vec<Value> = Vec::new();
    let mut objects = std::collections::HashMap::new();
    while i < args.len() {
        // `--call DESC ARGS...` (static) or `--call-on RECV DESC ARGS...` (instance)
        let recv = match args[i].as_str() {
            "--call" => None,
            "--call-on" => {
                i += 1;
                Some(args[i].clone())
            }
            _ => {
                i += 1;
                continue;
            }
        };
        let d = args[i + 1].clone();
        let (cls, rest) = d.split_once("->").expect("Lcls;->name(sig)");
        let p = rest.find('(').unwrap();
        let (name, sig) = (&rest[..p], &rest[p..]);
        let (params, _) = rd_jni::descriptor(sig).expect("sig");
        let mut vals = Vec::new();
        i += 2;
        for t in &params {
            // `@static:Lcls;->FIELD` passes a static field value (e.g. a Kotlin
            // `object` INSTANCE receiver for its synthetic `$default` helpers).
            let v = if let Some(c) = args[i].strip_prefix("@new:") {
                // a fresh instance (constructor runs); later `@obj:Lcls;` reuses it
                let r = e.new_instance(c).expect("new instance");
                objects.insert(c.to_string(), r);
                Value::Obj(r)
            } else if let Some(c) = args[i].strip_prefix("@obj:") {
                Value::Obj(*objects.get(c).expect("@obj:Lcls; needs an earlier @new"))
            } else if let Some(list) = args[i].strip_prefix("@floats:") {
                let f: Vec<Value> = list
                    .split(',')
                    .map(|x| Value::Float(x.trim().parse().expect("float")))
                    .collect();
                let r = e
                    .heap
                    .alloc_array(rd_vm::heap::ElemKind::Float, f.len(), "[F")
                    .expect("float[]");
                *e.heap.array_elems_mut(r).expect("float[]") = f;
                Value::Obj(r)
            } else if let Some(n) = args[i].strip_prefix("@ret:") {
                // value returned by the N-th (1-based) earlier --call
                let n: usize = n.parse().expect("@ret:N");
                returned
                    .get(n.wrapping_sub(1))
                    .cloned()
                    .expect("@ret:N refers to an earlier call")
            } else if let Some(f) = args[i].strip_prefix("@static:") {
                let (c, n) = f.split_once("->").expect("@static:Lcls;->FIELD");
                e.ensure_initialized(c).expect("static initializer");
                e.statics
                    .get(&(c.to_string(), n.to_string()))
                    .cloned()
                    .expect("static field")
            } else {
                rd_vm::parse_arg_value(&args[i], t).expect("arg")
            };
            vals.push(v);
            i += 1;
        }
        let before = e.native_report()["instructions"].as_u64().unwrap_or(0);
        let r = match &recv {
            None => e.invoke_static(cls, name, sig, &vals),
            Some(c) => {
                let o = *objects
                    .get(c.strip_prefix("@obj:").unwrap_or(c))
                    .expect("--call-on @obj:Lcls; needs an earlier @new");
                e.invoke_instance(o, name, sig, &vals)
            }
        };
        returned.push(r.as_ref().ok().cloned().unwrap_or(Value::Null));
        let after = e.native_report()["instructions"].as_u64().unwrap_or(0);
        phases.push(serde_json::json!({"phase":d,"result":show(r,&e),"native_instructions_delta":after-before}));
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"apk":apk_path,"sha256":sha,"package":package,"phases":phases,"final":e.native_report()})).unwrap());
}
#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("build with --features native");
    std::process::exit(2);
}
