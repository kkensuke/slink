mod cli;
mod engine;
mod inspect;
mod paths;
mod registry;
mod transaction;

fn main() {
    let result = std::env::args_os()
        .skip(1)
        .map(|s| {
            s.into_string()
                .map_err(|_| anyhow::anyhow!("arguments must be valid UTF-8"))
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .and_then(cli::Args::parse)
        .and_then(engine::run);
    let code = match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("slink: {e:#}");
            2
        }
    };
    std::process::exit(code.into());
}
