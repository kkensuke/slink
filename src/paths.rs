use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

pub fn text(p: &Path) -> Result<&str> {
    p.to_str().context("path is not valid UTF-8")
}
pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .context("HOME must be an absolute directory")
}
pub fn absolute(p: &Path) -> Result<PathBuf> {
    Ok(if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    })
}
pub fn validate_link(s: &str) -> Result<()> {
    if s.is_empty()
        || s.contains('\0')
        || s.ends_with('/')
        || s.rsplit('/').next().is_some_and(|v| v == "." || v == "..")
    {
        bail!("invalid link name {s:?}");
    }
    Ok(())
}
pub fn validate_target(s: &str) -> Result<()> {
    if s.is_empty() || s.contains('\0') {
        bail!("target must be nonempty and contain no NUL");
    }
    Ok(())
}
pub fn link_from_cli(s: &str) -> Result<PathBuf> {
    validate_link(s)?;
    absolute(Path::new(s))
}
pub fn link_from_registry(s: &str, base: &Path) -> Result<PathBuf> {
    validate_link(s)?;
    if let Some(rest) = s.strip_prefix("~/") {
        return Ok(home()?.join(rest));
    }
    Ok(if Path::new(s).is_absolute() {
        s.into()
    } else {
        base.join(s)
    })
}

// Resolve existing directories without collapsing .. across a symlink. A missing
// suffix may contain only normal names: future traversal through .. is ambiguous.
pub fn directory_location(p: &Path) -> Result<PathBuf> {
    match std::fs::canonicalize(p) {
        Ok(x) => {
            if !x.is_dir() {
                bail!("not a directory: {p:?}");
            }
            Ok(x)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(p).is_ok() {
                bail!("unresolvable parent: {p:?}");
            }
            if text(p)?.rsplit('/').any(|x| x == "..") {
                bail!("cannot resolve missing parent containing '..': {p:?}");
            }
            let name = p.file_name().context("parent has no filename")?;
            Ok(directory_location(p.parent().context("parent has no ancestor")?)?.join(name))
        }
        Err(e) => Err(e).with_context(|| format!("cannot resolve parent {p:?}")),
    }
}

#[cfg(target_os = "macos")]
fn case_sensitive(p: &Path) -> Result<bool> {
    use std::os::fd::AsRawFd;
    let mut p = p.to_path_buf();
    while !p.exists() {
        p = p.parent().context("no existing ancestor")?.to_path_buf();
    }
    let f = std::fs::File::open(p)?;
    let n = unsafe { libc::fpathconf(f.as_raw_fd(), libc::_PC_CASE_SENSITIVE) };
    match n {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("cannot determine filesystem case sensitivity"),
    }
}
#[cfg(not(target_os = "macos"))]
fn case_sensitive(_: &Path) -> Result<bool> {
    Ok(true)
}

pub fn key(p: &Path) -> Result<String> {
    let parent = directory_location(p.parent().context("link has no parent")?)?;
    let joined = parent.join(p.file_name().context("link has no filename")?);
    let mut s = text(&joined)?.to_owned();
    if cfg!(target_os = "macos") {
        s = s.nfd().collect();
    }
    if !case_sensitive(&parent)? {
        s = s.to_lowercase();
    }
    Ok(s)
}

// Include both physical destinations and the directories traversed to reach
// them: canonicalizing a symlink parent alone hides a parent/child dependency.
pub fn overlaps(a: &Path, b: &Path) -> Result<bool> {
    let ak = key(a)?;
    let bk = key(b)?;
    if ak == bk || ak.starts_with(&(bk.clone() + "/")) || bk.starts_with(&(ak.clone() + "/")) {
        return Ok(true);
    }
    for (child, parent_key) in [(a, &bk), (b, &ak)] {
        for parent in child
            .ancestors()
            .skip(1)
            .filter(|p| p.file_name().is_some())
        {
            if key(parent)? == *parent_key {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn relative_target(target: &str, link: &Path) -> Result<String> {
    validate_target(target)?;
    let target = absolute(Path::new(target))?;
    let base = directory_location(link.parent().context("link has no parent")?)?;
    let b: Vec<_> = base.components().collect();
    let t: Vec<_> = target.components().collect();
    let common = b
        .iter()
        .zip(&t)
        .take_while(|(a, b)| a == b && !matches!(a, Component::ParentDir))
        .count();
    let mut out = PathBuf::new();
    for _ in &b[common..] {
        out.push("..");
    }
    // Keep target-side symlinks and ParentDir components as a reference path.
    for c in &t[common..] {
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    let mut result = text(&out)?.to_owned();
    if target.as_os_str().as_encoded_bytes().ends_with(b"/") {
        result.push('/');
    } else if target.as_os_str().as_encoded_bytes().ends_with(b"/.") {
        result.push_str("/.");
    }
    Ok(result)
}

pub fn target_path(link: &Path, target: &str) -> PathBuf {
    if Path::new(target).is_absolute() {
        target.into()
    } else {
        link.parent().expect("validated link").join(target)
    }
}
