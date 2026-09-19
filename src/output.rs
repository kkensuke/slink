use crate::{
    cli::OutputFormat,
    inspect::{self, Diagnosis, LinkState, TargetHealth},
    paths,
    registry::{Entry, Registry},
    transaction::Pending,
};
use anyhow::Result;
use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

mod mutation;
pub use mutation::{MutationAction, MutationOutput, MutationResult};

static STDOUT_ERROR: std::sync::OnceLock<io::Error> = std::sync::OnceLock::new();

pub fn write_stdout(args: std::fmt::Arguments<'_>) {
    if STDOUT_ERROR.get().is_none() {
        if let Err(error) = io::stdout().lock().write_fmt(args) {
            let _ = STDOUT_ERROR.set(error);
        }
    }
}

pub fn finish_stdout() -> Result<()> {
    if STDOUT_ERROR.get().is_none() {
        if let Err(error) = io::stdout().lock().flush() {
            let _ = STDOUT_ERROR.set(error);
        }
    }
    if let Some(error) = STDOUT_ERROR.get() {
        if error.kind() != io::ErrorKind::BrokenPipe {
            anyhow::bail!("cannot write stdout: {error}");
        }
    }
    Ok(())
}

pub fn list(entries: &[Entry], format: OutputFormat) {
    if format == OutputFormat::Tsv {
        outln!("LINK\tTARGET");
        for entry in entries {
            outln!("{}\t{}", quoted(&entry.link), quoted(&entry.target));
        }
        return;
    }

    let count = entries.len();
    outln!("{count} {}", plural(count, "link", "links"));
    for entry in entries {
        outln!();
        outln!("{}", display_path(&entry.link));
        outln!("  → {}", display_path(&entry.target));
    }
}

pub fn check(
    checked: &[(PathBuf, String, Diagnosis)],
    pending: Option<&Pending>,
    format: OutputFormat,
) {
    if format == OutputFormat::Tsv {
        if let Some(p) = pending {
            eprintln!(
                "PENDING: {:?} {} -> {}; repeat the original operation to recover",
                p.op,
                display_path(&p.link),
                display_path(&p.entry.target)
            );
        }
        outln!("LINK\tLINK_STATE\tTARGET\tTARGET_STATE\tACTUAL_TARGET\tACTUAL_TARGET_STATE");
        for (link, target, diagnosis) in checked {
            let actual = diagnosis.actual();
            outln!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                display_path(link),
                diagnosis.link.code(),
                display_path(target),
                diagnosis.expected_health.code(),
                actual.map_or_else(String::new, |(target, _)| quoted(target)),
                actual.map_or("", |(_, health)| health.code()),
            );
            print_diagnostic_reasons(link, diagnosis);
        }
        return;
    }

    let problems: Vec<_> = checked.iter().filter(|(_, _, d)| !d.is_healthy()).collect();
    let count = problems.len() + usize::from(pending.is_some());
    if count == 0 {
        outln!(
            "OK {} {}",
            checked.len(),
            plural(checked.len(), "link", "links")
        );
        return;
    }
    outln!(
        "{count} {} found ({} {} checked)",
        plural(count, "problem", "problems"),
        checked.len(),
        plural(checked.len(), "link", "links")
    );
    if let Some(p) = pending {
        outln!();
        outln!("! incomplete operation");
        outln!("  operation: {}", format!("{:?}", p.op).to_lowercase());
        outln!("  link: {}", display_path(&p.link));
        outln!("  target: {}", display_path(&p.entry.target));
        outln!("  repeat the original command with the same target and options to recover");
    }
    for (link, target, diagnosis) in problems {
        outln!();
        for line in render_diagnosis(diagnosis, link, target) {
            outln!("{line}");
        }
    }
}

fn print_diagnostic_reasons(link: &Path, diagnosis: &Diagnosis) {
    let link_reason = match &diagnosis.link {
        LinkState::Unknown(reason) => Some(reason.clone()),
        LinkState::Conflict(kind) => Some(format!("expected a symlink, found {kind}")),
        _ => None,
    };
    if let Some(reason) = link_reason {
        eprintln!("ERROR\t{}\t{}", display_path(link), quoted(&reason));
    }
    for (label, health) in [
        ("expected target", Some(&diagnosis.expected_health)),
        ("actual target", diagnosis.actual_health.as_ref()),
    ] {
        if let Some(reason) = health.and_then(TargetHealth::reason) {
            eprintln!("ERROR\t{}\t{label}\t{}", display_path(link), quoted(reason));
        }
    }
}

