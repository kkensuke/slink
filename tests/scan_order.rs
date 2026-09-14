use std::{fs, os::unix::fs::symlink};

mod support;
use support::Fixture;

#[test]
fn recursive_scan_tsv_uses_stable_link_path_order() {
    let f = Fixture::new();
    f.write_registry("");
    fs::create_dir_all(f.path("tree/a")).unwrap();
    fs::create_dir_all(f.path("tree/b")).unwrap();
    symlink("missing-a", f.path("tree/a/link")).unwrap();
    symlink("missing-b", f.path("tree/b/link")).unwrap();

    let output = f.ok(&["scan", "-R", "-o", "tsv", "tree"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let links = stdout
        .lines()
        .skip(1)
        .map(|line| {
            let cell = line.split('\t').nth(1).unwrap();
            serde_json::from_str::<String>(cell).unwrap()
        })
        .collect::<Vec<_>>();

    assert_eq!(links, vec![
        f.path("tree/a/link").to_str().unwrap().to_owned(),
        f.path("tree/b/link").to_str().unwrap().to_owned(),
    ]);
}
