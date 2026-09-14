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
