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
    pub fn code(&self) -> &'static str {
        match self {
            Self::Reachable => "REACHABLE",
            Self::Missing => "MISSING",
            Self::ResolutionError(_) => "RESOLUTION_ERROR",
            Self::Unknown(_) => "UNKNOWN",
        }
    }

    pub fn is_reachable(&self) -> bool {
        matches!(self, Self::Reachable)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::ResolutionError(reason) | Self::Unknown(reason) => Some(reason),
            Self::Reachable | Self::Missing => None,
        }
    }
}

#[derive(Debug)]
pub enum LinkState {
    Match(String),
    Missing,
    Mismatch(String),
    Conflict(&'static str),
    Unknown(String),
}

impl LinkState {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Match(_) => "MATCH",
            Self::Missing => "MISSING",
            Self::Mismatch(_) => "MISMATCH",
            Self::Conflict(_) => "CONFLICT",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

#[derive(Debug)]
pub struct Diagnosis {
    pub link: LinkState,
    pub expected_health: TargetHealth,
    pub actual_health: Option<TargetHealth>,
}

impl Diagnosis {
    pub fn is_healthy(&self) -> bool {
        matches!(self.link, LinkState::Match(_))
            && matches!(self.expected_health, TargetHealth::Reachable)
            && self
                .actual_health
                .as_ref()
                .is_some_and(TargetHealth::is_reachable)
    }

    pub fn actual(&self) -> Option<(&str, &TargetHealth)> {
        match &self.link {
            LinkState::Match(actual) | LinkState::Mismatch(actual) => Some((
                actual,
                self.actual_health.as_ref().expect("observed target health"),
            )),
            _ => None,
        }
    }

    pub fn inspection_failed(&self) -> bool {
        matches!(self.link, LinkState::Unknown(_))
            || self.expected_health.is_unknown()
            || self
                .actual_health
                .as_ref()
                .is_some_and(TargetHealth::is_unknown)
    }
}

pub fn snapshot(p: &Path) -> Result<Option<Snapshot>> {
    let m = match fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !m.file_type().is_symlink() {
        bail!("expected a symlink: {p:?}");
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

pub fn target_health(p: &Path) -> TargetHealth {
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

pub fn diagnose(p: &Path, expected: &str) -> Diagnosis {
    diagnose_observation(p, expected, snapshot(p))
}

pub fn diagnose_snapshot(p: &Path, expected: &str, actual: Snapshot) -> Diagnosis {
    diagnose_observation(p, expected, Ok(Some(actual)))
}

fn diagnose_observation(p: &Path, expected: &str, actual: Result<Option<Snapshot>>) -> Diagnosis {
    let link = match &actual {
        Ok(Some(s)) => match paths::target_matches(p, &s.target, expected) {
            Ok(true) => LinkState::Match(s.target.clone()),
            Ok(false) => LinkState::Mismatch(s.target.clone()),
            Err(error) => LinkState::Unknown(error.to_string()),
        },
        Ok(None) => LinkState::Missing,
        Err(error) => match fs::symlink_metadata(p) {
            Ok(metadata) if !metadata.file_type().is_symlink() => {
                LinkState::Conflict(if metadata.is_file() {
                    "file"
                } else if metadata.is_dir() {
                    "directory"
                } else {
                    "filesystem object"
                })
            }
            _ => LinkState::Unknown(error.to_string()),
        },
    };
    let expected_health = target_health(&paths::target_path(p, expected));
    let actual_health = match &actual {
        Ok(Some(snapshot)) => Some(target_health(&paths::target_path(p, &snapshot.target))),
        _ => None,
    };
    Diagnosis {
        link,
        expected_health,
        actual_health,
    }
}
