use crate::{
    cli::{Args, Command, HELP},
    inspect::{self, Snapshot},
    output::{self, MutationAction, MutationOutput, MutationResult},
    paths,
    registry::{Entry, Registry},
    transaction::{self, Operation, Request},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub fn run(args: Args) -> Result<u8> {
    match args.command {
        Command::Config => {
            outln!("{}", paths::text(&paths::default_registry_path()?)?);
            return Ok(0);
        }
        Command::Help => {
            out!("{HELP}");
            return Ok(0);
        }
        Command::Version => {
            outln!("slink {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        Command::List => {
            output::list(&Registry::read_entries()?, args.output_format);
            return Ok(0);
        }
        _ => {}
    }
    let mut r = Registry::open(matches!(
        args.command,
        Command::Create | Command::Adopt | Command::Scan
    ))?;
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
    let mut report = MutationOutput::new(args.command, args.dry_run);
    let mut recovered = None;
    if let Some(p) = transaction::load(&r)? {
        let same_request = p.request == Request::from(&args);
        let same_selection = if args.command == Command::Create {
            let link = paths::link_from_cli(&args.operands[1])?;
            link == p.link
                && paths::materialize_create_target(&link, &args.operands[0], args.relative)?
                    == p.entry.target
        } else {
            (args.command == Command::Fix && args.operands.is_empty())
                || args
                    .operands
                    .iter()
                    .any(|s| paths::link_from_cli(s).is_ok_and(|x| x == p.link))
        };
        if !same_request || !same_selection {
            bail!("incomplete operation; repeat the original operation with the same target and options before making other changes");
        }
        let recovery = if args.dry_run {
            transaction::preview(&mut r, &p)
        } else {
            transaction::finish(&mut r, &p).context("recovery stopped; pending operation retained")
        };
        if let Err(error) = recovery {
            report.failure(&p.link, &error);
            report.finish();
            return Ok(2);
        }
        report.record(MutationResult {
            action: match p.op {
                Operation::Create => MutationAction::Create,
                Operation::Replace => MutationAction::Replace,
                Operation::Remove => MutationAction::Remove,
            },
            link: p.link.clone(),
            target: p.entry.target,
            parent: None,
            recovered: true,
        });
        if args.command == Command::Create {
            return Ok(report.finish());
        }
        recovered = Some(p.link);
    }
    if args.command == Command::Create {
        match plan_create(&r, &args).and_then(|plan| plan.apply(&mut r, &args)) {
            Ok(result) => report.record(result),
            Err(error) => {
                let link = paths::from_cli(&args.operands[1])
                    .unwrap_or_else(|_| PathBuf::from(&args.operands[1]));
                report.failure(&link, &error);
            }
        }
    } else {
        mutate_many(&mut r, &args, recovered.as_deref(), &mut report)?;
    }
    Ok(report.finish())
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
    fn apply(self, r: &mut Registry, args: &Args) -> Result<MutationResult> {
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
                    MutationAction::Register
                } else {
                    MutationAction::Unchanged
                }
            }
            Change::Keep(None) => {
                if registry_changes {
                    MutationAction::Unregister
                } else {
                    MutationAction::Unchanged
                }
            }
            Change::Create => MutationAction::Create,
            Change::Replace(_) => MutationAction::Replace,
            Change::Remove(_) => MutationAction::Remove,
        };
        let parent = self
            .mkdir
            .then(|| self.link.parent().expect("validated parent").to_path_buf());
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
        Ok(MutationResult {
            action,
            link: self.link,
            target: self.entry.target,
            parent,
            recovered: false,
        })
    }
}

