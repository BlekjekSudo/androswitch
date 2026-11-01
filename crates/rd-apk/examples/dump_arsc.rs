//! Dump estrutural do resources.arsc — debug de resolução de recursos.
//! Uso: cargo run -p rd-apk --example dump_arsc -- <apk> [resid_hex]

use rd_apk::Apk;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: dump_arsc <apk> [resid]");
    let apk = Apk::open(std::path::Path::new(&path)).expect("apk");
    let arsc = apk.arsc.as_ref().expect("sem resources.arsc");

    println!("global strings: {}", arsc.global_strings.len());
    for pkg in &arsc.packages {
        // ADR 014: resids vêm do índice lazy (sem materializar entradas).
        let ids = pkg.resource_ids();
        println!(
            "package id=0x{:02x} name={:?} types={} keys={} entries={}",
            pkg.id,
            pkg.name,
            pkg.type_strings.len(),
            pkg.key_strings.len(),
            ids.len()
        );
        // distribuição de type ids
        let mut type_ids: Vec<u16> = ids.iter().map(|r| ((r >> 16) & 0xFF) as u16).collect();
        type_ids.sort();
        type_ids.dedup();
        println!("  type ids presentes: {:?}", type_ids);
    }

    if let Some(hex) = std::env::args().nth(2) {
        let resid = u32::from_str_radix(hex.trim_start_matches("0x"), 16).expect("resid hex");
        println!("\nresolvendo 0x{resid:08x}:");
        let pkg = arsc.package_for(resid);
        println!("  package: {:?}", pkg.map(|p| p.name.clone()));
        // ADR 014: snapshot materializado do resid (diagnóstico).
        let entries = pkg
            .and_then(|p| {
                p.entries_snapshot()
                    .into_iter()
                    .find(|(id, _)| *id == resid)
            })
            .map(|(_, es)| es);
        match entries {
            Some(es) if !es.is_empty() => {
                println!("  {} entradas:", es.len());
                for e in &es {
                    println!(
                        "    cfg(lang={:?}, density={:?}, api={:?}) type={} data=0x{:x} string={:?}",
                        e.config.language, e.config.density, e.config.api_level,
                        e.data_type, e.data, e.string
                    );
                }
                println!("  resolve_string → {:?}", arsc.resolve_string(resid));
                println!("  resolve_name   → {:?}", arsc.resolve_name(resid));
            }
            _ => println!("  SEM ENTRADAS para este resid"),
        }
    }
}
