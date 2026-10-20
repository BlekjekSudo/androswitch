//! Static probe: ABI, DT_NEEDED, undefined imports and JNI exports of an ELF.
use rd_ndk::elf::Elf;
fn main() {
    for p in std::env::args().skip(1) {
        let b = std::fs::read(&p).expect("read");
        match Elf::parse(&b) {
            Ok(e) => {
                let undef: Vec<_> = e
                    .symbols
                    .iter()
                    .filter(|s| !s.defined && !s.name.is_empty())
                    .map(|s| s.name.clone())
                    .collect();
                let jni: Vec<_> = e
                    .symbols
                    .iter()
                    .filter(|s| {
                        s.defined && (s.name.starts_with("Java_") || s.name == "JNI_OnLoad")
                    })
                    .map(|s| s.name.clone())
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({"file":p,"abi":e.abi,"needed":e.needed,"relocations":e.relocations.len(),"undefined":undef,"jni":jni})
                );
            }
            Err(err) => println!("{}", serde_json::json!({"file":p,"error":err})),
        }
    }
}
