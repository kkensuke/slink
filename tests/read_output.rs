use std::{fs, os::unix::fs::symlink, process::Command};

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
            .env("NO_COLOR", "1")
            .arg("--file")
            .arg(self.root.join("links.toml"))
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args).output().unwrap()
    }
}

#[test]
fn list_uses_vertical_human_output_and_keeps_tsv_available() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "one"]).status.success());
    assert!(f.run(&["target", "two"]).status.success());

    let human = f.run(&["list"]);
    assert!(human.status.success());
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.starts_with("2 links\n\n"));
    assert!(stdout.contains("one\n  → target\n"));
    assert!(stdout.contains("two\n  → target\n"));
    assert!(!stdout.contains("LINK\tTARGET"));

    let tsv = f.run(&["list", "--format", "tsv"]);
    assert!(tsv.status.success());
    let stdout = String::from_utf8(tsv.stdout).unwrap();
    assert!(stdout.starts_with("LINK\tTARGET\n"));
}

#[test]
fn scan_groups_managed_and_unmanaged_and_puts_issues_first() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("tree")).unwrap();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    fs::write(f.root.join("healthy"), "ok").unwrap();

    assert!(f.run(&["expected", "tree/managed"]).status.success());
    fs::remove_file(f.root.join("tree/managed")).unwrap();
    symlink("../actual", f.root.join("tree/managed")).unwrap();
    symlink("../healthy", f.root.join("tree/unmanaged-ok")).unwrap();
    symlink("../missing", f.root.join("tree/unmanaged-broken")).unwrap();

    let output = f.run(&["scan", "tree"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Managed (1)\n"));
    assert!(stdout.contains("target differs"));
    assert!(stdout.contains("Unmanaged (2)\n"));
    assert!(stdout.contains("target is missing"));
    assert!(stdout.contains("3 symlinks found: 1 managed, 2 unmanaged, 2 link issues\n"));

    let broken = stdout.find("unmanaged-broken").unwrap();
    let healthy = stdout.find("unmanaged-ok").unwrap();
    assert!(broken < healthy, "stdout={stdout}");
    assert!(!stdout.contains("\x1b["));
}

#[test]
fn scan_reports_traversal_errors_separately_and_returns_one() {
    let f = Fixture::new();
    fs::write(f.root.join("not-a-directory"), "x").unwrap();

    let output = f.run(&["scan", "not-a-directory"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scan errors (1)\n"));
    assert!(stdout.contains("cannot scan"));
    assert!(stdout.contains("scan roots must be directories, not symlinks"));
}
