use std::{env, fs, process};
fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: as-probe APK REQUEST.json");
        process::exit(64);
    }
    let apk = fs::read(&args[0]).expect("read APK");
    let request = fs::read(&args[1]).expect("read request");
    // The recursive upstream interpreter has large debug frames. Keep the host
    // stack explicit so VM depth errors precede native Windows stack overflow.
    let result = std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || as_switch_smoke::probe::run_json(apk, &request))
        .expect("probe thread")
        .join()
        .expect("probe completed");
    println!("{}", String::from_utf8(result).expect("JSON is UTF-8"));
}
