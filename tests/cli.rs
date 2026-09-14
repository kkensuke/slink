use std::os::unix::fs::symlink;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir(root.join("home")).unwrap();
        Self { _dir: dir, root }
    }
    fn path(&self, p: &str) -> PathBuf {
        self.root.join(p)
    }
    fn write(&self, p: &str, text: &str) {
        fs::write(self.path(p), text).unwrap();
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_slink"));
        c.current_dir(&self.root)
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env_remove("SLINK_TEST_CRASH")
            .arg("--file")
            .arg(self.path("links.toml"))
            .args(args);
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let o = self.run(args);
        assert_eq!(
            o.status.code(),
            Some(0),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        o
    }
    fn registry(&self) -> String {
        fs::read_to_string(self.path("links.toml")).unwrap()
    }
    fn entries(&self) -> usize {
        self.registry()
            .parse::<toml_edit::DocumentMut>()
            .unwrap()
            .get("links")
            .and_then(|i| i.as_array_of_tables())
            .map_or(0, |a| a.len())
    }
    fn target(&self, p: &str) -> PathBuf {
        fs::read_link(self.path(p)).unwrap()
    }
    fn crash(&self, args: &[&str], stage: &str) {
        let o = self
            .command(args)
            .env("SLINK_TEST_CRASH", stage)
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(99),
            "{stage}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn help_and_invalid_options_do_not_write() {
    let f = Fixture::new();
    f.ok(&[]);
    assert!(!f.path("links.toml").exists());
    for args in [
        &["list", "--parents"][..],
        &["adopt", "--relative", "x"],
        &["remove"],
        &["--unknown"],
        &["x"],
        &["check", "--dry-run"],
    ] {
        assert_eq!(f.run(args).status.code(), Some(2), "{args:?}");
    }
    assert!(!f.path("links.toml.slink-lock").exists());
}

#[test]
fn create_check_list_and_idempotency() {
    let f = Fixture::new();
    f.write("source", "hello");
    f.ok(&["source", "link"]);
    assert_eq!(f.target("link"), Path::new("source"));
    assert_eq!(fs::read_to_string(f.path("link")).unwrap(), "hello");
    let before = f.registry();
    f.ok(&["source", "link"]);
    assert_eq!(before, f.registry());
    f.ok(&["check"]);
    assert!(String::from_utf8(f.ok(&["list"]).stdout)
        .unwrap()
        .contains("source"));
    assert_eq!(f.entries(), 1);
}

#[test]
fn dangling_link_is_created_and_diagnosed() {
    let f = Fixture::new();
    let o = f.ok(&["future", "link"]);
    assert!(String::from_utf8(o.stderr).unwrap().contains("MISSING"));
    assert!(fs::symlink_metadata(f.path("link"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(f.run(&["check"]).status.code(), Some(1));
    let before = f.registry();
    f.ok(&["fix"]);
    assert_eq!(f.registry(), before);
    f.write("future", "ready");
    f.ok(&["check"]);
}

#[test]
fn relative_is_opt_in_and_parents_are_not_targets() {
    let f = Fixture::new();
    f.write("source", "ok");
    assert_eq!(f.run(&["source", "sub/raw"]).status.code(), Some(1));
    assert!(!f.path("sub").exists());
    f.ok(&["--parents", "source", "sub/raw"]);
    assert_eq!(f.target("sub/raw"), Path::new("source"));
    f.ok(&["--relative", "--parents", "source", "other/link"]);
    assert_eq!(f.target("other/link"), Path::new("../source"));
    assert_eq!(fs::read_to_string(f.path("other/link")).unwrap(), "ok");
    f.ok(&["--parents", "missing/target", "deep/link"]);
    assert!(!f.path("missing").exists());
}

#[test]
fn relative_keeps_target_symlink_and_parent_components() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("tree/child")).unwrap();
    f.write("tree/data", "correct");
    f.write("data", "wrong");
    symlink("tree/child", f.path("alias")).unwrap();
    f.ok(&["--relative", "--parents", "alias/../data", "out/link"]);
    assert_eq!(f.target("out/link"), Path::new("../alias/../data"));
    assert_eq!(fs::read_to_string(f.path("out/link")).unwrap(), "correct");
    symlink("tree/data", f.path("pointer")).unwrap();
    f.ok(&["--relative", "pointer", "out/second"]);
    assert_eq!(f.target("out/second"), Path::new("../pointer"));
}

#[test]
fn relative_uses_physical_link_parent() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("tree/deep")).unwrap();
    f.write("source", "ok");
    symlink("tree/deep", f.path("alias")).unwrap();
    f.ok(&["--relative", "source", "alias/link"]);
    assert_eq!(f.target("alias/link"), Path::new("../../source"));
    assert_eq!(fs::read_to_string(f.path("alias/link")).unwrap(), "ok");
}

