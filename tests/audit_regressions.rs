use fs2::FileExt;
use std::os::unix::fs::symlink;
use std::{fs, process::Command};

struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir(root.join("home")).unwrap();
        Self { _dir: dir, root }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_slink"));
        command
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .arg("--file")
            .arg(self.root.join("links.toml"))
            .args(args);
        command
    }
}

#[test]
fn real_parent_creation_is_not_reported_as_would_mkdir() {
    let f = Fixture::new();
    let output = f
        .command(&["--parents", "future", "missing/link"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("MKDIR"), "stdout={stdout:?}");
    assert!(!stdout.contains("WOULD_MKDIR"), "stdout={stdout:?}");
    assert!(f.root.join("missing").is_dir());
}

#[test]
fn dry_run_parent_creation_is_reported_as_would_mkdir() {
    let f = Fixture::new();
    let output = f
        .command(&["--dry-run", "--parents", "future", "missing/link"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("WOULD_MKDIR"), "stdout={stdout:?}");
    assert!(!f.root.join("missing").exists());
}

#[test]
fn registry_rejects_duplicate_destinations_through_parent_aliases() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("real")).unwrap();
    symlink("real", f.root.join("alias")).unwrap();
    fs::write(
        f.root.join("links.toml"),
        "version = 1\n\n[[links]]\nlink = 'real/link'\ntarget = 'one'\n\n[[links]]\nlink = 'alias/link'\ntarget = 'two'\n",
    )
    .unwrap();

    let output = f.command(&["list"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("duplicate link"));
}

#[test]
fn invalid_create_link_name_uses_argument_error_exit_code() {
    let f = Fixture::new();
    let output = f.command(&["target", "bad/"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!f.root.join("links.toml.slink-lock").exists());
}

#[test]
fn ordinary_directory_at_managed_link_is_never_replaced() {
    let f = Fixture::new();
    assert_eq!(
        f.command(&["future", "link"]).status().unwrap().code(),
        Some(0)
    );
    fs::remove_file(f.root.join("link")).unwrap();
    fs::create_dir(f.root.join("link")).unwrap();

    assert_eq!(
        f.command(&["fix", "--replace"]).status().unwrap().code(),
        Some(1)
    );
    assert_eq!(
        f.command(&["remove", "link"]).status().unwrap().code(),
        Some(1)
    );
    assert!(f.root.join("link").is_dir());
}

#[test]
fn editing_registry_link_field_does_not_remove_the_old_link() {
    let f = Fixture::new();
    assert_eq!(
        f.command(&["future", "old-link"]).status().unwrap().code(),
        Some(0)
    );
    let registry = fs::read_to_string(f.root.join("links.toml")).unwrap();
    fs::write(
        f.root.join("links.toml"),
        registry.replace("old-link", "new-link"),
    )
    .unwrap();

    assert_eq!(f.command(&["fix"]).status().unwrap().code(), Some(0));
    assert!(fs::symlink_metadata(f.root.join("old-link"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::symlink_metadata(f.root.join("new-link"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn paths_with_spaces_and_quotes_round_trip_through_registry() {
    let f = Fixture::new();
    let target = "source with space 'and quote'";
    let link = "link with space 'and quote'";
    fs::write(f.root.join(target), "ok").unwrap();

    assert_eq!(f.command(&[target, link]).status().unwrap().code(), Some(0));
    assert_eq!(fs::read_to_string(f.root.join(link)).unwrap(), "ok");
    assert_eq!(
        f.command(&["check", link]).status().unwrap().code(),
        Some(0)
    );
}

#[test]
fn concurrent_registry_lock_is_reported_without_mutation() {
    let f = Fixture::new();
    let lock_path = f.root.join("links.toml.slink-lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.lock_exclusive().unwrap();

    let output = f.command(&["future", "link"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("registry is busy"));
    assert!(!f.root.join("links.toml").exists());
    assert!(!f.root.join("link").exists());
}
