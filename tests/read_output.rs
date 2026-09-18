use std::{fs, os::unix::fs::symlink};

mod support;
use support::Fixture;

#[test]
fn list_uses_vertical_human_output_and_keeps_tsv_available() {
    let f = Fixture::new();
    fs::write(f.root.join("target"), "ok").unwrap();
    assert!(f.run(&["target", "one"]).status.success());
    assert!(f.run(&["target", "two"]).status.success());

    let human = f.run(&["list"]);
    assert!(human.status.success());
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.starts_with("2 links\n\n"));
    for link in ["one", "two"] {
        assert!(stdout.contains(&format!("{:?}\n  → {:?}\n", f.path(link), f.path("target"))));
    }
    assert!(!stdout.contains("LINK\tTARGET"));

    let tsv = f.run(&["list", "--format", "tsv"]);
    assert!(tsv.status.success());
    let stdout = String::from_utf8(tsv.stdout).unwrap();
    assert!(stdout.starts_with("LINK\tTARGET\n"));
}

#[test]
fn list_formats_both_paths_without_changing_stored_strings() {
    let f = Fixture::new();
    f.write_entries(&[
        ("home//./link", "home//./target/."),
        ("home/other", "home/target///"),
    ]);
    let before = f.registry();

    let human = String::from_utf8(f.ok(&["list"]).stdout).unwrap();
    assert!(human.contains(&format!(
        "{:?}\n  → {:?}\n",
        f.path("home/link"),
        f.path("home/target/.")
    )));
    assert!(human.contains(&format!("  → {:?}\n", f.path("home/target/"))));
    assert!(!human.contains("~/"));

    let tsv = String::from_utf8(f.ok(&["list", "-o", "tsv"]).stdout).unwrap();
    let rows: Vec<Vec<String>> = tsv
        .lines()
        .skip(1)
        .map(|line| {
            line.split('\t')
                .map(|cell| serde_json::from_str(cell).unwrap())
                .collect()
        })
        .collect();
    assert_eq!(rows[0][0], f.path("home//./link").to_str().unwrap());
    assert_eq!(rows[0][1], f.path("home//./target/.").to_str().unwrap());
    assert_eq!(rows[1][1], f.path("home/target///").to_str().unwrap());
    assert_eq!(f.registry(), before);
}