struct ScanEntry {
    link: PathBuf,
    target: String,
    health: ScanHealth,
}

enum ScanHealth {
    Managed {
        expected: String,
        diagnosis: Diagnosis,
    },
    Unmanaged(TargetHealth),
}

impl ScanEntry {
    fn is_managed(&self) -> bool {
        matches!(self.health, ScanHealth::Managed { .. })
    }

    fn has_issue(&self) -> bool {
        match &self.health {
            ScanHealth::Managed { diagnosis, .. } => !diagnosis.is_healthy(),
            ScanHealth::Unmanaged(health) => !health.is_reachable(),
        }
    }

    fn actual_health(&self) -> &TargetHealth {
        match &self.health {
            ScanHealth::Managed { diagnosis, .. } => diagnosis
                .actual_health
                .as_ref()
                .expect("scanned symlink health"),
            ScanHealth::Unmanaged(health) => health,
        }
    }

    fn inspection_failed(&self) -> bool {
        match &self.health {
            ScanHealth::Managed { diagnosis, .. } => diagnosis.inspection_failed(),
            ScanHealth::Unmanaged(health) => health.is_unknown(),
        }
    }
}

struct ScanError {
    path: PathBuf,
    reason: String,
}

pub fn scan(r: &Registry, roots: &[String], recursive: bool, format: OutputFormat) -> Result<u8> {
    let (entries, errors) = collect_scan(r, roots, recursive)?;
    let failed = !errors.is_empty() || entries.iter().any(ScanEntry::inspection_failed);
    if format == OutputFormat::Tsv {
        print_scan_tsv(&entries, &errors);
    } else {
        print_scan_human(entries, &errors);
    }
    Ok(u8::from(failed))
}

fn collect_scan(
    r: &Registry,
    roots: &[String],
    recursive: bool,
) -> Result<(Vec<ScanEntry>, Vec<ScanError>)> {
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
            paths::from_cli(root)
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
                if recursive {
                    stack.push(path);
                }
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
            let target = snapshot.target.clone();
            let health = match r.find(&path) {
                Ok(Some(index)) => {
                    let expected = r.entries[index].target.clone();
                    let diagnosis = inspect::diagnose_snapshot(&path, &expected, snapshot);
                    ScanHealth::Managed {
                        expected,
                        diagnosis,
                    }
                }
                Ok(None) => ScanHealth::Unmanaged(inspect::target_health(&paths::target_path(
                    &path, &target,
                ))),
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
                target,
                health,
            });
        }
    }

    Ok((entries, errors))
}

fn print_scan_tsv(entries: &[ScanEntry], errors: &[ScanError]) {
    outln!(
        "MANAGEMENT\tLINK\tLINK_STATE\tTARGET\tTARGET_STATE\tACTUAL_TARGET\tACTUAL_TARGET_STATE"
    );
    let mut entries = entries.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| a.link.cmp(&b.link));
    for entry in entries {
        let (link_state, target, target_state) = match &entry.health {
            ScanHealth::Managed {
                expected,
                diagnosis,
            } => (
                diagnosis.link.code(),
                display_path(expected),
                diagnosis.expected_health.code(),
            ),
            ScanHealth::Unmanaged(_) => ("", String::new(), ""),
        };
        outln!(
            "{}\t{}\t{link_state}\t{target}\t{target_state}\t{}\t{}",
            if entry.is_managed() {
                "MANAGED"
            } else {
                "UNMANAGED"
            },
            display_path(&entry.link),
            quoted(&entry.target),
            entry.actual_health().code(),
        );
        match &entry.health {
            ScanHealth::Managed { diagnosis, .. } => {
                print_diagnostic_reasons(&entry.link, diagnosis)
            }
            ScanHealth::Unmanaged(health) => {
                if let Some(reason) = health.reason() {
                    eprintln!("ERROR\t{}\t{}", display_path(&entry.link), quoted(reason));
                }
            }
        }
    }
    for error in errors {
        eprintln!(
            "ERROR\t{}\t{}",
            display_path(&error.path),
            quoted(&error.reason)
        );
    }
}

