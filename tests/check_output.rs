use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

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
            .env_remove("SLINK_TEST_CRASH")
            .arg("--file")
            .arg(self.root.join("links.toml"))
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args).output().unwrap()
    }

    fn registry(&self, text: &str) {
        fs::write(self.root.join("links.toml"), text).unwrap();
    }
}

#[test]
fn check_prints_only_a_summary_when_all_links_are_healthy() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "one"]).status.success());
    assert!(f.run(&["target", "two"]).status.success());

    let output = f.run(&["check"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK 2 links\n");
}

#[test]
fn check_explains_a_target_mismatch_with_expected_and_actual_text() {
    let f = Fixture::new();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    symlink("actual", f.root.join("link")).unwrap();
    f.registry("version = 1\n[[link]]\nlink = 'link'\ntarget = 'expected'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("1 problem found (1 link checked)\n\n"));
    assert!(stdout.contains("— target differs\n"));
    assert!(stdout.contains("  expected: \"expected\"\n"));
    assert!(stdout.contains("  actual:   \"actual\"\n"));
}

#[test]
fn check_reports_missing_link_and_shortens_home_in_display() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    let target = f.root.join("target");
    f.registry(&format!(
        "version = 1\n[[link]]\nlink = '~/.missing-link'\ntarget = {:?}\n",
        target.to_str().unwrap()
    ));

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("! ~/.missing-link — link is missing\n"));
    assert!(!stdout.contains(f.root.join("home").to_str().unwrap()));
}

#[test]
fn check_reports_a_missing_target_without_repeating_internal_states() {
    let f = Fixture::new();
    symlink("missing", f.root.join("link")).unwrap();
    f.registry("version = 1\n[[link]]\nlink = 'link'\ntarget = 'missing'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("— target is missing\n  target: \"missing\"\n"));
    assert!(!stdout.contains("MATCH"));
    assert!(!stdout.contains("MISSING\t"));
}

#[test]
fn check_reports_conflicting_directory_in_plain_language() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    fs::create_dir(f.root.join("link")).unwrap();
    f.registry("version = 1\n[[link]]\nlink = 'link'\ntarget = 'target'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("— expected a symlink, found directory\n"));
    assert!(!stdout.contains("CONFLICT"));
}

#[test]
fn check_reports_resolution_errors_with_the_os_reason() {
    let f = Fixture::new();
    symlink("link", f.root.join("link")).unwrap();
    f.registry("version = 1\n[[link]]\nlink = 'link'\ntarget = 'link'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("— target cannot be resolved\n"));
    assert!(stdout.contains("  target: \"link\"\n"));
    assert!(stdout.contains("  reason: "));
    assert!(!stdout.contains("RESOLUTION_ERROR"));
}

#[test]
fn check_escapes_control_characters_in_human_output() {
    let f = Fixture::new();
    let link = "line\nbreak";
    symlink("missing", f.root.join(link)).unwrap();
    f.registry("version = 1\n[[link]]\nlink = \"line\\nbreak\"\ntarget = 'missing'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("line\\nbreak"));
    assert_eq!(
        stdout.lines().filter(|line| line.starts_with("! ")).count(),
        1
    );
}

#[test]
fn check_reports_pending_recovery_separately() {
    let f = Fixture::new();
    let crashed = f
        .command(&["future", "link"])
        .env("SLINK_TEST_CRASH", "linked")
        .output()
        .unwrap();
    assert_eq!(crashed.status.code(), Some(99));

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("! incomplete operation\n"));
    assert!(stdout.contains("  operation: create\n"));
    assert!(stdout.contains("  target: \"future\"\n"));
    assert!(
        stdout.contains("repeat the original command with the same target and options to recover")
    );
}

#[test]
fn check_selected_link_keeps_the_checked_count_scoped_to_the_selection() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "one"]).status.success());
    assert!(f.run(&["target", "two"]).status.success());

    let output = f.run(&["check", "one"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK 1 link\n");
    assert_eq!(
        fs::read_link(f.root.join("one")).unwrap(),
        Path::new("target")
    );
}

#[test]
fn different_target_strings_remain_distinguishable_in_human_output() {
    let f = Fixture::new();
    let expected = "a\\nb";
    let actual = "a\nb";
    fs::write(f.root.join(expected), "expected").unwrap();
    fs::write(f.root.join(actual), "actual").unwrap();
    symlink(actual, f.root.join("link")).unwrap();
    f.registry("version = 1\n[[link]]\nlink = 'link'\ntarget = 'a\\nb'\n");

    let output = f.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected_display = stdout
        .lines()
        .find_map(|line| line.strip_prefix("  expected: "))
        .unwrap();
    let actual_display = stdout
        .lines()
        .find_map(|line| line.strip_prefix("  actual:   "))
        .unwrap();
    assert_ne!(expected_display, actual_display);
    assert_eq!(
        serde_json::from_str::<String>(expected_display).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_str::<String>(actual_display).unwrap(),
        actual
    );
}

#[test]
fn target_output_preserves_spaces_and_escapes_all_terminal_controls() {
    let f = Fixture::new();
    let target = " leading\tline\n\\\"\u{7f}\u{9b} trailing ";
    symlink(target, f.root.join("link")).unwrap();
    assert!(f.run(&["adopt", "link"]).status.success());
    for args in [
        vec!["list"],
        vec!["list", "--format", "tsv"],
        vec!["scan", "."],
    ] {
        let output = f.run(&args);
        assert!(output.status.success(), "args={args:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t'));
        let value = if args.contains(&"tsv") {
            stdout.lines().nth(1).unwrap().split('\t').nth(1).unwrap()
        } else {
            stdout
                .lines()
                .find_map(|line| {
                    let line = line.trim_start();
                    line.strip_prefix("→ ")
                        .or_else(|| line.strip_prefix("target: "))
                })
                .unwrap()
        };
        assert_eq!(serde_json::from_str::<String>(value).unwrap(), target);
    }
}
