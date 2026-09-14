mod cli;
mod engine;
mod inspect;
mod paths;
mod registry;
mod transaction;

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

fn default_registry_path() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(root) => root,
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .context("HOME must be an absolute directory")?
            .join(".config"),
    };
    Ok(root.join("slink/links.toml"))
}

fn run() -> Result<u8> {
    let args = std::env::args_os()
        .skip(1)
        .map(|s| {
            s.into_string()
                .map_err(|_| anyhow::anyhow!("arguments must be valid UTF-8"))
        })
        .collect::<Result<Vec<_>>>()?;

    if args.first().is_some_and(|arg| arg == "config") {
        if args.len() != 1 {
            bail!("config takes no operands or options");
        }
        println!("{}", paths::text(&default_registry_path()?)?);
        return Ok(0);
    }

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
