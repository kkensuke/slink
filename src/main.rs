mod cli;
mod engine;
mod inspect;
mod output;
mod paths;
mod registry;
mod transaction;

use anyhow::Result;

fn run() -> Result<u8> {
    let args = std::env::args_os()
        .skip(1)
        .map(|s| {
            s.into_string()
                .map_err(|_| anyhow::anyhow!("arguments must be valid UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;

    engine::run(cli::Args::parse(args)?)
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("slink: {e:#}");
            2
        }
    };
    std::process::exit(code.into());
}
