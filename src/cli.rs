use anyhow::{bail, Result};

pub const HELP: &str = "slink — managed symbolic links\n\nUSAGE:\n  slink [OPTIONS] <target> <link>\n  slink --config\n  slink list [-o <human|tsv>]\n  slink check [-o <human|tsv>] [link ...]\n  slink fix [-fnp] [link ...]\n  slink remove [-kn] <link ...>\n  slink adopt [-n] <link ...>\n  slink scan [-R] [-o <human|tsv>] [directory ...]\n\nOPTIONS:\n  -c, --config         Print the registry path without creating it\n  -f, --force          Replace different symlinks (create/fix only)\n  -p, --parents        Create missing link parent directories (create/fix)\n  -n, --dry-run        Display a mutation plan without writing anything\n  -k, --keep-link      Unregister without deleting the link (remove only)\n  -R, --recursive      Scan subdirectories; never follow directory symlinks\n  -o, --format <name>  Output for list/check/scan: human (default) or tsv\n  -h, --help           Show help\n  -V, --version        Show version\n\nCLI paths start from the working directory; ~/ expands to your home.\nRegistry v2 stores absolute link and target paths only.\nUse -- before literal operands, e.g. slink -- list ./list-link.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Create,
    Config,
    List,
    Check,
    Fix,
    Remove,
    Adopt,
    Scan,
    Help,
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Tsv,
}

#[derive(Debug)]
pub struct Args {
    pub command: Command,
    pub operands: Vec<String>,
    pub output_format: OutputFormat,
    pub dry_run: bool,
    pub parents: bool,
    pub force: bool,
    pub keep_link: bool,
    pub recursive: bool,
}

impl Args {
    pub fn parse(input: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut input = input.into_iter().peekable();
        let empty = input.peek().is_none();
        let mut a = Self {
            command: Command::Create,
            operands: vec![],
            output_format: OutputFormat::Human,
            dry_run: false,
            parents: false,
            force: false,
            keep_link: false,
            recursive: false,
        };
        let mut info = None;
        let mut format_set = false;
        let mut literal = false;
        let mut first = true;
        while let Some(s) = input.next() {
            if !literal && s == "--" {
                literal = true;
                continue;
            }
            if !literal && s.starts_with('-') && s != "-" {
                // Expand short names and clusters once; all option validation
                // then uses the same names as long options.
                let mut flags = vec![];
                if let Some(long) = s.strip_prefix("--") {
                    flags.push(long.to_owned());
                } else {
                    let mut chars = s[1..].chars();
                    while let Some(c) = chars.next() {
                        let name = match c {
                            'c' => "config",
                            'f' => "force",
                            'p' => "parents",
                            'n' => "dry-run",
                            'k' => "keep-link",
                            'R' => "recursive",
                            'h' => "help",
                            'V' => "version",
                            'o' => {
                                let rest = chars.as_str();
                                flags.push(if rest.is_empty() {
                                    "format".into()
                                } else {
                                    format!("format={rest}")
                                });
                                break;
                            }
                            _ => bail!("unknown option -{c}; use -- for literal paths"),
                        };
                        flags.push(name.to_owned());
                    }
                }
                for flag in flags {
                    match flag.as_str() {
                        "config" | "help" | "version" => {
                            let command = match flag.as_str() {
                                "config" => Command::Config,
                                "help" => Command::Help,
                                _ => Command::Version,
                            };
                            if info.is_some_and(|old| old != command) {
                                bail!("--config, --help and --version are mutually exclusive");
                            }
                            info = Some(command);
                        }
                        "force" => a.force = true,
                        "parents" => a.parents = true,
                        "dry-run" => a.dry_run = true,
                        "keep-link" => a.keep_link = true,
                        "recursive" => a.recursive = true,
                        "format" => {
                            a.output_format =
                                parse_output_format(&input.next().ok_or_else(|| {
                                    anyhow::anyhow!("--format needs human or tsv")
                                })?)?;
                            format_set = true;
                        }
                        _ if flag.starts_with("format=") => {
                            a.output_format = parse_output_format(&flag[7..])?;
                            format_set = true;
                        }
                        _ => bail!("unknown option --{flag}; use -- for literal paths"),
                    }
                }
                continue;
            }
            if first && !literal {
                let command = match s.as_str() {
                    "list" => Some(Command::List),
                    "check" => Some(Command::Check),
                    "fix" => Some(Command::Fix),
                    "remove" => Some(Command::Remove),
                    "adopt" => Some(Command::Adopt),
                    "scan" => Some(Command::Scan),
                    _ => None,
                };
                first = false;
                if let Some(command) = command {
                    a.command = command;
                    continue;
                }
            }
            first = false;
            a.operands.push(s);
        }
        if let Some(command) = info {
            if !first
                || format_set
                || a.dry_run
                || a.parents
                || a.force
                || a.keep_link
                || a.recursive
            {
                bail!("--config, --help and --version take no other command, operands or options");
            }
            a.command = command;
            return Ok(a);
        }
        if empty {
            a.command = Command::Help;
            return Ok(a);
        }
        match a.command {
            Command::Create if a.operands.len() != 2 => bail!("creation needs <target> <link>"),
            Command::List if !a.operands.is_empty() => bail!("list takes no operands"),
            Command::Remove | Command::Adopt if a.operands.is_empty() => {
                bail!("this command needs at least one path")
            }
            Command::Scan if a.operands.is_empty() => a.operands.push(".".into()),
            _ => {}
        }
        if format_set && !matches!(a.command, Command::List | Command::Check | Command::Scan) {
            bail!("--format is only valid for list/check/scan");
        }
        if (a.parents || a.force) && !matches!(a.command, Command::Create | Command::Fix) {
            bail!("--parents and --force are only valid for create/fix");
        }
        if a.keep_link && a.command != Command::Remove {
            bail!("--keep-link is only valid for remove");
        }
        if a.recursive && a.command != Command::Scan {
            bail!("--recursive is only valid for scan");
        }
        if a.dry_run && !a.mutates() {
            bail!("--dry-run is only valid for mutation commands");
        }
        if a.command == Command::Create {
            crate::paths::validate_target(&a.operands[0])?;
            crate::paths::validate_link(&a.operands[1])?;
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

fn parse_output_format(value: &str) -> Result<OutputFormat> {
    match value {
        "human" => Ok(OutputFormat::Human),
        "tsv" => Ok(OutputFormat::Tsv),
        _ => bail!("invalid --format {value:?}; expected human or tsv"),
    }
}
