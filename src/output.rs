use crate::{
    cli::OutputFormat,
    inspect::{self, Diagnosis, TargetHealth},
    paths,
    registry::Registry,
};
use anyhow::Result;
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
};

pub fn list(r: &Registry, format: OutputFormat) -> Result<u8> {
    if format == OutputFormat::Tsv {
        println!("LINK\tTARGET");
        for entry in &r.entries {
            println!("{:?}\t{:?}", entry.link, entry.target);
        }
        return Ok(0);
    }

    let count = r.entries.len();
    println!("{count} {}", plural(count, "link", "links"));
    for entry in &r.entries {
        println!();
        println!("{}", inspect::display_link(&r.link(entry)?));
        println!("  → {}", inspect::display_text(&entry.target));
    }
    Ok(0)
}

struct ScanEntry {
    link: PathBuf,
    target: String,
    actual_health: TargetHealth,
    managed: Option<(String, Diagnosis)>,
}

impl ScanEntry {
    fn is_managed(&self) -> bool {
        self.managed.is_some()
    }

    fn has_issue(&self) -> bool {
        match &self.managed {
            Some((_, diagnosis)) => !diagnosis.is_healthy(),
            None => !self.actual_health.is_reachable(),
        }
    }
}

struct ScanError {
    path: PathBuf,
    reason: String,
}

pub fn scan(r: &Registry, roots: &[String], format: OutputFormat) -> Result<u8> {
    let (entries, errors) = collect_scan(r, roots)?;
    if format == OutputFormat::Tsv {
        print_scan_tsv(&entries, &errors);
    } else {
        print_scan_human(entries, &errors);
    }
    Ok(u8::from(!errors.is_empty()))
}

