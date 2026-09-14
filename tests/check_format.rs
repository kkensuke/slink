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
fn check_tsv_keeps_the_previous_machine_readable_shape() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "link"]).status.success());

    let output = f.run(&["check", "--format", "tsv"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("LINK_STATE\tTARGET_STATE\tLINK\tTARGET\n"));
    assert!(stdout.contains("MATCH\tREACHABLE\t"));
}

#[test]
fn read_only_views_accept_supported_formats_and_reject_invalid_formats() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    fs::create_dir(f.root.join("tree")).unwrap();
    assert!(f.run(&["target", "link"]).status.success());

    assert!(f.run(&["check", "--format=human"]).status.success());
    assert!(f.run(&["list", "--format", "tsv"]).status.success());
    assert!(f.run(&["list", "--format", "human"]).status.success());
    assert!(f.run(&["scan", "--format", "tsv", "tree"]).status.success());
    assert!(f
        .run(&["scan", "--format", "human", "tree"])
        .status
        .success());

    assert_eq!(f.run(&["check", "--format", "json"]).status.code(), Some(2));
    assert_eq!(f.run(&["fix", "--format", "tsv"]).status.code(), Some(2));
}
