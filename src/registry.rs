use crate::paths;
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub link: String,
    pub target: String,
}

pub struct Registry {
    pub requested: PathBuf,
    pub path: PathBuf,
    pub base: PathBuf,
    pub original: Option<Vec<u8>>,
    pub doc: DocumentMut,
    pub entries: Vec<Entry>,
}

impl Registry {
    pub fn open(file: Option<&Path>, allow_missing: bool) -> Result<Self> {
        let requested = if let Some(f) = file {
            paths::absolute(f)?
        } else {
            let root = match std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|x| x.is_absolute())
            {
                Some(root) => root,
                None => paths::home()?.join(".config"),
            };
            root.join("slink/links.toml")
        };
        let path = match fs::symlink_metadata(&requested) {
            Ok(_) => {
                let p = fs::canonicalize(&requested).context("cannot resolve registry")?;
                if !p.is_file() {
                    bail!("registry is not a regular file");
                }
                p
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => requested.clone(),
            Err(e) => return Err(e.into()),
        };
        let original = match fs::read(&path) {
            Ok(b) => Some(b),
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    && (allow_missing
                        || path
                            .with_file_name(format!(
                                "{}.slink-pending",
                                path.file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or_default()
                            ))
                            .is_file()) =>
            {
                None
            }
            Err(e) => return Err(e).with_context(|| format!("cannot read registry {requested:?}")),
        };
        Self::parse(requested, path, original)
    }
    fn parse(requested: PathBuf, path: PathBuf, original: Option<Vec<u8>>) -> Result<Self> {
        let doc = match &original {
            Some(b) => std::str::from_utf8(b)?
                .parse::<DocumentMut>()
                .context("invalid TOML")?,
            None => {
                let mut d = DocumentMut::new();
                d["version"] = value(1);
                d
            }
        };
        if doc.get("version").and_then(Item::as_integer) != Some(1) {
            bail!("registry version must be 1");
        }
        for (k, _) in doc.iter() {
            if k != "version" && k != "links" {
                bail!("unknown registry field {k:?}");
            }
        }
        let mut entries = vec![];
        if let Some(links) = doc.get("links") {
            for table in links
                .as_array_of_tables()
                .context("links must use [[links]] tables")?
            {
                if table.len() != 2 || !table.contains_key("link") || !table.contains_key("target")
                {
                    bail!("each [[links]] needs exactly link and target");
                }
                let link = table["link"]
                    .as_str()
                    .context("link must be a string")?
                    .to_owned();
                let target = table["target"]
                    .as_str()
                    .context("target must be a string")?
                    .to_owned();
                paths::validate_link(&link)?;
                paths::validate_target(&target)?;
                entries.push(Entry { link, target });
            }
        }
        let base = requested
            .parent()
            .context("registry has no parent")?
            .to_path_buf();
        let r = Self {
            requested,
            path,
            base,
            original,
            doc,
            entries,
        };
        let mut seen = HashSet::new();
        for e in &r.entries {
            if !seen.insert(r.link(e)?) {
                bail!("duplicate link: {:?}", e.link);
            }
        }
        Ok(r)
    }
    pub fn link(&self, entry: &Entry) -> Result<PathBuf> {
        paths::link_from_registry(&entry.link, &self.base)
    }
    pub fn find(&self, p: &Path) -> Result<Option<usize>> {
        for (i, e) in self.entries.iter().enumerate() {
            if self.link(e)? == p {
                return Ok(Some(i));
            }
        }
        let key = paths::key(p)?;
        for (i, e) in self.entries.iter().enumerate() {
            if paths::key(&self.link(e)?).is_ok_and(|k| k == key) {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }
    pub fn sidecar(&self, suffix: &str) -> Result<PathBuf> {
        Ok(self.path.with_file_name(format!(
            "{}.slink-{suffix}",
            self.path
                .file_name()
                .and_then(|s| s.to_str())
                .context("registry filename must be UTF-8")?
        )))
    }
    pub fn validate_destination(&self, link: &Path) -> Result<()> {
        for control in [
            self.requested.clone(),
            self.path.clone(),
            self.sidecar("lock")?,
            self.sidecar("pending")?,
        ] {
            if paths::overlaps(link, &control)? {
                bail!("destination overlaps the registry or its control files: {link:?}");
            }
        }
        Ok(())
    }
    pub fn lock(&self) -> Result<File> {
        fs::create_dir_all(self.path.parent().context("registry parent")?)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.sidecar("lock")?)?;
        f.try_lock_exclusive()
            .context("registry is busy; retry after the other slink command finishes")?;
        self.verify()?;
        Ok(f)
    }
    pub fn verify(&self) -> Result<()> {
        let current = match fs::read(&self.path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if current != self.original {
            bail!("registry changed concurrently; retry with the new contents");
        }
        if current.is_some() && fs::canonicalize(&self.requested)? != self.path {
            bail!("registry symlink changed concurrently");
        }
        Ok(())
    }
    pub fn with_added(&self, entry: &Entry) -> DocumentMut {
        let mut doc = self.doc.clone();
        if doc.get("links").is_none() {
            doc["links"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        let mut t = Table::new();
        t["link"] = value(&entry.link);
        t["target"] = value(&entry.target);
        doc["links"]
            .as_array_of_tables_mut()
            .expect("validated registry")
            .push(t);
        doc
    }
    pub fn without(&self, index: usize) -> DocumentMut {
        let mut doc = self.doc.clone();
        let arr = doc["links"]
            .as_array_of_tables_mut()
            .expect("validated registry");
        arr.remove(index);
        if arr.is_empty() {
            doc.remove("links");
        }
        doc
    }
    pub fn render(&self, doc: &DocumentMut) -> String {
        let text = doc.to_string();
        let crlf = self.original.as_ref().is_some_and(|b| {
            b.contains(&b'\n')
                && b.iter()
                    .enumerate()
                    .all(|(i, c)| *c != b'\n' || (i > 0 && b[i - 1] == b'\r'))
        });
        if crlf {
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            text
        }
    }
    pub fn save_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.verify()?;
        atomic_write(&self.path, bytes, self.original.is_some())?;
        // Keep exactly the file spelling selected by the user, including aliases.
        *self = Self::open(Some(&self.requested), false)?;
        Ok(())
    }
    // Advance a dry-run's registry in memory so later items see earlier plans.
    pub fn preview_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        *self = Self::parse(
            self.requested.clone(),
            self.path.clone(),
            Some(bytes.to_vec()),
        )?;
        Ok(())
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path.parent().context("file has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if replace {
        temp.as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
    }
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if replace {
        temp.persist(path)?;
    } else {
        temp.persist_noclobber(path)?;
    }
    File::open(parent)?.sync_all()?;
    Ok(())
}
