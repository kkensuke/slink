use crate::{inspect::{snapshot, Snapshot}, registry::{atomic_write, Entry, Registry}};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, os::unix::{ffi::OsStrExt, fs::OpenOptionsExt}, path::{Path, PathBuf}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation { Create, Remove, Replace }

#[derive(Debug, Serialize, Deserialize)]
pub struct Pending {
    version: u8,
    pub op: Operation,
    pub entry: Entry,
    pub link: PathBuf,
    pub parents: bool,
    before: Option<String>,
    after: String,
    old: Option<Snapshot>,
    backup: Option<PathBuf>,
}

pub fn load(r: &Registry) -> Result<Option<Pending>> {
    let mut f = match fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(r.sidecar("pending")?) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("cannot read pending operation"),
    };
    let mut data = vec![]; f.read_to_end(&mut data)?;
    let p: Pending = serde_json::from_slice(&data).context("invalid pending operation; preserve this file for recovery")?;
    if p.version != 1 || !p.link.is_absolute() || p.entry.target.is_empty() { bail!("invalid pending operation"); }
    if let Some(b) = &p.backup {
        let dir = b.parent().context("invalid backup path")?;
        if b.file_name().and_then(|s| s.to_str()) != Some("old") || dir.parent() != p.link.parent() || !dir.file_name().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with(".slink-")) { bail!("invalid pending backup path"); }
    }
    Ok(Some(p))
}

pub fn begin(r: &Registry, op: Operation, entry: Entry, link: PathBuf, old: Option<Snapshot>, after: String, parents: bool) -> Result<Pending> {
    r.verify()?;
    let backup = if old.is_some() {
        let dir = tempfile::Builder::new().prefix(".slink-").tempdir_in(link.parent().context("link parent")?)?.keep();
        Some(dir.join("old"))
    } else { None };
    let pending = Pending { version: 1, op, entry, link, parents, before: r.original.as_ref().map(|v| String::from_utf8(v.clone())).transpose()?, after, old, backup };
    let data = serde_json::to_vec_pretty(&pending)?;
    if let Err(e) = atomic_write(&r.sidecar("pending")?, &data, false) {
        if let Some(b) = &pending.backup { let _ = fs::remove_dir(b.parent().expect("backup parent")); }
        return Err(e);
    }
    Ok(pending)
}

// Both native implementations refuse an occupied destination. There is no
// remove-then-rename fallback, which could destroy a concurrently created file.
fn move_exclusive(from: &Path, to: &Path) -> Result<()> {
    let from = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let result = unsafe { libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result = { bail!("this platform is unsupported"); };
    if result != 0 { return Err(std::io::Error::last_os_error()).context("exclusive move failed"); }
    Ok(())
}

fn sync_parent(p: &Path) -> Result<()> { fs::File::open(p.parent().context("missing parent")?)?.sync_all()?; Ok(()) }

pub fn finish(r: &mut Registry, p: &Pending) -> Result<()> {
    let current = r.original.as_deref();
    if current != p.before.as_deref().map(str::as_bytes) && current != Some(p.after.as_bytes()) { bail!("registry differs from the pending operation; preserve the pending file and resolve the conflict"); }
    if let (Some(old), Some(backup)) = (&p.old, &p.backup) {
        match snapshot(backup)? {
            Some(s) if &s == old => {},
            Some(_) => bail!("unexpected backup; left untouched at {backup:?}"),
            None => {
                let original = if current == Some(p.after.as_bytes()) && p.op == Operation::Remove { None } else { snapshot(&p.link)? };
                if original.as_ref() == Some(old) {
                    move_exclusive(&p.link, backup)?;
                    sync_parent(&p.link)?; sync_parent(backup)?;
                    if !snapshot(backup).is_ok_and(|s| s.as_ref() == Some(old)) {
                        let _ = move_exclusive(backup, &p.link);
                        bail!("link changed during move; preserve recovery directory {:?}", backup.parent());
                    }
                } else {
                    let already_done = current == Some(p.after.as_bytes()) && match p.op {
                        Operation::Remove => true,
                        Operation::Replace => original.as_ref().is_some_and(|s| s.target == p.entry.target),
                        Operation::Create => false,
                    };
                    if !already_done { bail!("old link and backup differ from the pending operation; no changes made"); }
                }
            }
        }
    }
    if p.op != Operation::Remove {
        if p.parents { fs::create_dir_all(p.link.parent().context("link parent")?)?; }
        match snapshot(&p.link)? {
            None => { std::os::unix::fs::symlink(&p.entry.target, &p.link)?; sync_parent(&p.link)?; }
            Some(s) if s.target == p.entry.target => {},
            Some(_) => bail!("destination changed; pending operation retained"),
        }
    }
    if r.original.as_deref() != Some(p.after.as_bytes()) { r.save_bytes(p.after.as_bytes())?; }
    if let (Some(old), Some(backup)) = (&p.old, &p.backup) {
        if let Some(s) = snapshot(backup)? {
            if &s != old { bail!("unexpected backup; left untouched at {backup:?}"); }
            fs::remove_file(backup)?; sync_parent(backup)?;
        }
        match fs::remove_dir(backup.parent().context("backup parent")?) {
            Ok(()) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(e).context("recovery directory is not empty; left untouched"),
        }
    }
    fs::remove_file(r.sidecar("pending")?)?;
    sync_parent(&r.path)?;
    Ok(())
}
