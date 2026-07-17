#![forbid(unsafe_code)]

mod check_local_sources;
mod simdoc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("simdoc") => simdoc::run(args),
        Some("check-local-sources") => check_local_sources::run(args),
        Some(other) => Err(format!("unknown xtask subcommand `{other}`")),
        None => Err("usage: xtask <simdoc|check-local-sources>".to_owned()),
    };
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
