mod support;

use std::{fs, os::unix::fs::symlink};
use support::Fixture;

#[test]
fn scan_without_directory_uses_cwd_and_recursive_flag_only_changes_depth() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("work/nested")).unwrap();
    symlink("missing", f.path("work/top")).unwrap();
    symlink("missing", f.path("work/nested/deep")).unwrap();

    let shallow = f
        .command(&["scan"])
        .current_dir(f.path("work"))
        .output()
        .unwrap();
    assert!(shallow.status.success());
    let shallow = String::from_utf8(shallow.stdout).unwrap();
    assert!(shallow.contains("top"));
    assert!(!shallow.contains("deep"));

    let recursive = f
        .command(&["scan", "-R"])
        .current_dir(f.path("work"))
        .output()
        .unwrap();
    assert!(recursive.status.success());
    let recursive = String::from_utf8(recursive.stdout).unwrap();
    assert!(recursive.contains("top"));
    assert!(recursive.contains("deep"));
}