fn plan_link(r: &Registry, args: &Args, entry: Entry, after: String) -> Result<Plan> {
    let link = r.link(&entry)?;
    r.validate_destination(&link)?;
    ensure_separate(r, &link)?;
    paths::reject_self_reference(&link, &entry.target)?;
    let old = inspect::snapshot(&link)?;
    let change = match old {
        Some(old) if old.target == entry.target => Change::Keep(Some(old)),
        Some(old) if paths::target_matches(&link, &old.target, &entry.target)? => {
            if args.force {
                Change::Replace(old)
            } else {
                Change::Keep(Some(old))
            }
        }
        Some(old) if args.force => Change::Replace(old),
        Some(old) => bail!(inspect::TargetMismatch {
            expected: entry.target,
            actual: old.target,
        }),
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
        target: paths::materialize_create_target(&link, &args.operands[0], args.relative)?,
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
        target: paths::adopt_target(&link, &old.target)?,
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

fn plan_unregister(r: &Registry, entry: Entry) -> Result<Plan> {
    let link = r.link(&entry)?;
    let after = r.render(&r.without(r.find(&link)?.context("not registered")?));
    Ok(Plan {
        entry,
        link,
        change: Change::Keep(None),
        after,
        mkdir: false,
    })
}

fn plan_remove(r: &Registry, entry: Entry) -> Result<Plan> {
    let link = r.link(&entry)?;
    let after = r.render(&r.without(r.find(&link)?.context("not registered")?));
    r.validate_destination(&link)?;
    let change = match inspect::snapshot(&link)? {
        Some(old) => {
            if !paths::target_matches(&link, &old.target, &entry.target)? {
                bail!("target differs; link left untouched; use unregister to remove only the registration");
            }
            Change::Remove(old)
        }
        None => Change::Keep(None),
    };
    Ok(Plan {
        entry,
        link,
        change,
        after,
        mkdir: false,
    })
}

fn stable_path_key(path: &Path) -> String {
    paths::key(path).unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

fn planned_link_in_path(path: &Path, indexes: &HashMap<String, usize>) -> Option<(usize, PathBuf)> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    ancestors.into_iter().find_map(|ancestor| {
        indexes
            .get(&stable_path_key(ancestor))
            .copied()
            .map(|index| (index, ancestor.to_path_buf()))
    })
}

fn projected_fix_dependencies(
    r: &Registry,
    links: &[PathBuf],
    indexes: &HashMap<String, usize>,
    target: PathBuf,
) -> Result<Vec<usize>> {
    let mut current = paths::normalize(&target);
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();

    while let Some((dependency, prefix)) = planned_link_in_path(&current, indexes) {
        if !seen.insert(dependency) {
            break;
        }
        dependencies.push(dependency);
        let link = &links[dependency];
        let entry = &r.entries[r.find(link)?.context("not registered")?];
        let suffix = current
            .strip_prefix(&prefix)
            .expect("planned link is a path prefix");
        current = paths::normalize(&paths::target_path(link, &entry.target).join(suffix));
    }

    Ok(dependencies)
}

fn order_fix_paths(r: &Registry, mut links: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    links.retain(|link| seen.insert(stable_path_key(link)));
    links.sort_by_key(|link| stable_path_key(link));

    let indexes = links
        .iter()
        .enumerate()
        .map(|(index, link)| (stable_path_key(link), index))
        .collect::<HashMap<_, _>>();
    let mut dependencies = vec![Vec::new(); links.len()];
    for (index, link) in links.iter().enumerate() {
        let entry = &r.entries[r.find(link)?.context("not registered")?];
        let target = paths::target_path(link, &entry.target);
        dependencies[index] = projected_fix_dependencies(r, &links, &indexes, target)?;
    }

    fn visit(index: usize, dependencies: &[Vec<usize>], state: &mut [u8], order: &mut Vec<usize>) {
        if state[index] == 2 {
            return;
        }
        if state[index] == 1 {
            // A dependency cycle has no topological order. The outer stable
            // path ordering gives the cycle a deterministic fallback order.
            return;
        }
        state[index] = 1;
        for &dependency in &dependencies[index] {
            visit(dependency, dependencies, state, order);
        }
        state[index] = 2;
        order.push(index);
    }

    let mut state = vec![0; links.len()];
    let mut order = Vec::with_capacity(links.len());
    for index in 0..links.len() {
        visit(index, &dependencies, &mut state, &mut order);
    }
    Ok(order
        .into_iter()
        .map(|index| links[index].clone())
        .collect())
}

fn mutate_many(
    r: &mut Registry,
    args: &Args,
    recovered: Option<&Path>,
    report: &mut MutationOutput,
) -> Result<()> {
    // A recovered removal may no longer be registered. Exclude it before lookup.
    let operands = args
        .operands
        .iter()
        .filter(|s| !paths::link_from_cli(s).is_ok_and(|p| Some(p.as_path()) == recovered))
        .cloned()
        .collect::<Vec<_>>();
    if !args.operands.is_empty() && operands.is_empty() {
        return Ok(());
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
    if args.command == Command::Fix {
        paths_to_visit = order_fix_paths(r, paths_to_visit)?;
    }
    let mut seen = HashSet::new();
    for link in paths_to_visit {
        if !seen.insert(paths::key(&link).unwrap_or_else(|_| link.to_string_lossy().into_owned())) {
            continue;
        }
        let plan = match args.command {
            Command::Adopt => plan_adopt(r, link.clone()),
            Command::Fix | Command::Remove | Command::Unregister => {
                let entry = r.entries[r.find(&link)?.context("not registered")?].clone();
                if args.command == Command::Fix {
                    plan_link(
                        r,
                        args,
                        entry,
                        String::from_utf8(r.original.clone().context("missing registry")?)?,
                    )
                } else if args.command == Command::Remove {
                    plan_remove(r, entry)
                } else {
                    plan_unregister(r, entry)
                }
            }
            _ => unreachable!("validated mutation command"),
        };
        match plan.and_then(|p| p.apply(r, args)) {
            Ok(result) => report.record(result),
            Err(error) => report.failure(&link, &error),
        }
        if !args.dry_run && transaction::load(r)?.is_some() {
            eprintln!(
                "! incomplete operation\
  remaining links were not processed; repeat the operation for the failed link to recover"
            );
            break;
        }
    }
    Ok(())
}
