use crate::{
    cli::{Args, Command, HELP},
    inspect::{self, Snapshot},
    output, paths,
    registry::{Entry, Registry},
    transaction::{self, Operation, Request},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub fn run(args: Args) -> Result<u8> {
    match args.command {
        Command::Config => {
            println!("{}", paths::text(&paths::default_registry_path()?)?);
            return Ok(0);
        }
        Command::Help => {
            print!("{HELP}");
            return Ok(0);
        }
        Command::Version => {
            println!("slink {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        _ => {}
    }
    let mut r = Registry::open(matches!(
        args.command,
        Command::Create | Command::Adopt | Command::Scan
    ))?;
    if args.command == Command::List {
        return output::list(&r, args.output_format);
    }
    if args.command == Command::Scan {
        return output::scan(&r, &args.operands, args.recursive, args.output_format);
    }
    let pending = transaction::load(&r)?;
    if args.command == Command::Check {
        let mut checked = Vec::new();
        for entry in selected(&r, &args.operands)? {
            let link = r.link(&entry)?;
            let diagnosis = inspect::diagnose(&link, &entry.target);
            checked.push((link, entry.target, diagnosis));
        }
        let failed = pending.is_some() || checked.iter().any(|(_, _, d)| !d.is_healthy());
        output::check(&checked, pending.as_ref(), args.output_format);
        return Ok(u8::from(failed));
    }
    let _lock = if args.dry_run { None } else { Some(r.lock()?) };
    // A preview advances only the in-memory registry; verify its disk snapshot once.
    if args.dry_run {
        r.verify()?;
    }
    let mut recovered = None;
    if let Some(p) = transaction::load(&r)? {
        let same_request = p.request == Request::from(&args);
        let same_selection = if args.command == Command::Create {
            paths::link_from_cli(&args.operands[1])? == p.link
                && paths::target_from_cli(&args.operands[0])? == p.entry.target
        } else {
            (args.command == Command::Fix && args.operands.is_empty())
                || args
                    .operands
                    .iter()
                    .any(|s| paths::link_from_cli(s).is_ok_and(|x| x == p.link))
        };
        if !same_request || !same_selection || args.keep_link {
            bail!("PENDING: repeat the original operation with the same target and options before making other changes");
        }
        if args.dry_run {
            transaction::preview(&mut r, &p)?;
            println!("WOULD_RECOVER\t{:?}\t{:?}", p.op, p.link);
        } else {
            transaction::finish(&mut r, &p)
                .context("recovery stopped; pending operation retained")?;
            println!("RECOVERED\t{:?}", p.link);
        }
        if args.command != Command::Remove {
            output::warn_target(&p.link, &p.entry.target);
        }
        if args.command == Command::Create {
            return Ok(0);
        }
        recovered = Some(p.link);
    }
    let result = if args.command == Command::Create {
        plan_create(&r, &args).and_then(|plan| plan.apply(&mut r, &args))
    } else {
        return mutate_many(&mut r, &args, recovered.as_deref());
    };
    match result {
        Ok(()) => Ok(0),
        Err(error) => {
            eprintln!("slink: {error:#}");
            Ok(1)
        }
    }
}

fn selected(r: &Registry, operands: &[String]) -> Result<Vec<Entry>> {
    if operands.is_empty() {
        return Ok(r.entries.clone());
    }
    let mut entries = vec![];
    let mut seen = HashSet::new();
    for s in operands {
        let link = paths::link_from_cli(s)?;
        let index = r
            .find(&link)?
            .with_context(|| format!("not registered: {link:?}"))?;
        if seen.insert(index) {
            entries.push(r.entries[index].clone());
        }
    }
    Ok(entries)
}

fn ensure_separate(r: &Registry, link: &Path) -> Result<()> {
    let existing = r.find(link)?;
    for (index, entry) in r.entries.iter().enumerate() {
        if Some(index) != existing && paths::overlaps(link, &r.link(entry)?)? {
            bail!("nested managed destination; manage the parent or its children: {link:?}");
        }
    }
    Ok(())
}

enum Change {
    Keep(Option<Snapshot>),
    Create,
    Replace(Snapshot),
    Remove(Snapshot),
}

// One plan represents independent filesystem and registry changes. Both preview
// and execution consume this plan; command handlers only select the authority.
struct Plan {
    entry: Entry,
    link: PathBuf,
    change: Change,
    after: String,
    mkdir: bool,
}

impl Plan {
    fn apply(self, r: &mut Registry, args: &Args) -> Result<()> {
        if !args.dry_run {
            r.verify()?;
        }
        let registry_changes = r.original.as_deref() != Some(self.after.as_bytes());
        let action = match &self.change {
            Change::Keep(Some(old)) => {
                if inspect::snapshot(&self.link)?.as_ref() != Some(old) {
                    bail!("link changed before registration: {:?}", self.link);
                }
                if registry_changes {
                    "REGISTER"
                } else {
                    "UNCHANGED"
                }
            }
            Change::Keep(None) => {
                if registry_changes {
                    "UNREGISTER"
                } else {
                    "UNCHANGED"
                }
            }
            Change::Create => {
                if registry_changes {
                    "CREATE+REGISTER"
                } else {
                    "CREATE"
                }
            }
            Change::Replace(_) => {
                if registry_changes {
                    "REPLACE+REGISTER"
                } else {
                    "REPLACE"
                }
            }
            Change::Remove(_) => "REMOVE+UNREGISTER",
        };
        let prefix = if args.dry_run { "WOULD_" } else { "" };
        if self.mkdir {
            println!(
                "{prefix}MKDIR\t{:?}",
                self.link.parent().context("link parent")?
            );
        }
        println!("{prefix}{action}\t{:?}\t{:?}", self.link, self.entry.target);
        if args.dry_run {
            r.preview_bytes(self.after.as_bytes())?;
        } else {
            let operation = match self.change {
                Change::Create => Some((Operation::Create, None)),
                Change::Replace(old) => Some((Operation::Replace, Some(old))),
                Change::Remove(old) => Some((Operation::Remove, Some(old))),
                Change::Keep(_) => None,
            };
            if let Some((op, old)) = operation {
                let pending = transaction::begin(
                    r,
                    op,
                    self.entry.clone(),
                    self.link.clone(),
                    old,
                    self.after,
                    Request::from(args),
                )?;
                transaction::finish(r, &pending)
                    .context("operation incomplete; repeat this command to recover")?;
            } else if registry_changes {
                r.save_bytes(self.after.as_bytes())?;
            }
        }
        if args.command != Command::Remove {
            output::warn_target(&self.link, &self.entry.target);
        }
        Ok(())
    }
}

fn plan_link(r: &Registry, args: &Args, entry: Entry, after: String) -> Result<Plan> {
    let link = r.link(&entry)?;
    r.validate_destination(&link)?;
    ensure_separate(r, &link)?;
    paths::reject_self_reference(&link, &entry.target)?;
    let old = inspect::snapshot(&link)?;
    let change = match old {
        Some(old) if paths::target_matches(&link, &old.target, &entry.target)? => Change::Keep(Some(old)),
        Some(old) if args.force => Change::Replace(old),
        Some(old) => bail!("target differs at {link:?}: actual {:?}, requested {:?}; use --force (-f) to replace this symlink", old.target, entry.target),
        None => Change::Create,
    };
    let mut mkdir = false;
    if matches!(change, Change::Create | Change::Replace(_)) {
        let parent = link.parent().context("link parent")?;
        paths::directory_location(parent)?;
        mkdir = !parent.is_dir();
        if mkdir && !args.parents {
            bail!("parent directory is missing; use --parents (-p): {parent:?}");
        }
    }
    Ok(Plan {
        entry,
        link,
        change,
        after,
        mkdir,
    })
}

fn plan_create(r: &Registry, args: &Args) -> Result<Plan> {
    let link = paths::link_from_cli(&args.operands[1])?;
    let entry = Entry {
        link: paths::text(&link)?.to_owned(),
        target: paths::target_from_cli(&args.operands[0])?,
    };
    let after = r.render(&r.with_entry(&entry)?);
    plan_link(r, args, entry, after)
}

fn plan_adopt(r: &Registry, link: PathBuf) -> Result<Plan> {
    r.validate_destination(&link)?;
    ensure_separate(r, &link)?;
    let old = inspect::snapshot(&link)?.context("no symlink to adopt")?;
    let entry = Entry {
        link: paths::text(&link)?.to_owned(),
        target: paths::reference_target(&link, &old.target)?,
    };
    let after = r.render(&r.with_entry(&entry)?);
    Ok(Plan {
        entry,
        link,
        change: Change::Keep(Some(old)),
        after,
        mkdir: false,
    })
}

fn plan_remove(r: &Registry, args: &Args, entry: Entry) -> Result<Plan> {
    let link = r.link(&entry)?;
    let after = r.render(&r.without(r.find(&link)?.context("not registered")?));
    let change = if args.keep_link {
        Change::Keep(None)
    } else {
        r.validate_destination(&link)?;
        match inspect::snapshot(&link)? {
            Some(old) => {
                if !paths::target_matches(&link, &old.target, &entry.target)? {
                    bail!("MISMATCH: link left untouched; use --keep-link to unregister only");
                }
                Change::Remove(old)
            }
            None => Change::Keep(None),
        }
    };
    Ok(Plan {
        entry,
        link,
        change,
        after,
        mkdir: false,
    })
}

fn mutate_many(r: &mut Registry, args: &Args, recovered: Option<&Path>) -> Result<u8> {
    // A recovered removal may no longer be registered. Exclude it before lookup.
    let operands = args
        .operands
        .iter()
        .filter(|s| !paths::link_from_cli(s).is_ok_and(|p| Some(p.as_path()) == recovered))
        .cloned()
        .collect::<Vec<_>>();
    if !args.operands.is_empty() && operands.is_empty() {
        return Ok(0);
    }
    let entries = if args.command == Command::Adopt {
        vec![]
    } else {
        selected(r, &operands)?
    };
    let mut paths_to_visit = if args.command == Command::Adopt {
        operands
            .iter()
            .map(|s| paths::link_from_cli(s))
            .collect::<Result<Vec<_>>>()?
    } else {
        entries
            .iter()
            .map(|e| r.link(e))
            .collect::<Result<Vec<_>>>()?
    };
    paths_to_visit.retain(|p| Some(p.as_path()) != recovered);
    let mut seen = HashSet::new();
    let mut failed = false;
    for link in paths_to_visit {
        if !seen.insert(paths::key(&link).unwrap_or_else(|_| link.to_string_lossy().into_owned())) {
            continue;
        }
        let plan = match args.command {
            Command::Adopt => plan_adopt(r, link.clone()),
            Command::Fix | Command::Remove => {
                let entry = r.entries[r.find(&link)?.context("not registered")?].clone();
                if args.command == Command::Fix {
                    plan_link(
                        r,
                        args,
                        entry,
                        String::from_utf8(r.original.clone().context("missing registry")?)?,
                    )
                } else {
                    plan_remove(r, args, entry)
                }
            }
            _ => unreachable!("validated mutation command"),
        };
        if let Err(error) = plan.and_then(|p| p.apply(r, args)) {
            eprintln!("ERROR\t{link:?}\t{error:#}");
            failed = true;
        }
        if !args.dry_run && transaction::load(r)?.is_some() {
            eprintln!("PENDING: stopped subsequent changes; repeat the operation for the failed link to recover");
            break;
        }
    }
    Ok(u8::from(failed))
}
