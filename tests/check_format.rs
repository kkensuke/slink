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
        "LINK_STATE\tTARGET_STATE\tLINK\tTARGET\tACTUAL_TARGET_STATE\tACTUAL_TARGET\n"
    ));
    assert!(stdout.contains("MATCH\tREACHABLE\t"));
    assert_eq!(stdout.lines().count(), 2);
    assert!(stdout.lines().all(|line| line.split('\t').count() == 6));
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
    assert_eq!(rows[0][0], "MISMATCH");
    assert_eq!(rows[0][1], "REACHABLE");
    assert_eq!(rows[0][4], "REACHABLE");
    assert_eq!(
        serde_json::from_str::<String>(rows[0][3]).unwrap(),
        f.path("expected").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(rows[0][5]).unwrap(),
        "actual"
    );
    assert_eq!(rows[1][0], "MISSING");
    assert_eq!(rows[2][0], "CONFLICT");
    for row in &rows[1..] {
        assert_eq!(&row[4..], &["", ""]);
    }
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("expected a symlink, found file"));
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
