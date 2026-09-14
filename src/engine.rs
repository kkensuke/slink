use crate::{
    cli::{Args, Command, HELP},
    inspect, paths,
    registry::{Entry, Registry},
    transaction::{self, Operation},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub fn run(args: Args) -> Result<u8> {
    if args.command == Command::Help {
        print!("{HELP}");
        return Ok(0);
    }
    if args.command == Command::Version {
        println!("slink {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    let allow_missing = matches!(args.command, Command::Create | Command::Adopt)
        || (args.command == Command::Scan && args.file.is_none());
    let mut r = Registry::open(args.file.as_deref(), allow_missing)?;
    if args.command == Command::List {
        println!("LINK\tTARGET");
        for e in &r.entries {
            println!("{:?}\t{:?}", e.link, e.target);
        }
        return Ok(0);
    }
    if args.command == Command::Scan {
        return scan(&r, &args.operands);
    }
    let pending = transaction::load(&r)?;
    if args.command == Command::Check {
        let mut failed = pending.is_some();
        if let Some(p) = &pending {
            eprintln!(
                "PENDING: {:?} {:?} -> {:?}; repeat the operation for this link with the original options to recover",
                p.op, p.link, p.entry.target
            );
        }
        println!("LINK_STATE\tTARGET_STATE\tLINK\tTARGET");
        for e in selected(&r, &args.operands)? {
            failed |= !inspect::report(&r.link(&e)?, &e.target);
        }
        return Ok(u8::from(failed));
    }
    let _lock = if args.dry_run { None } else { Some(r.lock()?) };
    let pending = transaction::load(&r)?;
    if let Some(p) = pending {
        let wanted = match (args.command, p.op) {
            (Command::Create, Operation::Create) => {
                paths::link_from_cli(&args.operands[1])? == p.link
                    && create_target(&args, &p.link)? == p.entry.target
            }
            (Command::Remove, Operation::Remove) if !args.keep_link => {
                contains(&args.operands, &p.link)?
            }
            (Command::Fix, Operation::Replace) if args.replace => {
                args.operands.is_empty() || contains(&args.operands, &p.link)?
            }
            _ => false,
        };
        if !wanted || args.parents != p.parents {
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
            inspect::warn_target(&p.link, &p.entry.target);
        }
        if args.command == Command::Create {
            return Ok(0);
        }
        // The original removal may already be unregistered. Do not remove a new
        // object that appeared at its path after the interrupted operation.
        {
            let operands = if args.operands.is_empty() {
                r.entries
                    .iter()
                    .map(|e| Ok(paths::text(&r.link(e)?)?.to_owned()))
                    .collect::<Result<Vec<_>>>()?
            } else {
                args.operands.clone()
            };
            let rest = operands
                .iter()
                .filter(|s| paths::link_from_cli(s).ok().as_ref() != Some(&p.link))
                .cloned()
                .collect::<Vec<_>>();
            if rest.is_empty() {
                return Ok(0);
            }
            return mutate_many(&mut r, &args, &rest);
        }
    }
    if args.command == Command::Create {
        return match create(&mut r, &args) {
            Ok(()) => Ok(0),
            Err(e) => {
                eprintln!("slink: {e:#}");
                Ok(1)
            }
        };
    }
    mutate_many(&mut r, &args, &args.operands)
}

fn contains(input: &[String], p: &Path) -> Result<bool> {
    Ok(input
        .iter()
        .map(|s| paths::link_from_cli(s))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(|x| x == p))
}

fn selected(r: &Registry, operands: &[String]) -> Result<Vec<Entry>> {
    if operands.is_empty() {
        return Ok(r.entries.clone());
    }
    let mut selected = vec![];
    let mut seen = HashSet::new();
    for s in operands {
        let p = paths::link_from_cli(s)?;
        let i = r
            .find(&p)?
            .with_context(|| format!("not registered: {p:?}"))?;
        if seen.insert(i) {
            selected.push(r.entries[i].clone());
        }
    }
    Ok(selected)
}

fn create_target(a: &Args, link: &Path) -> Result<String> {
    let t = &a.operands[0];
    paths::validate_target(t)?;
    if a.relative {
        paths::relative_target(t, link)
    } else {
        Ok(t.clone())
    }
}

fn parents_plan(link: &Path, parents: bool, dry: bool) -> Result<()> {
    let parent = link.parent().context("link parent")?;
    paths::directory_location(parent)?;
    if !parent.is_dir() {
        if !parents {
            bail!("parent directory is missing; use --parents: {parent:?}");
        }
        println!("{}MKDIR\t{parent:?}", if dry { "WOULD_" } else { "" });
        if !dry {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn ensure_separate(r: &Registry, link: &Path) -> Result<()> {
    for e in &r.entries {
        if paths::overlaps(link, &r.link(e)?)? {
            bail!("duplicate or nested managed destination; manage the parent or its children: {link:?}");
        }
    }
    Ok(())
}

fn create(r: &mut Registry, a: &Args) -> Result<()> {
    let link = paths::link_from_cli(&a.operands[1])?;
    r.validate_destination(&link)?;
    let target = create_target(a, &link)?;
    if let Some(i) = r.find(&link)? {
        if r.entries[i].target == target
            && inspect::snapshot(&link)?.is_some_and(|s| s.target == target)
        {
            println!("UNCHANGED\t{link:?}");
            inspect::warn_target(&link, &target);
            return Ok(());
        }
        bail!("already registered with a different state; edit the registry or use fix");
    }
    ensure_separate(r, &link)?;
    if inspect::snapshot(&link)?.is_some() {
        bail!("existing symlink is unregistered; use adopt: {link:?}");
    }
    parents_plan(&link, a.parents, true)?;
    let entry = Entry {
        link: paths::text(&link)?.to_owned(),
        target,
    };
    println!(
        "{}CREATE+REGISTER\t{link:?}\t{:?}",
        if a.dry_run { "WOULD_" } else { "" },
        entry.target
    );
    if !a.dry_run {
        let after = r.render(&r.with_added(&entry));
        let p = transaction::begin(
            r,
            Operation::Create,
            entry.clone(),
            link.clone(),
            None,
            after,
            a.parents,
        )?;
        transaction::finish(r, &p)
            .context("creation incomplete; repeat this command to recover")?;
    }
    inspect::warn_target(&link, &entry.target);
    Ok(())
}

fn mutate_many(r: &mut Registry, a: &Args, operands: &[String]) -> Result<u8> {
    let entries = if a.command == Command::Adopt {
        vec![]
    } else {
        selected(r, operands)?
    };
    if a.command == Command::Fix {
        for (i, e) in entries.iter().enumerate() {
            for other in &entries[..i] {
                if paths::overlaps(&r.link(e)?, &r.link(other)?)? {
                    bail!("ambiguous or nested destinations; select one link at a time");
                }
            }
        }
    }
    let mut failed = false;
    let mut seen = HashSet::new();
    if a.command == Command::Adopt {
        for s in operands {
            let p = paths::link_from_cli(s)?;
            if !seen.insert(p.clone()) {
                continue;
            }
            if let Err(e) = adopt(r, a, &p) {
                eprintln!("ERROR\t{p:?}\t{e:#}");
                failed = true;
            }
            if !a.dry_run && transaction::load(r)?.is_some() {
                break;
            }
        }
    } else {
        for e in entries {
            let p = r.link(&e)?;
            let result = if a.command == Command::Fix {
                fix(r, a, &e, &p)
            } else {
                remove(r, a, &e, &p)
            };
            if let Err(err) = result {
                eprintln!("ERROR\t{p:?}\t{err:#}");
                failed = true;
            }
            if !a.dry_run && transaction::load(r)?.is_some() {
                eprintln!(
                    "PENDING: stopped subsequent changes; repeat the operation for the failed link to recover"
                );
                break;
            }
        }
    }
    Ok(u8::from(failed))
}

fn adopt(r: &mut Registry, a: &Args, p: &Path) -> Result<()> {
    r.validate_destination(p)?;
    let s = inspect::snapshot(p)?.context("no symlink to adopt")?;
    if let Some(i) = r.find(p)? {
        if r.entries[i].target != s.target {
            bail!("registered target differs; registry left unchanged");
        }
        println!("UNCHANGED\t{p:?}");
        return Ok(());
    }
    ensure_separate(r, p)?;
    let e = Entry {
        link: paths::text(p)?.to_owned(),
        target: s.target.clone(),
    };
    println!(
        "{}REGISTER\t{p:?}\t{:?}",
        if a.dry_run { "WOULD_" } else { "" },
        e.target
    );
    if inspect::snapshot(p)?.as_ref() != Some(&s) {
        bail!("symlink changed before registration");
    }
    let after = r.render(&r.with_added(&e));
    if a.dry_run {
        r.preview_bytes(after.as_bytes())?;
    } else {
        r.save_bytes(after.as_bytes())?;
    }
    inspect::warn_target(p, &e.target);
    Ok(())
}

fn fix(r: &mut Registry, a: &Args, e: &Entry, p: &Path) -> Result<()> {
    r.validate_destination(p)?;
    let old = inspect::snapshot(p)?;
    if let Some(s) = &old {
        if s.target == e.target {
            println!("UNCHANGED\t{p:?}");
            inspect::warn_target(p, &e.target);
            return Ok(());
        }
        if !a.replace {
            bail!(
                "MISMATCH: expected {:?}, actual {:?}; use --replace to change it",
                e.target,
                s.target
            );
        }
    }
    parents_plan(p, a.parents, true)?;
    println!(
        "{}{}\t{p:?}\t{:?}",
        if a.dry_run { "WOULD_" } else { "" },
        if old.is_some() { "REPLACE" } else { "CREATE" },
        e.target
    );
    if !a.dry_run {
        r.verify()?;
        if old.is_some() {
            let after = String::from_utf8(r.original.clone().context("missing registry")?)?;
            let pending = transaction::begin(
                r,
                Operation::Replace,
                e.clone(),
                p.to_path_buf(),
                old,
                after,
                a.parents,
            )?;
            transaction::finish(r, &pending)?;
        } else {
            parents_plan(p, a.parents, false)?;
            std::os::unix::fs::symlink(&e.target, p)?;
        }
    }
    inspect::warn_target(p, &e.target);
    Ok(())
}

fn remove(r: &mut Registry, a: &Args, e: &Entry, p: &Path) -> Result<()> {
    let i = r.find(p)?.context("not registered")?;
    let after = r.render(&r.without(i));
    if a.keep_link {
        println!("{}UNREGISTER\t{p:?}", if a.dry_run { "WOULD_" } else { "" });
        if a.dry_run {
            r.preview_bytes(after.as_bytes())?;
        } else {
            r.save_bytes(after.as_bytes())?;
        }
        return Ok(());
    }
    r.validate_destination(p)?;
    let old = inspect::snapshot(p)?;
    if old.as_ref().is_some_and(|s| s.target != e.target) {
        bail!("MISMATCH: link left untouched; use --keep-link to unregister only");
    }
    println!(
        "{}REMOVE+UNREGISTER\t{p:?}",
        if a.dry_run { "WOULD_" } else { "" }
    );
    if a.dry_run {
        r.preview_bytes(after.as_bytes())?;
    } else {
        if old.is_none() {
            r.save_bytes(after.as_bytes())?;
        } else {
            let pending = transaction::begin(
                r,
                Operation::Remove,
                e.clone(),
                p.to_path_buf(),
                old,
                after,
                a.parents,
            )?;
            transaction::finish(r, &pending)?;
        }
    }
    Ok(())
}

fn scan(r: &Registry, roots: &[String]) -> Result<u8> {
    let mut failed = false;
    let mut visited = HashSet::new();
    let mut stack: Vec<PathBuf> = roots
        .iter()
        .map(|s| paths::absolute(Path::new(s)))
        .collect::<Result<_>>()?;
    println!("MANAGEMENT\tTARGET_STATE\tLINK\tTARGET");
    while let Some(p) = stack.pop() {
        let result = (|| -> Result<()> {
            if !fs::symlink_metadata(&p)?.is_dir() {
                bail!("scan roots must be directories, not symlinks: {p:?}");
            }
            if !visited.insert(fs::canonicalize(&p)?) {
                return Ok(());
            }
            let mut children = fs::read_dir(&p)?.collect::<std::io::Result<Vec<_>>>()?;
            children.sort_by_key(|x| x.file_name());
            for child in children {
                let q = child.path();
                match child.file_type() {
                    Ok(t) if t.is_symlink() => match inspect::snapshot(&q) {
                        Ok(Some(s)) => println!(
                            "{}\t{}\t{q:?}\t{:?}",
                            if r.find(&q)?.is_some() {
                                "MANAGED"
                            } else {
                                "UNMANAGED"
                            },
                            inspect::health(&paths::target_path(&q, &s.target)),
                            s.target
                        ),
                        Ok(None) => {
                            failed = true;
                            eprintln!("CHANGED\t{q:?}");
                        }
                        Err(e) => {
                            failed = true;
                            eprintln!("ERROR\t{q:?}\t{e:#}");
                        }
                    },
                    Ok(t) if t.is_dir() => stack.push(q),
                    Ok(_) => {}
                    Err(e) => {
                        failed = true;
                        eprintln!("ERROR\t{q:?}\t{e}");
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            failed = true;
            eprintln!("ERROR\t{p:?}\t{e:#}");
        }
    }
    Ok(u8::from(failed))
}