#[test]
fn dry_run_creates_no_registry_lock_journal_or_directories() {
    let f = Fixture::new();
    let before = fs::read_dir(&f.root).unwrap().count();
    let o = f.ok(&[
        "--relative",
        "--parents",
        "--dry-run",
        "source",
        "deep/child/link",
    ]);
    assert!(String::from_utf8(o.stdout)
        .unwrap()
        .contains("WOULD_CREATE"));
    assert_eq!(before, fs::read_dir(&f.root).unwrap().count());
    assert!(!f.path("deep").exists());
    symlink("future", f.path("existing")).unwrap();
    f.ok(&["adopt", "--dry-run", "existing"]);
    assert!(!f.path("links.toml").exists());
}

#[test]
fn fix_requires_replace_and_protects_ordinary_files() {
    let f = Fixture::new();
    f.ok(&["one", "link"]);
    fs::remove_file(f.path("link")).unwrap();
    symlink("two", f.path("link")).unwrap();
    assert_eq!(f.run(&["fix"]).status.code(), Some(1));
    assert_eq!(f.target("link"), Path::new("two"));
    f.ok(&["fix", "--replace", "--dry-run"]);
    assert_eq!(f.target("link"), Path::new("two"));
    f.ok(&["fix", "--replace"]);
    assert_eq!(f.target("link"), Path::new("one"));
    fs::remove_file(f.path("link")).unwrap();
    f.write("link", "keep me");
    assert_eq!(f.run(&["fix", "--replace"]).status.code(), Some(1));
    assert_eq!(f.run(&["remove", "link"]).status.code(), Some(1));
    assert_eq!(fs::read_to_string(f.path("link")).unwrap(), "keep me");
    f.ok(&["remove", "--keep-link", "link"]);
    assert_eq!(f.entries(), 0);
    assert_eq!(fs::read_to_string(f.path("link")).unwrap(), "keep me");
}

#[test]
fn missing_link_and_parent_restore_are_separate() {
    let f = Fixture::new();
    f.ok(&["--parents", "future", "parent/link"]);
    fs::remove_file(f.path("parent/link")).unwrap();
    fs::remove_dir(f.path("parent")).unwrap();
    assert_eq!(f.run(&["fix"]).status.code(), Some(1));
    assert!(!f.path("parent").exists());
    f.ok(&["fix", "--parents"]);
    assert_eq!(f.target("parent/link"), Path::new("future"));
    f.ok(&["remove", "parent/link"]);
    assert!(f.path("parent").is_dir());
}

#[test]
fn removing_records_manually_does_not_prune_links() {
    let f = Fixture::new();
    f.ok(&["future", "link"]);
    f.write("links.toml", "version = 1\n");
    f.ok(&["fix"]);
    assert_eq!(f.target("link"), Path::new("future"));
    assert_eq!(f.run(&["remove", "link"]).status.code(), Some(2));
}

#[test]
fn adopt_records_raw_targets_and_refuses_overwriting_registry() {
    let f = Fixture::new();
    symlink("future", f.path("link")).unwrap();
    assert_eq!(f.run(&["future", "link"]).status.code(), Some(1));
    f.ok(&["adopt", "link"]);
    let before = f.registry();
    f.ok(&["adopt", "link"]);
    assert_eq!(f.registry(), before);
    fs::remove_file(f.path("link")).unwrap();
    symlink("different", f.path("link")).unwrap();
    assert_eq!(f.run(&["adopt", "link"]).status.code(), Some(1));
    assert_eq!(f.registry(), before);
    f.ok(&["remove", "--keep-link", "link"]);
    assert_eq!(f.target("link"), Path::new("different"));
}

