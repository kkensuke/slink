use crate::paths;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot { pub dev: u64, pub ino: u64, pub target: String }

pub fn snapshot(p: &Path) -> Result<Option<Snapshot>> {
    let m = match fs::symlink_metadata(p) { Ok(m) => m, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(e) => return Err(e.into()) };
    if !m.file_type().is_symlink() { bail!("CONFLICT: not a symlink: {p:?}"); }
    let target = paths::text(&fs::read_link(p)?)?.to_owned();
    let after = fs::symlink_metadata(p)?;
    if m.dev() != after.dev() || m.ino() != after.ino() || m.ctime() != after.ctime() || m.ctime_nsec() != after.ctime_nsec() { bail!("link changed during inspection: {p:?}"); }
    Ok(Some(Snapshot { dev: m.dev(), ino: m.ino(), target }))
}

pub fn health(p: &Path) -> &'static str {
    match fs::metadata(p) {
        Ok(_) => "REACHABLE",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "MISSING",
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) || e.raw_os_error() == Some(libc::ENOTDIR) => "RESOLUTION_ERROR",
        Err(_) => "UNKNOWN",
    }
}

pub fn report(p: &Path, expected: &str) -> bool {
    let actual = snapshot(p);
    let state = match &actual {
        Ok(Some(s)) if s.target == expected => "MATCH",
        Ok(Some(_)) => "MISMATCH",
        Ok(None) => "MISSING",
        Err(_) => match fs::symlink_metadata(p) { Ok(m) if !m.file_type().is_symlink() => "CONFLICT", _ => "UNKNOWN" },
    };
    let target_state = health(&paths::target_path(p, expected));
    println!("{state}\t{target_state}\t{p:?}\t{expected:?}");
    match actual {
        Ok(Some(s)) if s.target != expected => println!("  actual: {:?}\t{}", s.target, health(&paths::target_path(p, &s.target))),
        Err(e) => eprintln!("  {e:#}"),
        _ => {}
    }
    state == "MATCH" && target_state == "REACHABLE"
}

pub fn warn_target(p: &Path, target: &str) {
    let h = health(&paths::target_path(p, target));
    if h != "REACHABLE" { eprintln!("warning: target {h}: {p:?} -> {target:?}"); }
}
