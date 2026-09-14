use super::{display_link, display_text, ok_marker, paint, plural, problem_marker, quoted};
use crate::{cli::Command, inspect, paths};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MutationAction {
    Create,
    Replace,
    Register,
    Unregister,
    Remove,
    Unchanged,
}

impl MutationAction {
    fn label(self, dry_run: bool, recovered: bool) -> String {
        let (done, planned, recovery) = match self {
            Self::Create => ("created", "create", "creation"),
            Self::Replace => ("replaced", "replace", "replacement"),
            Self::Register => ("registered", "register", "registration"),
            Self::Unregister => ("unregistered", "unregister", "unregistration"),
            Self::Remove => ("removed", "remove", "removal"),
            Self::Unchanged => return "unchanged".into(),
        };
        if recovered {
            format!(
                "{} {recovery}",
                if dry_run {
                    "would recover"
                } else {
                    "recovered"
                }
            )
        } else if dry_run {
            format!("would {planned}")
        } else {
            done.into()
        }
    }
}

pub struct MutationResult {
    pub action: MutationAction,
    pub link: PathBuf,
    pub target: String,
    pub parent: Option<PathBuf>,
    pub recovered: bool,
}

pub struct MutationOutput {
    dry_run: bool,
    fixing: bool,
    changed: usize,
    unchanged: usize,
    failed: usize,
    target_issues: usize,
    printed: bool,
    deferred: Vec<MutationResult>,
}

impl MutationOutput {
    pub fn new(command: Command, dry_run: bool) -> Self {
        Self {
            dry_run,
            fixing: command == Command::Fix,
            changed: 0,
            unchanged: 0,
            failed: 0,
            target_issues: 0,
            printed: false,
            deferred: Vec::new(),
        }
    }

    // Results arrive only after execution (or preview) succeeds. Target health
    // is advisory and does not turn a completed mutation into a failed one.
    // Fix output is deferred so health describes the final state, not a
    // transient state between dependent link mutations.
    pub fn record(&mut self, result: MutationResult) {
        if result.action == MutationAction::Unchanged {
            self.unchanged += 1;
        } else {
            self.changed += 1;
        }
        if self.fixing {
            self.deferred.push(result);
            return;
        }
        let health = observed_health(&result);
        self.print_result(&result, health);
    }

    fn print_result(&mut self, result: &MutationResult, health: Option<inspect::TargetHealth>) {
        let problem = health.as_ref().and_then(|h| h.problem_label());
        if problem.is_some() {
            self.target_issues += 1;
        }
        if self.fixing && result.action == MutationAction::Unchanged && problem.is_none() {
            return;
        }
        if self.printed {
            outln!();
        }
        self.printed = true;
        let marker = if problem.is_some() {
            problem_marker()
        } else if self.dry_run {
            paint("36", "○")
        } else {
            ok_marker()
        };
        let label = result.action.label(self.dry_run, result.recovered);
        let warning = problem.map_or(String::new(), |p| format!("; {p}"));
        outln!("{marker} {} — {label}{warning}", display_link(&result.link));
        outln!("  → {}", quoted(&result.target));
        if let Some(parent) = &result.parent {
            outln!(
                "  parent: {} ({})",
                display_link(parent),
                if self.dry_run {
                    "would create"
                } else {
                    "created"
                }
            );
        }
        if let Some(reason) = health.as_ref().and_then(|h| h.reason()) {
            outln!("  reason: {}", display_text(reason));
        }
    }

    pub fn failure(&mut self, link: &Path, error: &anyhow::Error) {
        self.failed += 1;
        eprintln!("! {} — failed", display_link(link));
        eprintln!("  reason: {}\n", display_text(&format!("{error:#}")));
    }

    pub fn finish(&mut self) -> u8 {
        if self.fixing {
            let results = std::mem::take(&mut self.deferred);
            for result in &results {
                let health = if self.dry_run {
                    projected_health_for(result, &results)
                } else {
                    observed_health(result)
                };
                self.print_result(result, health);
            }
        }
        if self.fixing || self.changed + self.unchanged + self.failed > 1 {
            if self.printed {
                outln!();
            }
            let changes = if self.dry_run {
                format!(
                    "{} {}",
                    self.changed,
                    plural(self.changed, "change planned", "changes planned")
                )
            } else {
                format!("{} changed", self.changed)
            };
            let failures = if self.failed > 0 {
                format!(", {} failed", self.failed)
            } else {
                String::new()
            };
            let warnings = if self.target_issues > 0 {
                format!(
                    ", {} {}",
                    self.target_issues,
                    plural(self.target_issues, "target issue", "target issues")
                )
            } else {
                String::new()
            };
            outln!(
                "{changes}, {} unchanged{failures}{warnings}",
                self.unchanged
            );
        }
        u8::from(self.failed > 0)
    }
}

fn observed_health(result: &MutationResult) -> Option<inspect::TargetHealth> {
    if matches!(
        result.action,
        MutationAction::Remove | MutationAction::Unregister
    ) {
        None
    } else {
        Some(inspect::target_health(&paths::target_path(
            &result.link,
            &result.target,
        )))
    }
}

fn projected_health_for(
    result: &MutationResult,
    results: &[MutationResult],
) -> Option<inspect::TargetHealth> {
    if matches!(
        result.action,
        MutationAction::Remove | MutationAction::Unregister
    ) {
        return None;
    }
    let target = paths::target_path(&result.link, &result.target);
    Some(projected_health(&target, results, &mut HashSet::new()))
}

fn projected_health(
    path: &Path,
    results: &[MutationResult],
    visiting: &mut HashSet<String>,
) -> inspect::TargetHealth {
    let key = paths::key(path).unwrap_or_else(|_| path.to_string_lossy().into_owned());
    let planned = results.iter().find(|result| {
        !matches!(
            result.action,
            MutationAction::Remove | MutationAction::Unregister
        ) && paths::key(&result.link).unwrap_or_else(|_| result.link.to_string_lossy().into_owned())
            == key
    });
    let Some(result) = planned else {
        return inspect::target_health(path);
    };
    if !visiting.insert(key.clone()) {
        return inspect::TargetHealth::ResolutionError("symlink loop in planned fix".into());
    }
    let target = paths::target_path(&result.link, &result.target);
    let health = projected_health(&target, results, visiting);
    visiting.remove(&key);
    health
}
