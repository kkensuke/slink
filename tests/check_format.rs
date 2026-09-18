use std::fs;

mod support;
use support::Fixture;

#[test]
fn check_tsv_uses_fixed_columns_and_one_row_per_link() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "link"]).status.success());

    let output = f.run(&["check", "--format", "tsv"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with(
        "LINK\tLINK_STATE\tTARGET\tTARGET_STATE\tACTUAL_TARGET\tACTUAL_TARGET_STATE\n"
    ));
    assert_eq!(stdout.lines().count(), 2);
    assert!(stdout.lines().all(|line| line.split('\t').count() == 6));
    let row: Vec<_> = stdout.lines().nth(1).unwrap().split('\t').collect();
    assert_eq!(row[1], "MATCH");
    assert_eq!(row[3], "REACHABLE");
    assert_eq!(row[5], "REACHABLE");
}

#[test]
fn check_tsv_keeps_mismatch_details_on_the_same_record() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    assert!(f.run(&["expected", "mismatch"]).status.success());
    assert!(f.run(&["expected", "missing"]).status.success());
    assert!(f.run(&["expected", "conflict"]).status.success());
    fs::remove_file(f.root.join("mismatch")).unwrap();
    symlink("actual", f.root.join("mismatch")).unwrap();
    fs::remove_file(f.root.join("missing")).unwrap();
    fs::remove_file(f.root.join("conflict")).unwrap();
    fs::write(f.root.join("conflict"), "ordinary file").unwrap();

    let output = f.run(&["check", "--format", "tsv"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<Vec<_>> = stdout
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.len() == 6));
    assert_eq!(rows[0][1], "MISMATCH");
    assert_eq!(rows[0][3], "REACHABLE");
    assert_eq!(rows[0][5], "REACHABLE");
    assert_eq!(
        serde_json::from_str::<String>(rows[0][2]).unwrap(),
        f.path("expected").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(rows[0][4]).unwrap(),
        "actual"
    );
    assert_eq!(rows[1][1], "MISSING");
    assert_eq!(rows[2][1], "CONFLICT");
    for row in &rows[1..] {
        assert_eq!(&row[4..], &["", ""]);
    }
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("expected a symlink, found file"));
}

#[test]
fn pending_paths_use_json_quotes_without_changing_recovery_data() {
    let f = Fixture::new();
    let target = "future\u{9b}/.";
    f.crash(&[target, "home/link"], "linked");
    let pending_path = f.path("config/slink/links.toml.slink-pending");
    let before = fs::read(&pending_path).unwrap();

    let output = f.run(&["check", "-o", "tsv"]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(!error.chars().any(|c| c.is_control() && c != '\n'));
    let (link, target_text) = error
        .trim_end()
        .strip_prefix("PENDING: Create ")
        .unwrap()
        .split_once(" -> ")
        .unwrap();
    let target_text = target_text
        .strip_suffix("; repeat the original operation to recover")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<String>(link).unwrap(),
        f.path("home/link").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(target_text).unwrap(),
        f.path(target).to_str().unwrap()
    );
    assert_eq!(fs::read(pending_path).unwrap(), before);
    assert_eq!(
        f.target("home/link").to_str().unwrap(),
        f.path(target).to_str().unwrap()
    );
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
