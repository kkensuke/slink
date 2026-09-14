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
pub fn default_registry_path() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        Some(root) => root,
        None => home()?.join(".config"),
    };
    Ok(root.join("slink/links.toml"))
}
// Preserve symlink references and directory-only suffixes. Only collapse a
// parent component when the preceding object is known to be an ordinary dir.
// Missing/inaccessible components and symlink/.. retain their OS meaning.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in p.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir
                if out.file_name().is_some()
                    && std::fs::symlink_metadata(&out).is_ok_and(|m| m.is_dir()) =>
            {
                out.pop();
            }
            Component::ParentDir if out == Path::new("/") => {}
            _ => out.push(component.as_os_str()),
        }
    }
    let bytes = p.as_os_str().as_encoded_bytes();
    if out != Path::new("/") {
        if bytes.ends_with(b"/") {
            out.push("");
        } else if bytes.ends_with(b"/.") {
            out.push(".");
        }
    }
    out
}

pub fn absolute(p: &Path) -> Result<PathBuf> {
    Ok(normalize(&if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    }))
}

pub fn from_cli(s: &str) -> Result<PathBuf> {
    validate_target(s)?;
    let path = if s == "~" {
        home()?
    } else if let Some(rest) = s.strip_prefix("~/") {
        home()?.join(rest)
    } else {
        PathBuf::from(s)
    };
    absolute(&path)
}

pub fn registry_path(s: &str, field: &str) -> Result<PathBuf> {
    validate_target(s)?;
    if !Path::new(s).is_absolute() {
        let example = from_cli(s)?;
        bail!("registry {field} must be an absolute path: {s:?}; replace it with an absolute path such as {example:?} (using the current working directory for relative input)");
    }
    Ok(normalize(Path::new(s)))
}

pub fn target_from_cli(s: &str) -> Result<String> {
    Ok(text(&from_cli(s)?)?.to_owned())
}

// readlink() is OS data: a literal '~' is not a home-directory abbreviation.
// Resolve only the link's containing directory, never the target symlinks.
pub fn reference_target(link: &Path, target: &str) -> Result<String> {
    validate_target(target)?;
    let path = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        directory_location(link.parent().context("link has no parent")?)?.join(target)
    };
    Ok(text(&normalize(&path))?.to_owned())
}

pub fn target_matches(link: &Path, actual: &str, expected: &str) -> Result<bool> {
    Ok(reference_target(link, actual)? == reference_target(link, expected)?)
}

pub fn reject_self_reference(link: &Path, target: &str) -> Result<()> {
    if let Ok(target_key) = key(Path::new(target)) {
        if key(link)? == target_key {
            bail!("target refers directly to the link itself: {link:?}");
        }
    }
    Ok(())
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
    let link = from_cli(s)?;
    validate_link(text(&link)?)?;
    Ok(link)
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

pub fn target_path(link: &Path, target: &str) -> PathBuf {
    if Path::new(target).is_absolute() {
        target.into()
    } else {
        link.parent().expect("validated link").join(target)
    }
}
