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

pub fn expand_registry_home(s: &str) -> Result<Option<PathBuf>> {
    if s == "${HOME}" {
        Ok(Some(home()?))
    } else if let Some(rest) = s.strip_prefix("${HOME}/") {
        Ok(Some(home()?.join(rest)))
    } else {
        Ok(None)
    }
}

pub fn registry_link_path(s: &str) -> Result<PathBuf> {
    validate_target(s)?;
    let path = expand_registry_home(s)?.unwrap_or_else(|| PathBuf::from(s));
    if !path.is_absolute() {
        bail!("registry link must be an absolute path: {s:?}");
    }
    Ok(normalize(&path))
}

// Keep one deterministic spelling for syntax that does not change target
// meaning. Do not resolve '..' or inspect the filesystem here.
pub fn canonical_target(target: &str) -> Result<String> {
    validate_target(target)?;
    let prefix_len = target.as_bytes().iter().take_while(|&&b| b == b'/').count();
    let prefix = &target[..prefix_len];
    let body = &target[prefix_len..];
    let directory_suffix = target.ends_with('/') || target.ends_with("/.");

    let parts = body
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();

    let mut out = if prefix_len > 0 {
        prefix.to_owned()
    } else {
        String::new()
    };
    for part in parts {
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(part);
    }

    if out.is_empty() {
        out = if prefix_len > 0 {
            prefix.to_owned()
        } else if directory_suffix {
            "./".to_owned()
        } else {
            ".".to_owned()
        };
    } else if directory_suffix && !out.ends_with('/') {
        out.push('/');
    }

    Ok(out)
}

pub fn is_reserved_home_target(target: &str) -> bool {
    !Path::new(target).is_absolute() && (target == "${HOME}" || target.starts_with("${HOME}/"))
}

pub fn registry_target(s: &str) -> Result<String> {
    if let Some(expanded) = expand_registry_home(s)? {
        return canonical_target(text(&expanded)?);
    }
    let target = canonical_target(s)?;
    if is_reserved_home_target(&target) {
        bail!("relative target {target:?} conflicts with reserved registry HOME syntax");
    }
    Ok(target)
}

// readlink() is OS data: a literal '~' is not a home-directory abbreviation.
// Resolve only the link's containing directory, never the target symlinks.
pub fn canonical_reference(link: &Path, target: &str) -> Result<String> {
    let target = canonical_target(target)?;
    let path = if Path::new(&target).is_absolute() {
        PathBuf::from(&target)
    } else {
        directory_location(link.parent().context("link has no parent")?)?.join(&target)
    };
    Ok(text(&normalize(&path))?.to_owned())
}

pub fn target_matches(link: &Path, actual: &str, expected: &str) -> Result<bool> {
    Ok(canonical_reference(link, actual)? == canonical_reference(link, expected)?)
}

fn relative_target(link: &Path, absolute_reference: &str) -> Result<String> {
    let target = Path::new(absolute_reference);
    if !target.is_absolute() {
        bail!("relative target generation needs an absolute reference");
    }
    let base = directory_location(link.parent().context("link has no parent")?)?;
    let base_components = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let common = base_components
        .iter()
        .zip(&target_components)
        .take_while(|(a, b)| a == b)
        .count();

    if common == 0 {
        bail!("cannot represent target safely as a relative symlink");
    }

    let mut candidate = PathBuf::new();
    for component in &base_components[common..] {
        if matches!(component, Component::Normal(_)) {
            candidate.push("..");
        } else {
            bail!("cannot represent target safely as a relative symlink");
        }
    }
    for component in &target_components[common..] {
        candidate.push(component.as_os_str());
    }
    if candidate.as_os_str().is_empty() {
        candidate.push(".");
    }

    let mut candidate = text(&candidate)?.to_owned();
    if absolute_reference.ends_with('/') && !candidate.ends_with('/') {
        candidate.push('/');
    }
    canonical_target(&candidate)
}

pub fn materialize_create_target(link: &Path, operand: &str, relative: bool) -> Result<String> {
    let absolute_path = from_cli(operand)?;
    let absolute = canonical_target(text(&absolute_path)?)?;
    if !relative {
        return Ok(absolute);
    }
    let candidate = relative_target(link, &absolute)?;
    if is_reserved_home_target(&candidate) {
        bail!(
            "cannot represent target relatively: relative target {candidate:?} conflicts with reserved registry HOME syntax; omit --relative to use an absolute target"
        );
    }
    let parent = link.parent().context("link has no parent")?;
    if parent.is_dir()
        && canonical_reference(link, &candidate)? != canonical_reference(link, &absolute)?
    {
        bail!("cannot represent target safely as a relative symlink");
    }
    // A missing parent cannot round-trip through canonical_reference(): its
    // conservative normalization intentionally preserves '..' across missing
    // components. relative_target() uses directory_location(), whose missing
    // suffix is restricted to ordinary names, so the candidate becomes valid
    // when create -p materializes those directories. Without -p, planning
    // still rejects the missing parent before any symlink is created.
    Ok(candidate)
}

pub fn adopt_target(link: &Path, raw: &str) -> Result<String> {
    let target = canonical_target(raw)?;
    if is_reserved_home_target(&target) {
        canonical_reference(link, &target)
    } else {
        Ok(target)
    }
}

pub fn serialize_home_absolute(path: &Path, expression: bool) -> Result<String> {
    if expression {
        let home = home()?;
        if path == home {
            return Ok("${HOME}".to_owned());
        }
        if let Ok(rest) = path.strip_prefix(&home) {
            let rest = text(rest)?;
            if !rest.is_empty() {
                return Ok(format!("${{HOME}}/{rest}"));
            }
        }
    }
    Ok(text(path)?.to_owned())
}

pub fn reject_self_reference(link: &Path, target: &str) -> Result<()> {
    if let Ok(target_key) = canonical_reference(link, target).and_then(|t| key(Path::new(&t))) {
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

#[cfg(test)]
mod tests {
    use super::canonical_target;

    #[test]
    fn canonical_target_removes_only_meaningless_spelling() {
        for (input, expected) in [
            ("./foo", "foo"),
            ("foo/./bar", "foo/bar"),
            ("foo//bar", "foo/bar"),
            ("foo/.", "foo/"),
            ("foo/./", "foo/"),
            ("a/../b", "a/../b"),
            ("../a/./b", "../a/b"),
            (".", "."),
            ("./", "./"),
            ("//server//a/./b/.", "//server/a/b/"),
        ] {
            let canonical = canonical_target(input).unwrap();
            assert_eq!(canonical, expected, "input={input:?}");
            assert_eq!(
                canonical_target(&canonical).unwrap(),
                expected,
                "idempotence input={input:?}"
            );
        }
    }
}