fn collect_scan(r: &Registry, roots: &[String]) -> Result<(Vec<ScanEntry>, Vec<ScanError>)> {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let mut visited = HashSet::new();
    let mut stack: Vec<PathBuf> = roots
        .iter()
        .map(|root| {
            // A trailing slash makes lstat follow the final symlink. Strip only
            // separators; keep '..' and intermediate symlinks in their OS order.
            let trimmed = root.trim_end_matches('/');
            let root = if !root.is_empty() && trimmed.is_empty() {
                "/"
            } else {
                trimmed
            };
            paths::absolute(Path::new(root))
        })
        .collect::<Result<_>>()?;

    while let Some(dir) = stack.pop() {
        let metadata = match fs::symlink_metadata(&dir) {
            Ok(metadata) => metadata,
            Err(error) => {
                errors.push(ScanError {
                    path: dir,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if !metadata.is_dir() {
            errors.push(ScanError {
                path: dir,
                reason: "scan roots must be directories, not symlinks".to_owned(),
            });
            continue;
        }

        let canonical = match fs::canonicalize(&dir) {
            Ok(path) => path,
            Err(error) => {
                errors.push(ScanError {
                    path: dir,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if !visited.insert(canonical) {
            continue;
        }

        let mut children = match fs::read_dir(&dir) {
            Ok(children) => match children.collect::<std::io::Result<Vec<_>>>() {
                Ok(children) => children,
                Err(error) => {
                    errors.push(ScanError {
                        path: dir,
                        reason: error.to_string(),
                    });
                    continue;
                }
            },
            Err(error) => {
                errors.push(ScanError {
                    path: dir,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        children.sort_by_key(|child| child.file_name());

        for child in children {
            let path = child.path();
            let file_type = match child.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    errors.push(ScanError {
                        path,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_symlink() {
                continue;
            }

            let snapshot = match inspect::snapshot(&path) {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    errors.push(ScanError {
                        path,
                        reason: "link changed during inspection".to_owned(),
                    });
                    continue;
                }
                Err(error) => {
                    errors.push(ScanError {
                        path,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            let actual_health =
                inspect::target_health(&paths::target_path(&path, &snapshot.target));
            let managed = match r.find(&path) {
                Ok(Some(index)) => {
                    let expected = r.entries[index].target.clone();
                    let diagnosis = inspect::diagnose(&path, &expected);
                    Some((expected, diagnosis))
                }
                Ok(None) => None,
                Err(error) => {
                    errors.push(ScanError {
                        path,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            entries.push(ScanEntry {
                link: path,
                target: snapshot.target,
                actual_health,
                managed,
            });
        }
    }

    Ok((entries, errors))
}

fn print_scan_tsv(entries: &[ScanEntry], errors: &[ScanError]) {
    println!("MANAGEMENT\tTARGET_STATE\tLINK\tTARGET");
    for entry in entries {
        println!(
            "{}\t{}\t{:?}\t{:?}",
            if entry.is_managed() {
                "MANAGED"
            } else {
                "UNMANAGED"
            },
            entry.actual_health.code(),
            entry.link,
            entry.target
        );
    }
    for error in errors {
        eprintln!("ERROR\t{:?}\t{}", error.path, error.reason);
    }
}

fn print_scan_human(mut entries: Vec<ScanEntry>, errors: &[ScanError]) {
    let total = entries.len();
    let managed_count = entries.iter().filter(|entry| entry.is_managed()).count();
    let unmanaged_count = total - managed_count;
    let issue_count = entries.iter().filter(|entry| entry.has_issue()).count();

    if total == 0 && errors.is_empty() {
        println!("No symlinks found.");
        return;
    }

    entries.sort_by(scan_order);
    let mut managed = Vec::new();
    let mut unmanaged = Vec::new();
    for entry in entries {
        if entry.is_managed() {
            managed.push(entry);
        } else {
            unmanaged.push(entry);
        }
    }

    let mut wrote_section = false;
    if !managed.is_empty() {
        println!("{}", heading(&format!("Managed ({})", managed.len())));
        print_scan_section(&managed);
        wrote_section = true;
    }
    if !unmanaged.is_empty() {
        if wrote_section {
            println!();
        }
        println!("{}", heading(&format!("Unmanaged ({})", unmanaged.len())));
        print_scan_section(&unmanaged);
        wrote_section = true;
    }
    if !errors.is_empty() {
        if wrote_section {
            println!();
        }
        println!("{}", heading(&format!("Scan errors ({})", errors.len())));
        for (index, error) in errors.iter().enumerate() {
            if index > 0 {
                println!();
            }
            println!(
                "  {} {} — cannot scan",
                problem_marker(),
                inspect::display_link(&error.path)
            );
            println!("    reason: {}", inspect::display_text(&error.reason));
        }
    }

    println!();
    println!(
        "{total} {} found: {managed_count} managed, {unmanaged_count} unmanaged, {issue_count} {}",
        plural(total, "symlink", "symlinks"),
        plural(issue_count, "link issue", "link issues")
    );
}

fn print_scan_section(entries: &[ScanEntry]) {
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            println!();
        }
        match &entry.managed {
            Some((_, diagnosis)) if diagnosis.is_healthy() => {
                println!("  {} {}", ok_marker(), inspect::display_link(&entry.link));
                println!("    → {}", inspect::display_text(&entry.target));
            }
            Some((expected, diagnosis)) => {
                let lines = diagnosis.render(&entry.link, expected);
                for (line_index, line) in lines.iter().enumerate() {
                    if line_index == 0 {
                        if let Some(rest) = line.strip_prefix("! ") {
                            println!("  {} {rest}", problem_marker());
                        } else {
                            println!("  {line}");
                        }
                    } else {
                        println!("  {line}");
                    }
                }
            }
            None => print_unmanaged(entry),
        }
    }
}

fn print_unmanaged(entry: &ScanEntry) {
    match entry.actual_health.problem_label() {
        None => {
            println!("  {} {}", ok_marker(), inspect::display_link(&entry.link));
            println!("    → {}", inspect::display_text(&entry.target));
        }
        Some(problem) => {
            println!(
                "  {} {} — {problem}",
                problem_marker(),
                inspect::display_link(&entry.link)
            );
            println!("    → {}", inspect::display_text(&entry.target));
            if let Some(reason) = entry.actual_health.reason() {
                println!("    reason: {}", inspect::display_text(reason));
            }
        }
    }
}

fn scan_order(a: &ScanEntry, b: &ScanEntry) -> Ordering {
    match (a.has_issue(), b.has_issue()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.link.cmp(&b.link),
    }
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn color_enabled() -> bool {
    io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").ok().as_deref() != Some("dumb")
}

fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_owned()
    }
}

fn ok_marker() -> String {
    paint("32", "✓")
}

fn problem_marker() -> String {
    paint("31", "!")
}

fn heading(text: &str) -> String {
    paint("1", text)
}