#[test]
fn multiple_removals_do_not_delete_targets() {
    let f = Fixture::new();
    f.write("source", "data");
    f.ok(&["source", "a"]);
    f.ok(&["source", "b"]);
    f.ok(&["remove", "a", "b"]);
    assert_eq!(f.entries(), 0);
    assert_eq!(fs::read_to_string(f.path("source")).unwrap(), "data");
}

#[test]
fn comments_quotes_order_and_crlf_survive_registration() {
    for newline in ["\n", "\r\n"] {
        let f = Fixture::new();
        let original="# intro\nversion = 1\n\n# first link\n[[links]]\nlink = 'one' # location\ntarget   = 'source'\n".replace('\n',newline);
        f.write("links.toml", &original);
        symlink("future", f.path("two")).unwrap();
        f.ok(&["adopt", "two"]);
        assert!(f.registry().starts_with(&original), "{}", f.registry());
        if newline == "\r\n" {
            assert!(!f.registry().replace("\r\n", "").contains('\n'));
        }
        f.ok(&["remove", "--keep-link", "two"]);
        assert_eq!(f.registry(), original);
    }
}

#[test]
fn registry_symlink_is_preserved_with_logical_relative_base() {
    let f = Fixture::new();
    fs::create_dir(f.path("store")).unwrap();
    f.write(
        "store/real.toml",
        "version = 1\n[[links]]\nlink = 'one'\ntarget = 'source'\n",
    );
    f.write("source", "data");
    symlink("source", f.path("one")).unwrap();
    symlink("store/real.toml", f.path("links.toml")).unwrap();
    f.ok(&["check"]);
    symlink("future", f.path("two")).unwrap();
    f.ok(&["adopt", "two"]);
    assert_eq!(f.target("links.toml"), Path::new("store/real.toml"));
    assert_eq!(f.entries(), 2);
}

#[test]
fn list_does_not_inspect_managed_paths() {
    let f = Fixture::new();
    f.write(
        "links.toml",
        "version = 1\n[[links]]\nlink = 'missing/../link'\ntarget = 'source'\n",
    );
    f.ok(&["list"]);
    assert!(!f.path("missing").exists());
}

