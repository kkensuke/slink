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

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_slink"))
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .arg("--file")
            .arg(self.root.join("links.toml"))
            .args(args)
            .output()
            .unwrap()
    }
}

#[test]
fn check_prints_only_a_summary_when_all_links_are_healthy() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "link"]).status.success());

    let output = f.run(&["check"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK 1 link\n");
}

#[test]
fn check_explains_a_target_mismatch_with_expected_and_actual_text() {
    let f = Fixture::new();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    symlink("actual", f.root.join("link")).unwrap();
    fs::write(
        f.root.join("links.toml"),
        "version = 1\n[[links]]\nlink = 'link'\ntarget = 'expected'\n",
    )
    .unwrap();

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("1 problem found (1 link checked)\n\n"));
    assert!(stdout.contains("— target differs\n"));
    assert!(stdout.contains("  expected: expected\n"));
    assert!(stdout.contains("  actual:   actual\n"));
}
