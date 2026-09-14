use anyhow::{bail, Result};
use std::path::PathBuf;

pub const HELP: &str = "slink — managed symbolic links\n\nUSAGE:\n  slink [OPTIONS] <target> <link>\n  slink list\n  slink check [link ...]\n  slink fix [--parents] [--replace] [link ...]\n  slink remove [--keep-link] <link ...>\n  slink adopt <link ...>\n  slink scan <directory ...>\n\nOPTIONS:\n  --file <path>  Use one explicit TOML registry\n  --relative     Generate a relative target (creation only)\n  --parents      Create missing link parent directories (create/fix)\n  --replace      Replace mismatched managed symlinks (fix only)\n  --keep-link    Unregister without deleting the link (remove only)\n  --dry-run      Display a mutation plan without writing anything\n  --help         Show help\n  --version      Show version\n\nUse -- before literal operands, e.g. slink -- list ./list-link.\nWithout --relative, target is stored literally, as with ln -s.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Create,
    List,
    Check,
    Fix,
    Remove,
    Adopt,
    Scan,
    Help,
    Version,
}

#[derive(Debug)]
pub struct Args {
    pub command: Command,
    pub operands: Vec<String>,
    pub file: Option<PathBuf>,
    pub dry_run: bool,
    pub relative: bool,
    pub parents: bool,
    pub replace: bool,
    pub keep_link: bool,
}

impl Args {
    pub fn parse(input: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut a = Self {
            command: Command::Create,
            operands: vec![],
            file: None,
            dry_run: false,
            relative: false,
            parents: false,
            replace: false,
            keep_link: false,
        };
        let mut input = input.into_iter();
        let mut literal = false;
        let mut first = true;
        while let Some(s) = input.next() {
            if !literal {
                match s.as_str() {
                    "--" => {
                        literal = true;
                        continue;
                    }
                    "--help" | "-h" => {
                        a.command = Command::Help;
                        return Ok(a);
                    }
                    "--version" | "-V" => {
                        a.command = Command::Version;
                        return Ok(a);
                    }
                    "--file" => {
                        a.file = Some(
                            input
                                .next()
                                .filter(|x| !x.is_empty())
                                .ok_or_else(|| anyhow::anyhow!("--file needs a path"))?
                                .into(),
                        );
                        continue;
                    }
                    "--dry-run" => {
                        a.dry_run = true;
                        continue;
                    }
                    "--relative" => {
                        a.relative = true;
                        continue;
                    }
                    "--parents" => {
                        a.parents = true;
                        continue;
                    }
                    "--replace" => {
                        a.replace = true;
                        continue;
                    }
                    "--keep-link" => {
                        a.keep_link = true;
                        continue;
                    }
                    _ if s.starts_with("--file=") => {
                        if s.len() == 7 {
                            bail!("--file needs a path");
                        }
                        a.file = Some(s[7..].into());
                        continue;
                    }
                    _ if s.starts_with('-') => {
                        bail!("unknown option {s:?}; use -- for literal paths")
                    }
                    _ => {}
                }
            }
            if first && !literal {
                let cmd = match s.as_str() {
                    "list" => Some(Command::List),
                    "check" => Some(Command::Check),
                    "fix" => Some(Command::Fix),
                    "remove" => Some(Command::Remove),
                    "adopt" => Some(Command::Adopt),
                    "scan" => Some(Command::Scan),
                    _ => None,
                };
                first = false;
                if let Some(cmd) = cmd {
                    a.command = cmd;
                    continue;
                }
            }
            first = false;
            a.operands.push(s);
        }
        if first && a.command == Command::Create {
            a.command = Command::Help;
            return Ok(a);
        }
        match a.command {
            Command::Create if a.operands.len() != 2 => bail!("creation needs <target> <link>"),
            Command::List if !a.operands.is_empty() => bail!("list takes no operands"),
            Command::Remove | Command::Adopt | Command::Scan if a.operands.is_empty() => {
                bail!("this command needs at least one path")
            }
            _ => {}
        }
        if a.relative && a.command != Command::Create {
            bail!("--relative is only valid for creation");
        }
        if a.parents && !matches!(a.command, Command::Create | Command::Fix) {
            bail!("--parents is only valid for create/fix");
        }
        if a.replace && a.command != Command::Fix {
            bail!("--replace is only valid for fix");
        }
        if a.keep_link && a.command != Command::Remove {
            bail!("--keep-link is only valid for remove");
        }
        if a.dry_run && !a.mutates() {
            bail!("--dry-run is only valid for mutation commands");
        }
        Ok(a)
    }
    pub fn mutates(&self) -> bool {
        matches!(
            self.command,
            Command::Create | Command::Adopt | Command::Fix | Command::Remove
        )
    }
}
