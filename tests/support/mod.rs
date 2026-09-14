#![allow(dead_code)]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

pub struct Fixture {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir(root.join("home")).unwrap();
        Self { _dir: dir, root }
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    pub fn registry_path(&self) -> PathBuf {
        self.path("config/slink/links.toml")
    }
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_slink"));
        command
            .current_dir(&self.root)
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env_remove("SLINK_TEST_CRASH")
            .args(args);
        command
    }
    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
    pub fn ok(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    pub fn registry(&self) -> String {
        fs::read_to_string(self.registry_path()).unwrap()
    }
    pub fn write_registry(&self, text: &str) {
        fs::create_dir_all(self.registry_path().parent().unwrap()).unwrap();
        fs::write(self.registry_path(), text).unwrap();
    }
    pub fn target(&self, name: &str) -> PathBuf {
        fs::read_link(self.path(name)).unwrap()
    }
    pub fn crash(&self, args: &[&str], stage: &str) {
        let output = self
            .command(args)
            .env("SLINK_TEST_CRASH", stage)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(99),
            "{stage}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
