use std::{env, fs, process};
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        eprintln!("usage: as-probe APK REQUEST.json");
        process::exit(64);
    }
    let apk = if args[0] == "-" {
        Vec::new()
    } else {
        fs::read(&args[0]).expect("read APK")
    };
    let request = fs::read(&args[1]).expect("read request");
    // The recursive upstream interpreter has large debug frames. Keep the host
    // stack explicit so VM depth errors precede native Windows stack overflow.
    let result = std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let filesystem = args.get(2).map(|path| {
                std::rc::Rc::new(
                    as_platform::host_fs::HostFileSystem::new(path).expect("storage root"),
                ) as std::rc::Rc<dyn as_platform::fs::FileSystem>
            });
            as_switch_smoke::probe::run_json_with_fs(apk, &request, filesystem)
        })
        .expect("probe thread")
        .join()
        .expect("probe completed");
    println!("{}", String::from_utf8(result).expect("JSON is UTF-8"));
}