#[test]
fn scan_is_read_only_and_does_not_follow_directory_links() {
    let f = Fixture::new();
    fs::create_dir(f.path("tree")).unwrap();
    symlink("..", f.path("tree/loop")).unwrap();
    symlink("missing", f.path("tree/broken")).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_slink"))
        .current_dir(&f.root)
        .env("HOME", f.path("home"))
        .env("XDG_CONFIG_HOME", f.path("config"))
        .args(["scan", "tree"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    let out = String::from_utf8(o.stdout).unwrap();
    assert_eq!(out.lines().count(), 3);
    assert!(out.contains("UNMANAGED"));
    assert!(!f.path("config").exists());
}

#[test]
fn invalid_registry_does_not_touch_files() {
    let f = Fixture::new();
    f.write("link", "keep");
    for text in [
        "version = 2\n",
        "version = 1\nunknown = true\n",
        "version = 1\n[[links]]\nlink = 'x'\ntarget = 'y'\nextra = 1\n",
        "version = 1\n[[links]]\nlink = 'x'\ntarget = 'y'\n[[links]]\nlink = 'x'\ntarget = 'z'\n",
    ] {
        f.write("links.toml", text);
        assert_eq!(f.run(&["fix", "--replace"]).status.code(), Some(2));
        assert_eq!(f.registry(), text);
    }
    assert_eq!(fs::read_to_string(f.path("link")).unwrap(), "keep");
}

#[test]
fn reserved_names_and_control_characters_are_literal_after_separator() {
    let f = Fixture::new();
    f.ok(&["--", "list", "a\nlink"]);
    assert_eq!(f.target("a\nlink"), Path::new("list"));
    let out = String::from_utf8(f.ok(&["list"]).stdout).unwrap();
    assert!(out.contains("a\\nlink"));
    assert_eq!(out.lines().count(), 2);
}

#[test]
fn creation_recovers_at_every_stage() {
    for stage in ["prepared", "linked", "committed", "cleaned"] {
        let f = Fixture::new();
        f.crash(&["--parents", "future", "sub/link"], stage);
        assert_eq!(f.run(&["check"]).status.code(), Some(1));
        assert_ne!(f.run(&["fix"]).status.code(), Some(0));
        f.ok(&["--parents", "future", "sub/link"]);
        assert_eq!(f.entries(), 1);
        assert_eq!(f.target("sub/link"), Path::new("future"));
        assert!(!f.path("links.toml.slink-pending").exists());
    }
}

#[test]
fn remove_recovers_without_removing_other_entries_or_new_objects() {
    for stage in ["prepared", "moved", "committed", "cleaned"] {
        let f = Fixture::new();
        f.ok(&["future", "a"]);
        f.ok(&["future", "b"]);
        f.crash(&["remove", "a"], stage);
        assert_ne!(f.run(&["fix"]).status.code(), Some(0));
        if stage == "cleaned" {
            f.write("a", "new user file");
        }
        f.ok(&["remove", "a"]);
        assert_eq!(f.entries(), 1);
        assert_eq!(f.target("b"), Path::new("future"));
        if stage == "cleaned" {
            assert_eq!(fs::read_to_string(f.path("a")).unwrap(), "new user file");
        } else {
            assert!(fs::symlink_metadata(f.path("a")).is_err());
        }
    }
}

#[test]
fn replacement_requires_same_authorization_during_recovery() {
    for stage in ["prepared", "moved", "linked", "committed", "cleaned"] {
        let f = Fixture::new();
        f.ok(&["expected", "link"]);
        fs::remove_file(f.path("link")).unwrap();
        symlink("old", f.path("link")).unwrap();
        f.crash(&["fix", "--replace"], stage);
        assert_ne!(f.run(&["fix"]).status.code(), Some(0));
        f.ok(&["fix", "--replace"]);
        assert_eq!(f.target("link"), Path::new("expected"));
    }
}

#[test]
fn concurrent_manifest_edit_stops_recovery() {
    let f = Fixture::new();
    f.crash(&["future", "link"], "linked");
    f.write("links.toml", "# user edit\nversion = 1\n");
    assert_ne!(f.run(&["future", "link"]).status.code(), Some(0));
    assert_eq!(f.registry(), "# user edit\nversion = 1\n");
    assert_eq!(f.target("link"), Path::new("future"));
    assert!(f.path("links.toml.slink-pending").exists());
}

#[test]
fn replacing_during_recovery_preserves_conflicting_regular_file() {
    let f = Fixture::new();
    f.ok(&["expected", "link"]);
    fs::remove_file(f.path("link")).unwrap();
    symlink("old", f.path("link")).unwrap();
    f.crash(&["fix", "--replace"], "moved");
    f.write("link", "do not delete");
    assert_ne!(f.run(&["fix", "--replace"]).status.code(), Some(0));
    assert_eq!(fs::read_to_string(f.path("link")).unwrap(), "do not delete");
    fs::remove_file(f.path("link")).unwrap();
    f.ok(&["fix", "--replace"]);
    assert_eq!(f.target("link"), Path::new("expected"));
}

#[test]
fn filesystem_case_and_unicode_aliases_do_not_duplicate_registrations() {
    for (first, second) in [("Alpha", "alpha"), ("caf\u{e9}", "cafe\u{301}")] {
        let f = Fixture::new();
        f.ok(&["future", first]);
        let aliases = fs::symlink_metadata(f.path(second)).is_ok();
        f.ok(&["future", second]);
        assert_eq!(f.entries(), if aliases { 1 } else { 2 });
    }
}

#[test]
fn recovery_dry_run_rejects_registry_edits_without_writing() {
    let f = Fixture::new();
    f.crash(&["future", "link"], "linked");
    f.write("links.toml", "# user edit\nversion = 1\n");
    let pending = fs::read(f.path("links.toml.slink-pending")).unwrap();
    assert_eq!(
        f.run(&["--dry-run", "future", "link"]).status.code(),
        Some(2)
    );
    assert_eq!(f.registry(), "# user edit\nversion = 1\n");
    assert_eq!(f.target("link"), Path::new("future"));
    assert_eq!(
        fs::read(f.path("links.toml.slink-pending")).unwrap(),
        pending
    );
}

#[test]
fn recovery_dry_run_checks_destinations_and_previews_remaining_items() {
    let f = Fixture::new();
    f.ok(&["expected", "a"]);
    f.ok(&["expected", "b"]);
    f.ok(&["expected", "c"]);
    for link in ["a", "b", "c"] {
        fs::remove_file(f.path(link)).unwrap();
        symlink("old", f.path(link)).unwrap();
    }
    f.crash(&["fix", "--replace"], "moved");
    f.write("a", "preserve me");
    assert_eq!(
        f.run(&["fix", "--replace", "--dry-run"]).status.code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(f.path("a")).unwrap(), "preserve me");
    fs::remove_file(f.path("a")).unwrap();
    let registry = f.registry();
    let pending = fs::read(f.path("links.toml.slink-pending")).unwrap();
    let output = String::from_utf8(f.ok(&["fix", "--replace", "--dry-run"]).stdout).unwrap();
    assert_eq!(output.matches("WOULD_RECOVER").count(), 1);
    assert_eq!(output.matches("WOULD_REPLACE").count(), 2);
    assert_eq!(f.registry(), registry);
    assert_eq!(
        fs::read(f.path("links.toml.slink-pending")).unwrap(),
        pending
    );
    assert!(fs::symlink_metadata(f.path("a")).is_err());
    assert_eq!(f.target("b"), Path::new("old"));
    f.ok(&["fix", "--replace"]);
    for link in ["a", "b", "c"] {
        assert_eq!(f.target(link), Path::new("expected"));
    }
}

#[test]
fn dry_run_adopt_recognizes_aliases_planned_earlier_in_the_batch() {
    let f = Fixture::new();
    fs::create_dir(f.path("dir")).unwrap();
    symlink("dir", f.path("alias")).unwrap();
    symlink("future", f.path("dir/link")).unwrap();
    let output = String::from_utf8(
        f.ok(&["adopt", "--dry-run", "dir/link", "alias/link"])
            .stdout,
    )
    .unwrap();
    assert_eq!(output.matches("WOULD_REGISTER").count(), 1);
    assert_eq!(output.matches("UNCHANGED").count(), 1);
    assert!(!f.path("links.toml").exists());
    assert!(!f.path("links.toml.slink-lock").exists());
    f.ok(&["adopt", "dir/link", "alias/link"]);
    assert_eq!(f.entries(), 1);
}

#[test]
fn removal_recovery_dry_run_preserves_the_link_and_previews_the_batch() {
    for stage in ["prepared", "moved", "committed", "cleaned"] {
        let f = Fixture::new();
        f.ok(&["future", "a"]);
        f.ok(&["future", "b"]);
        f.crash(&["remove", "a", "b"], stage);
        let registry = f.registry();
        let pending = fs::read(f.path("links.toml.slink-pending")).unwrap();
        let original = fs::read_link(f.path("a")).ok();
        let output = String::from_utf8(f.ok(&["remove", "--dry-run", "a", "b"]).stdout).unwrap();
        assert!(output.contains("WOULD_RECOVER"));
        assert!(output.contains("WOULD_REMOVE+UNREGISTER"));
        assert_eq!(f.registry(), registry);
        assert_eq!(
            fs::read(f.path("links.toml.slink-pending")).unwrap(),
            pending
        );
        assert_eq!(fs::read_link(f.path("a")).ok(), original);
        assert_eq!(f.target("b"), Path::new("future"));
        f.ok(&["remove", "a", "b"]);
        assert_eq!(f.entries(), 0);
    }
}