fn print_scan_human(mut entries: Vec<ScanEntry>, errors: &[ScanError]) {
    let total = entries.len();
    let managed_count = entries.iter().filter(|entry| entry.is_managed()).count();
    let unmanaged_count = total - managed_count;
    let issue_count = entries.iter().filter(|entry| entry.has_issue()).count();

    if total == 0 && errors.is_empty() {
        outln!("No symlinks found.");
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
        outln!("{}", heading(&format!("Managed ({})", managed.len())));
        print_scan_section(&managed);
        wrote_section = true;
    }
    if !unmanaged.is_empty() {
        if wrote_section {
            outln!();
        }
        outln!("{}", heading(&format!("Unmanaged ({})", unmanaged.len())));
        print_scan_section(&unmanaged);
        wrote_section = true;
    }
    if !errors.is_empty() {
        if wrote_section {
            outln!();
        }
        outln!("{}", heading(&format!("Scan errors ({})", errors.len())));
        for (index, error) in errors.iter().enumerate() {
            if index > 0 {
                outln!();
            }
            outln!(
                "  {} {} — cannot scan",
                problem_marker(),
                display_path(&error.path)
            );
            outln!("    reason: {}", display_text(&error.reason));
        }
    }

    outln!();
    outln!(
        "{total} {} found: {managed_count} managed, {unmanaged_count} unmanaged, {issue_count} {}",
        plural(total, "symlink", "symlinks"),
        plural(issue_count, "link issue", "link issues")
    );
}

fn print_scan_section(entries: &[ScanEntry]) {
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            outln!();
        }
        match &entry.health {
            ScanHealth::Managed { diagnosis, .. } if diagnosis.is_healthy() => {
                outln!("  {} {}", ok_marker(), display_path(&entry.link));
                outln!("    → {}", display_path(&entry.target));
            }
            ScanHealth::Managed {
                expected,
                diagnosis,
            } => {
                let lines = render_diagnosis(diagnosis, &entry.link, expected);
                for (line_index, line) in lines.iter().enumerate() {
                    if line_index == 0 {
                        if let Some(rest) = line.strip_prefix("! ") {
                            outln!("  {} {rest}", problem_marker());
                        } else {
                            outln!("  {line}");
                        }
                    } else {
                        outln!("  {line}");
                    }
                }
            }
            ScanHealth::Unmanaged(_) => print_unmanaged(entry),
        }
    }
}

