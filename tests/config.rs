use std::{fs, path::PathBuf, process::Command};

fn slink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_slink"))
}

#[test]
fn config_prints_only_the_default_registry_path() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let xdg = dir.path().join("xdg");
    fs::create_dir(&home).unwrap();

    let output = slink()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("config")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", xdg.join("slink/links.toml").display())
    );
    assert!(!xdg.exists());
}

#[test]
fn config_falls_back_to_home_for_unset_empty_or_relative_xdg() {
    for xdg in [None, Some(""), Some("relative")] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir(&home).unwrap();
        let mut command = slink();
        command.env("HOME", &home).arg("config");
        match xdg {
            Some(value) => {
                command.env("XDG_CONFIG_HOME", value);
            }
            None => {
                command.env_remove("XDG_CONFIG_HOME");
            }
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "xdg={xdg:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}\n",
                home.join(PathBuf::from(".config/slink/links.toml"))
                    .display()
            ),
            "xdg={xdg:?}"
        );
    }
}

#[test]
fn config_rejects_operands_and_options() {
    for args in [
        &["config", "extra"][..],
        &["config", "--file", "other.toml"],
    ] {
        assert_eq!(slink().args(args).status().unwrap().code(), Some(2));
    }
}

#[test]
fn config_rejects_options_in_either_position_without_creating_links() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["--file", "registry.toml", "config", "new-link"],
        vec!["config", "--file", "registry.toml", "new-link"],
        vec!["--parents", "config", "new-link"],
        vec!["--dry-run", "config"],
        vec!["config", "--format=human"],
    ] {
        let output = slink()
            .current_dir(dir.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "args={args:?}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn config_is_a_literal_target_after_separator() {
    let dir = tempfile::tempdir().unwrap();
    let output = slink()
        .current_dir(dir.path())
        .args(["--file", "registry.toml", "--", "config", "new-link"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_link(dir.path().join("new-link")).unwrap(),
        PathBuf::from("config")
    );
}
