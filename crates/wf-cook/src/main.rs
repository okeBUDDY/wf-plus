//! `wf-cook <art dir> <font dir> <out file>`: cooks the art outside a build,
//! for inspecting the pack.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: wf-cook <art dir> <font dir> <bitmap dir> <out file>");
        std::process::exit(2);
    }
    let config = wf_cook::Config::new(args[1].clone().into(), args[2].clone().into(), args[3].clone().into());
    match wf_cook::cook(&config) {
        Ok((blob, report)) => {
            std::fs::write(&args[4], &blob).expect("write pack");
            println!(
                "{} sprites ({} bitmaps), {} layers, {} curves, {} bytes; skipped vectors {:?}",
                report.sprites,
                report.bitmaps,
                report.layers,
                report.curves,
                blob.len(),
                report.skipped
            );
        }
        Err(e) => {
            eprintln!("wf-cook: {e}");
            std::process::exit(1);
        }
    }
}