fn print_unmanaged(entry: &ScanEntry) {
    match entry.actual_health().problem_label() {
        None => {
            outln!("  {} {}", ok_marker(), display_path(&entry.link));
            outln!("    → {}", display_path(&entry.target));
        }
        Some(problem) => {
            outln!(
                "  {} {} — {problem}",
                problem_marker(),
                display_path(&entry.link)
            );
            outln!("    → {}", display_path(&entry.target));
            if let Some(reason) = entry.actual_health().reason() {
                outln!("    reason: {}", display_text(reason));
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

impl TargetHealth {
    pub fn problem_label(&self) -> Option<&'static str> {
        match self {
            Self::Reachable => None,
            Self::Missing => Some("target is missing"),
            Self::ResolutionError(_) => Some("target cannot be resolved"),
            Self::Unknown(_) => Some("cannot inspect target"),
        }
    }

    fn annotation(&self) -> &'static str {
        match self {
            Self::Reachable => "",
            Self::Missing => " (missing)",
            Self::ResolutionError(_) => " (cannot be resolved)",
            Self::Unknown(_) => " (cannot be inspected)",
        }
    }
}

fn render_diagnosis(diagnosis: &Diagnosis, p: &Path, expected: &str) -> Vec<String> {
    let link = display_path(p);
    let mut lines = Vec::new();
    match &diagnosis.link {
        LinkState::Match(_) => {
            let health = diagnosis
                .actual_health
                .as_ref()
                .filter(|h| !h.is_reachable())
                .unwrap_or(&diagnosis.expected_health);
            match health {
                TargetHealth::Reachable => {}
                TargetHealth::Missing => {
                    lines.push(format!("! {link} — target is missing"));
                    lines.push(format!("  target: {}", display_path(expected)));
                }
                TargetHealth::ResolutionError(reason) => {
                    lines.push(format!("! {link} — target cannot be resolved"));
                    lines.push(format!("  target: {}", display_path(expected)));
                    lines.push(format!("  reason: {}", display_text(reason)));
                }
                TargetHealth::Unknown(reason) => {
                    lines.push(format!("! {link} — cannot inspect target"));
                    lines.push(format!("  target: {}", display_path(expected)));
                    lines.push(format!("  reason: {}", display_text(reason)));
                }
            }
        }
        LinkState::Missing => {
            lines.push(format!("! {link} — link is missing"));
            push_target(&mut lines, "target", expected, &diagnosis.expected_health);
        }
        LinkState::Mismatch(actual) => {
            lines.push(format!("! {link} — target differs"));
            push_target(&mut lines, "expected", expected, &diagnosis.expected_health);
            push_target(
                &mut lines,
                "actual",
                actual,
                diagnosis
                    .actual_health
                    .as_ref()
                    .expect("mismatch has actual target health"),
            );
        }
        LinkState::Conflict(kind) => {
            lines.push(format!("! {link} — expected a symlink, found {kind}"));
            push_target(&mut lines, "target", expected, &diagnosis.expected_health);
        }
        LinkState::Unknown(reason) => {
            lines.push(format!("! {link} — cannot inspect link"));
            lines.push(format!("  reason: {}", display_text(reason)));
            push_target(&mut lines, "target", expected, &diagnosis.expected_health);
        }
    }
    lines
}

fn target_line(label: &str, target: &str) -> String {
    let separator = if label == "actual" { ":   " } else { ": " };
    format!("  {label}{separator}{}", display_path(target))
}

fn push_target(lines: &mut Vec<String>, label: &str, target: &str, health: &TargetHealth) {
    lines.push(format!(
        "{}{}",
        target_line(label, target),
        health.annotation()
    ));
    if let Some(reason) = health.reason() {
        lines.push(format!("  {label} reason: {}", display_text(reason)));
    }
}

// Display cleanup is lexical: never resolve '..', expand '~', or inspect the
// filesystem. Preserve leading separators and directory-only suffixes.
fn clean_display_path(text: &str) -> String {
    let prefix_len = text.len() - text.trim_start_matches('/').len();
    let mut out = text[..prefix_len].to_owned();
    let mut parts = text[prefix_len..].split('/').peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() || (part == "." && !out.is_empty() && parts.peek().is_some()) {
            continue;
        }
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(part);
    }
    if text.ends_with('/') && !out.is_empty() && !out.ends_with('/') {
        out.push('/');
    }
    out
}

fn display_path(path: impl AsRef<Path>) -> String {
    let text = path.as_ref().to_str().unwrap_or("<non-UTF-8>");
    quoted(&clean_display_path(text))
}

pub fn display_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() || c == '\\' {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

fn quoted(text: &str) -> String {
    let json = serde_json::to_string(text).expect("string serialization");
    let mut escaped = String::new();
    for c in json.chars() {
        // JSON permits DEL and C1 controls; terminals should never receive them.
        if c.is_control() {
            escaped.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            escaped.push(c);
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{clean_display_path, display_path};
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

    #[test]
    fn path_display_preserves_path_boundaries_and_directory_suffixes() {
        for (input, expected) in [
            ("/Users/example/.zshenv", "/Users/example/.zshenv"),
            ("/a/./b", "/a/b"),
            ("/a//./b", "/a/b"),
            ("/a/./b/", "/a/b/"),
            ("/a/./b/.", "/a/b/."),
            ("/a/b///", "/a/b/"),
            ("/a/b/./", "/a/b/"),
            ("/a/../b", "/a/../b"),
            ("/../b", "/../b"),
            ("../a/./b", "../a/b"),
            ("./a/./b", "./a/b"),
            ("~/a/./b", "~/a/b"),
            ("//server//a/./b/.", "//server/a/b/."),
            ("///a//b", "///a/b"),
            ("/", "/"),
            ("//", "//"),
            ("///", "///"),
            ("/.", "/."),
            ("/./", "/"),
            (".", "."),
            ("./", "./"),
            ("././", "./"),
            ("", ""),
            (" a ", " a "),
            ("a/./.", "a/."),
            ("a/.//", "a/"),
            ("./.", "./."),
            ("//./", "//"),
        ] {
            assert_eq!(clean_display_path(input), expected, "input={input:?}");
            assert_eq!(clean_display_path(expected), expected, "input={input:?}");
            let displayed = display_path(input);
            assert_eq!(
                serde_json::from_str::<String>(&displayed).unwrap(),
                expected,
                "input={input:?}"
            );
        }
    }

    #[test]
    fn path_display_quotes_names_and_escapes_terminal_controls() {
        let name = " 日本語 \"\\\t\n\r\0\u{1b}\u{7f}\u{9b} ";
        let displayed = display_path(name);
        assert!(displayed.starts_with('"') && displayed.ends_with('"'));
        assert!(!displayed.chars().any(char::is_control));
        assert_eq!(serde_json::from_str::<String>(&displayed).unwrap(), name);
        assert!(displayed.contains("日本語"));
    }

    #[test]
    fn path_display_keeps_the_non_utf8_placeholder() {
        let path = Path::new(OsStr::from_bytes(b"/invalid-\xff"));
        assert_eq!(display_path(path), "\"<non-UTF-8>\"");
    }
}
