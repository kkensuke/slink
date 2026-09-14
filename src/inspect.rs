use crate::paths;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub dev: u64,
    pub ino: u64,
    pub target: String,
}

#[derive(Debug)]
pub enum TargetHealth {
    Reachable,
    Missing,
    ResolutionError(String),
    Unknown(String),
}

impl TargetHealth {
    fn code(&self) -> &'static str {
        match self {
            Self::Reachable => "REACHABLE",
            Self::Missing => "MISSING",
            Self::ResolutionError(_) => "RESOLUTION_ERROR",
            Self::Unknown(_) => "UNKNOWN",
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

    fn reason(&self) -> Option<&str> {
        match self {
            Self::ResolutionError(reason) | Self::Unknown(reason) => Some(reason),
            Self::Reachable | Self::Missing => None,
        }
    }
}

#[derive(Debug)]
pub enum LinkState {
    Match,
    Missing,
    Mismatch(String),
    Conflict(&'static str),
    Unknown(String),
}

#[derive(Debug)]
pub struct Diagnosis {
    link: LinkState,
    expected_health: TargetHealth,
    actual_health: Option<TargetHealth>,
}

impl Diagnosis {
    pub fn is_healthy(&self) -> bool {
        matches!(self.link, LinkState::Match)
            && matches!(self.expected_health, TargetHealth::Reachable)
    }

    pub fn print(&self, p: &Path, expected: &str) {
        let link = display_link(p);
        match &self.link {
            LinkState::Match => match &self.expected_health {
                TargetHealth::Reachable => {}
                TargetHealth::Missing => {
                    println!("! {link} — target is missing");
                    println!("  target: {}", display_text(expected));
                }
                TargetHealth::ResolutionError(reason) => {
                    println!("! {link} — target cannot be resolved");
                    println!("  target: {}", display_text(expected));
                    println!("  reason: {}", display_text(reason));
                }
                TargetHealth::Unknown(reason) => {
                    println!("! {link} — cannot inspect target");
                    println!("  target: {}", display_text(expected));
                    println!("  reason: {}", display_text(reason));
                }
            },
            LinkState::Missing => {
                println!("! {link} — link is missing");
                print_target("target", expected, &self.expected_health);
            }
            LinkState::Mismatch(actual) => {
                println!("! {link} — target differs");
                print_target("expected", expected, &self.expected_health);
                print_target(
                    "actual",
                    actual,
                    self.actual_health
                        .as_ref()
                        .expect("mismatch has actual target health"),
                );
            }
            LinkState::Conflict(kind) => {
                println!("! {link} — expected a symlink, found {kind}");
                print_target("target", expected, &self.expected_health);
            }
            LinkState::Unknown(reason) => {
                println!("! {link} — cannot inspect link");
                println!("  reason: {}", display_text(reason));
                print_target("target", expected, &self.expected_health);
            }
        }
    }
}

pub fn snapshot(p: &Path) -> Result<Option<Snapshot>> {
    let m = match fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !m.file_type().is_symlink() {
        bail!("CONFLICT: not a symlink: {p:?}");
    }
    let target = paths::text(&fs::read_link(p)?)?.to_owned();
    let after = fs::symlink_metadata(p)?;
    if m.dev() != after.dev()
        || m.ino() != after.ino()
        || m.ctime() != after.ctime()
        || m.ctime_nsec() != after.ctime_nsec()
    {
        bail!("link changed during inspection: {p:?}");
    }
    Ok(Some(Snapshot {
        dev: m.dev(),
        ino: m.ino(),
        target,
    }))
}

fn target_health(p: &Path) -> TargetHealth {
    match fs::metadata(p) {
        Ok(_) => TargetHealth::Reachable,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TargetHealth::Missing,
        Err(e)
            if e.raw_os_error() == Some(libc::ELOOP) || e.raw_os_error() == Some(libc::ENOTDIR) =>
        {
            TargetHealth::ResolutionError(e.to_string())
        }
        Err(e) => TargetHealth::Unknown(e.to_string()),
    }
}

pub fn health(p: &Path) -> &'static str {
    target_health(p).code()
}

pub fn diagnose(p: &Path, expected: &str) -> Diagnosis {
    let actual = snapshot(p);
    let link = match &actual {
        Ok(Some(s)) if s.target == expected => LinkState::Match,
        Ok(Some(s)) => LinkState::Mismatch(s.target.clone()),
        Ok(None) => LinkState::Missing,
        Err(error) => match fs::symlink_metadata(p) {
            Ok(metadata) if !metadata.file_type().is_symlink() => LinkState::Conflict(
                if metadata.is_file() {
                    "file"
                } else if metadata.is_dir() {
                    "directory"
                } else {
                    "filesystem object"
                },
            ),
            _ => LinkState::Unknown(error.to_string()),
        },
    };
    let expected_health = target_health(&paths::target_path(p, expected));
    let actual_health = match &link {
        LinkState::Mismatch(actual) => Some(target_health(&paths::target_path(p, actual))),
        _ => None,
    };
    Diagnosis {
        link,
        expected_health,
        actual_health,
    }
}

fn print_target(label: &str, target: &str, health: &TargetHealth) {
    println!(
        "  {label}: {}{}",
        display_text(target),
        health.annotation()
    );
    if let Some(reason) = health.reason() {
        println!("  {label} reason: {}", display_text(reason));
    }
}

pub fn display_link(p: &Path) -> String {
    let compact = paths::home().ok().and_then(|home| {
        if p == home {
            Some("~".to_owned())
        } else {
            p.strip_prefix(home)
                .ok()
                .and_then(|rest| paths::text(rest).ok())
                .map(|rest| format!("~/{rest}"))
        }
    });
    display_text(compact.as_deref().unwrap_or_else(|| p.to_str().unwrap_or("<non-UTF-8>")))
}

pub fn display_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

pub fn warn_target(p: &Path, target: &str) {
    let h = health(&paths::target_path(p, target));
    if h != "REACHABLE" {
        eprintln!("warning: target {h}: {p:?} -> {target:?}");
    }
}
