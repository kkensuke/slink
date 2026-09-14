use std::{fs, process::Command};

#[test]
fn real_parent_creation_is_not_reported_as_would_mkdir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("home")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_slink"))
        .current_dir(&root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args([
            "--file",
            root.join("links.toml").to_str().unwrap(),
            "--parents",
            "future",
            "missing/link",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("MKDIR"), "stdout={stdout:?}");
    assert!(!stdout.contains("WOULD_MKDIR"), "stdout={stdout:?}");
    assert!(root.join("missing").is_dir());
}