#[test]
fn scan_groups_managed_and_unmanaged_and_puts_issues_first() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("tree")).unwrap();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    fs::write(f.root.join("healthy"), "ok").unwrap();

    assert!(f.run(&["expected", "tree/managed"]).status.success());
    fs::remove_file(f.root.join("tree/managed")).unwrap();
    symlink("../actual", f.root.join("tree/managed")).unwrap();
    symlink("../healthy", f.root.join("tree/unmanaged-ok")).unwrap();
    symlink("../missing", f.root.join("tree/unmanaged-broken")).unwrap();

    let output = f.run(&["scan", "tree"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Managed (1)\n"));
    assert!(stdout.contains("target differs"));
    assert!(stdout.contains("Unmanaged (2)\n"));
    assert!(stdout.contains("target is missing"));
    assert!(stdout.contains("3 symlinks found: 1 managed, 2 unmanaged, 2 link issues\n"));

    let broken = stdout.find("unmanaged-broken").unwrap();
    let healthy = stdout.find("unmanaged-ok").unwrap();
    assert!(broken < healthy, "stdout={stdout}");
    assert!(!stdout.contains("\x1b["));
}

#[test]
fn scan_reports_traversal_errors_separately_and_returns_one() {
    let f = Fixture::new();
    f.write_registry("");
    fs::write(f.root.join("not-a-directory"), "x").unwrap();

    let output = f.run(&["scan", "not-a-directory"]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scan errors (1)\n"));
    assert!(stdout.contains("cannot scan"));
    assert!(stdout.contains("scan roots must be directories, not symlinks"));

    let tsv = f.run(&["scan", "-o", "tsv", "./not-a-directory"]);
    assert_eq!(tsv.status.code(), Some(1));
    let error = String::from_utf8(tsv.stderr).unwrap();
    let cells: Vec<_> = error.trim_end().split('\t').collect();
    assert_eq!(cells.len(), 3);
    assert_eq!(cells[0], "ERROR");
    assert_eq!(
        serde_json::from_str::<String>(cells[1]).unwrap(),
        f.path("not-a-directory").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(cells[2]).unwrap(),
        "scan roots must be directories, not symlinks"
    );
}

#[test]
fn scan_rejects_symlink_roots_with_or_without_trailing_separators() {
    let f = Fixture::new();
    f.write_registry("");
    fs::create_dir(f.root.join("outside")).unwrap();
    symlink("missing", f.root.join("outside/item")).unwrap();
    symlink("outside", f.root.join("alias")).unwrap();
    for root in ["alias", "alias/", "alias///"] {
        let output = f.run(&["scan", root]);
        assert_eq!(output.status.code(), Some(1), "root={root}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("scan roots must be directories, not symlinks"));
        assert!(!stdout.contains("alias/item"));
    }
    assert!(f.run(&["scan", "outside/"]).status.success());
}

#[test]
fn scan_tsv_uses_registered_and_actual_columns_consistently() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("tree")).unwrap();
    fs::write(f.root.join("expected"), "ok").unwrap();
    fs::write(f.root.join("actual"), "ok").unwrap();
    assert!(f.run(&["expected", "tree/managed"]).status.success());
    fs::remove_file(f.root.join("tree/managed")).unwrap();
    symlink("../actual", f.root.join("tree/managed")).unwrap();
    symlink("../missing", f.root.join("tree/unmanaged")).unwrap();

    let output = f.run(&["scan", "--format", "tsv", "./tree"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("/./"));
    assert_eq!(
        stdout.lines().next().unwrap(),
        "MANAGEMENT\tLINK\tLINK_STATE\tTARGET\tTARGET_STATE\tACTUAL_TARGET\tACTUAL_TARGET_STATE"
    );
    assert_eq!(stdout.lines().count(), 3);
    assert!(stdout.lines().all(|line| line.split('\t').count() == 7));
    let rows: Vec<Vec<_>> = stdout
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows[0][0], "MANAGED");
    assert_eq!(rows[0][2], "MISMATCH");
    assert_eq!(rows[0][4], "REACHABLE");
    assert_eq!(rows[0][6], "REACHABLE");
    assert_eq!(
        serde_json::from_str::<String>(rows[0][3]).unwrap(),
        f.path("expected").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(rows[0][5]).unwrap(),
        "../actual"
    );
    assert_eq!(rows[1][0], "UNMANAGED");
    assert_eq!(&rows[1][2..5], &["", "", ""]);
    assert_eq!(
        serde_json::from_str::<String>(rows[1][5]).unwrap(),
        "../missing"
    );
    assert_eq!(rows[1][6], "MISSING");
}

#[test]
fn check_and_scan_format_paths_but_keep_actual_target_strings_in_tsv() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("data/target")).unwrap();
    fs::create_dir(f.path("expected")).unwrap();
    let actual = "../data//./target/.";
    for name in ["managed", "matching", "unmanaged"] {
        symlink(actual, f.path(&format!("home/{name}"))).unwrap();
    }
    f.write_entries(&[
        ("home/managed", "expected/./."),
        ("home/matching", "data//./target/."),
    ]);
    let before = f.registry();

    for command in ["check", "scan"] {
        let mut args = vec![command];
        if command == "scan" {
            args.push("home");
        }
        let human = f.run(&args);
        let status = if command == "check" { 1 } else { 0 };
        assert_eq!(human.status.code(), Some(status));
        let text = String::from_utf8(human.stdout).unwrap();
        assert!(text.contains(&format!("! {:?} — target differs", f.path("home/managed"))));
        assert!(text.contains(&format!("expected: {:?}", f.path("expected/."))));
        assert!(text.contains("actual:   \"../data/target/.\""));
        assert!(!text.contains("~/"));
        if command == "scan" {
            assert_eq!(text.matches("→ \"../data/target/.\"").count(), 2);
        }

        args.extend(["-o", "tsv"]);
        let tsv = f.run(&args);
        assert_eq!(tsv.status.code(), Some(status));
        let text = String::from_utf8(tsv.stdout).unwrap();
        let offset = usize::from(command == "scan");
        assert_eq!(text.lines().count(), if offset == 1 { 4 } else { 3 });
        for row in text.lines().skip(1) {
            let cells: Vec<_> = row.split('\t').collect();
            assert_eq!(cells.len(), 6 + offset);
            let link: String = serde_json::from_str(cells[offset]).unwrap();
            assert!(link.starts_with(f.path("home").to_str().unwrap()));
            assert_eq!(
                serde_json::from_str::<String>(cells[4 + offset]).unwrap(),
                actual
            );
            if link.ends_with("/unmanaged") {
                assert_eq!(cells[0], "UNMANAGED");
                assert_eq!(&cells[2..5], &["", "", ""]);
            } else {
                let (state, target) = if link.ends_with("/matching") {
                    ("MATCH", "data/target/.")
                } else {
                    ("MISMATCH", "expected/.")
                };
                assert_eq!(cells[1 + offset], state);
                assert_eq!(
                    serde_json::from_str::<String>(cells[2 + offset]).unwrap(),
                    f.path(target).to_str().unwrap()
                );
            }
        }
    }
    assert_eq!(f.registry(), before);
    for name in ["managed", "matching", "unmanaged"] {
        assert_eq!(f.target(&format!("home/{name}")).to_str().unwrap(), actual);
    }
}

