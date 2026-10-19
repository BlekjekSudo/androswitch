//! Deterministic mutation campaign; wall-clock duration is explicit and reported.
use rd_ndk::elf::Elf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let seconds = args
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    let mut seeds = Vec::new();
    for path in &args[1..] {
        seeds.push(std::fs::read(path).expect("read seed ELF"));
    }
    if seeds.is_empty() {
        seeds.push(vec![0; 128]);
    }
    let start = Instant::now();
    let mut report = Instant::now();
    let mut random = 0x9e3779b97f4a7c15u64;
    let mut next = || {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        random
    };
    let mut count = 0u64;
    let mut accepted = 0u64;
    while start.elapsed() < Duration::from_secs(seconds) {
        let seed = &seeds[(next() as usize) % seeds.len()];
        let mut data = seed.clone();
        if next() % 8 == 0 {
            let len = (next() as usize) % (data.len() + 1);
            data.truncate(len);
        } else {
            for _ in 0..1 + next() % 12 {
                if !data.is_empty() {
                    let i = (next() as usize) % data.len();
                    data[i] ^= next() as u8;
                }
            }
        }
        if Elf::parse(&data).is_ok() {
            accepted += 1;
        }
        count += 1;
        if report.elapsed() > Duration::from_secs(60) {
            println!(
                "elapsed={} cases={} accepted={}",
                start.elapsed().as_secs(),
                count,
                accepted
            );
            report = Instant::now();
        }
        if count % 4096 == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    println!("{{\"status\":\"passed\",\"cases\":{count},\"accepted\":{accepted},\"elapsed_seconds\":{},\"finished_epoch\":{}}}",start.elapsed().as_secs(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs());
}