#[test]
fn scan_returns_one_when_actual_or_registered_targets_cannot_be_inspected() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    fs::create_dir(f.root.join("tree")).unwrap();
    let private = f.root.join("private");
    fs::create_dir(&private).unwrap();
    fs::write(private.join("target"), "ok").unwrap();
    fs::write(f.root.join("public"), "ok").unwrap();
    assert!(f.run(&["private/target", "tree/managed"]).status.success());
    assert!(f.run(&["private/target", "tree/matching"]).status.success());
    fs::remove_file(f.root.join("tree/managed")).unwrap();
    symlink("../public", f.root.join("tree/managed")).unwrap();
    symlink("../private/target", f.root.join("tree/unmanaged")).unwrap();

    fs::set_permissions(&private, fs::Permissions::from_mode(0o000)).unwrap();
    let probe = fs::metadata(private.join("target"));
    let human = f.run(&["scan", "tree"]);
    let tsv = f.run(&["scan", "--format", "tsv", "tree"]);
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    if probe.is_ok() {
        eprintln!("permission regression requires a user without permission-bypass privileges");
        return;
    }
    assert_eq!(
        probe.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(human.status.code(), Some(1));
    assert_eq!(tsv.status.code(), Some(1));
    let stdout = String::from_utf8(human.stdout).unwrap();
    assert!(stdout.contains("cannot inspect target"));
    assert!(
        !stdout.contains("Scan errors"),
        "the scan root was fully readable"
    );
    let stdout = String::from_utf8(tsv.stdout).unwrap();
    let rows: Vec<Vec<_>> = stdout
        .lines()
        .skip(1)
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][2], "MISMATCH");
    assert_eq!(rows[0][4], "UNKNOWN");
    assert_eq!(rows[0][6], "REACHABLE");
    assert_eq!(rows[1][2], "MATCH");
    assert_eq!(rows[1][4], "UNKNOWN");
    assert_eq!(rows[1][6], "UNKNOWN");
    assert_eq!(rows[2][6], "UNKNOWN");
    assert!(String::from_utf8(tsv.stderr)
        .unwrap()
        .contains("Permission denied"));
}

#[test]
fn closed_stdout_pipe_does_not_panic_or_change_the_operation_status() {
    use std::{
        io::{BufRead, BufReader},
        process::Stdio,
    };
    let f = Fixture::new();
    let names = (0..256).map(|i| format!("link-{i}")).collect::<Vec<_>>();
    let target = "x".repeat(1024);
    let entries = names
        .iter()
        .map(|name| (name.as_str(), target.as_str()))
        .collect::<Vec<_>>();
    f.write_entries(&entries);
    let mut child = f
        .command(&["list"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "256 links\n");
    drop(reader);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn other_stdout_errors_remain_failures() {
    use std::process::Stdio;
    let f = Fixture::new();
    let sink = fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let output = f
        .command(&["--config"])
        .stdout(Stdio::from(sink))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("cannot write stdout"));
    assert!(!error.contains("panicked"));
}
