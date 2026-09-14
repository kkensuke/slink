mod support;
use std::{
    fs,
    os::unix::fs::{symlink, MetadataExt},
    path::Path,
};
use support::Fixture;

#[test]
fn cli_paths_use_cwd_and_expand_tilde_before_absolute_storage() {
    let f = Fixture::new();
    fs::write(f.path("source"), "ok").unwrap();
    f.ok(&["-p", "./source", "./sub/link"]);
    assert_eq!(f.target("sub/link"), f.path("source"));
    f.ok(&["~/missing", "~/link"]);
    assert_eq!(f.target("home/link"), f.path("home/missing"));
    assert!(!f.registry().contains("/./"));
    let doc = f.registry().parse::<toml_edit::DocumentMut>().unwrap();
    assert!(doc.get("version").is_none());
    for entry in doc["link"].as_array_of_tables().unwrap() {
        assert!(Path::new(entry["link"].as_str().unwrap()).is_absolute());
        assert!(Path::new(entry["target"].as_str().unwrap()).is_absolute());
    }
}

#[test]
fn info_options_are_exclusive_and_obsolete_options_are_rejected() {
    let f = Fixture::new();
    for option in ["-c", "--config"] {
        assert_eq!(
            String::from_utf8(f.ok(&[option]).stdout).unwrap().trim(),
            f.registry_path().to_str().unwrap()
        );
    }
    for args in [
        vec!["--config", "new-link"],
        vec!["--config", "scan"],
        vec!["-cp"],
        vec!["-c", "--format=human"],
        vec!["--file", "other", "scan"],
        vec!["--relative", "x", "y"],
        vec!["fix", "--replace"],
        vec!["scan", "-f"],
        vec!["adopt", "-p", "x"],
        vec!["-n"],
        vec!["--config", "--invalid"],
        vec!["--help", "--invalid"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
    }
    assert!(!f.path("config").exists());
    f.ok(&["config", "config-link"]);
    assert_eq!(f.target("config-link"), f.path("config"));
    f.ok(&["list", "-otsv"]);
    f.ok(&["-o", "tsv", "list"]);
    f.ok(&["--format=tsv", "list"]);
}

#[test]
fn registry_rejects_relative_and_tilde_paths_in_both_fields() {
    let f = Fixture::new();
    for (field, value) in [
        ("link", "relative"),
        ("link", "~/link"),
        ("target", "../source"),
        ("target", "~/source"),
    ] {
        let link = if field == "link" {
            value.to_string()
        } else {
            f.path("link").to_str().unwrap().to_string()
        };
        let target = if field == "target" {
            value.to_string()
        } else {
            f.path("source").to_str().unwrap().to_string()
        };
        f.write_registry(&format!("[[link]]\nlink = {link:?}\ntarget = {target:?}\n"));
        let before = f.registry();
        let output = f.run(&["fix"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains(&format!("registry {field} must be an absolute path")));
        assert_eq!(before, f.registry());
    }
    f.write_registry("version = 1\n");
    assert_eq!(f.run(&["list"]).status.code(), Some(2));
}

#[test]
fn registry_path_errors_do_not_guess_from_the_working_directory() {
    let f = Fixture::new();
    f.write_registry(&format!(
        "[[link]]\nlink = {:?}\ntarget = 'PhD'\n",
        f.path("link")
    ));
    let first = f.run(&["check"]);
    let second = f
        .command(&["check"])
        .current_dir(f.path("home"))
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(2));
    assert_eq!(second.status.code(), Some(2));
    assert_eq!(first.stderr, second.stderr);
    let error = String::from_utf8(first.stderr).unwrap();
    assert!(error.contains("registry target must be an absolute path: \"PhD\""));
    assert!(!error.contains("working directory"));
    assert!(!error.contains(f.path("PhD").to_str().unwrap()));
}

#[test]
fn create_registers_matching_links_and_force_updates_both_states() {
    let f = Fixture::new();
    symlink("one", f.path("link")).unwrap();
    let inode = fs::symlink_metadata(f.path("link")).unwrap().ino();
    f.ok(&["one", "link"]);
    assert_eq!(f.target("link"), Path::new("one"));
    assert_eq!(fs::symlink_metadata(f.path("link")).unwrap().ino(), inode);
    let registry = f.registry();
    f.ok(&["one", "link"]);
    assert_eq!(f.registry(), registry);
    assert_eq!(f.run(&["two", "link"]).status.code(), Some(1));
    assert_eq!(f.registry(), registry);
    f.ok(&["-f", "two", "link"]);
    assert_eq!(f.target("link"), f.path("two"));
    assert!(f.registry().contains(f.path("two").to_str().unwrap()));
    fs::remove_file(f.path("link")).unwrap();
    f.ok(&["three", "link"]);
    assert_eq!(f.target("link"), f.path("three"));
    symlink("old", f.path("unmanaged")).unwrap();
    f.ok(&["--force", "three", "unmanaged"]);
    assert_eq!(f.target("unmanaged"), f.path("three"));
}

#[test]
fn force_uses_exact_link_location_and_preserves_real_files() {
    let f = Fixture::new();
    fs::create_dir(f.path("directory")).unwrap();
    fs::write(f.path("file"), "keep").unwrap();
    for name in ["directory", "file"] {
        assert_eq!(f.run(&["-f", "source", name]).status.code(), Some(1));
    }
    assert_eq!(fs::read_to_string(f.path("file")).unwrap(), "keep");
    symlink("directory", f.path("alias")).unwrap();
    f.ok(&["-f", "source", "alias"]);
    assert_eq!(f.target("alias"), f.path("source"));
    assert_eq!(fs::read_dir(f.path("directory")).unwrap().count(), 0);
}

#[test]
fn references_preserve_symlink_chains_and_meaningful_parent_components() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("tree/child")).unwrap();
    fs::write(f.path("tree/data"), "correct").unwrap();
    fs::write(f.path("data"), "wrong").unwrap();
    symlink("tree/child", f.path("alias")).unwrap();
    f.ok(&["alias/../data", "first"]);
    assert_eq!(f.target("first"), f.path("alias/../data"));
    f.ok(&["first", "second"]);
    assert_eq!(f.target("second"), f.path("first"));
    assert_eq!(fs::read_to_string(f.path("second")).unwrap(), "correct");
    f.ok(&["check"]);
    assert_eq!(f.run(&["tree/data", "second"]).status.code(), Some(1));
    for args in [["self", "self"], ["./other", "other"]] {
        assert_eq!(f.run(&args).status.code(), Some(1));
    }
}

#[test]
fn adopt_preserves_relative_links_updates_registry_and_fix_restores_absolute() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("tree/bin")).unwrap();
    fs::write(f.path("tree/python"), "python").unwrap();
    symlink("../python", f.path("tree/bin/python3")).unwrap();
    symlink("tree/bin", f.path("alias")).unwrap();
    f.ok(&["adopt", "alias/python3"]);
    assert_eq!(f.target("alias/python3"), Path::new("../python"));
    let out = f.ok(&["check", "-otsv"]);
    let out = String::from_utf8(out.stdout).unwrap();
    let fields: Vec<_> = out.lines().nth(1).unwrap().split('\t').collect();
    assert_eq!(fields[0], "MATCH");
    assert_eq!(
        serde_json::from_str::<String>(fields[3]).unwrap(),
        f.path("tree/python").to_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<String>(fields[5]).unwrap(),
        "../python"
    );
    fs::remove_file(f.path("alias/python3")).unwrap();
    f.ok(&["fix"]);
    assert_eq!(f.target("alias/python3"), f.path("tree/python"));
    fs::remove_file(f.path("alias/python3")).unwrap();
    symlink("different", f.path("alias/python3")).unwrap();
    f.ok(&["adopt", "alias/python3"]);
    assert!(f
        .registry()
        .contains(f.path("tree/bin/different").to_str().unwrap()));
    f.ok(&["remove", "alias/python3"]);
}

#[test]
fn readlink_tilde_is_literal_and_target_directory_suffixes_survive() {
    let f = Fixture::new();
    symlink("~/source", f.path("existing")).unwrap();
    f.ok(&["adopt", "existing"]);
    assert!(f.registry().contains(f.path("~/source").to_str().unwrap()));
    fs::write(f.path("file"), "data").unwrap();
    for (target, link) in [("file/", "slash"), ("file/.", "dot")] {
        f.ok(&[target, link]);
        assert_eq!(f.target(link).as_os_str(), f.path(target).as_os_str());
        assert_eq!(f.run(&["check", link]).status.code(), Some(1));
    }
}

#[test]
fn scan_defaults_to_cwd_is_shallow_and_recurses_only_with_flag() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("github/V.workflow")).unwrap();
    fs::create_dir_all(f.path("Library/Services")).unwrap();
    symlink("missing", f.path("github/V.workflow/P.workflow")).unwrap();
    f.ok(&["github/V.workflow", "Library/Services/V.workflow"]);
    let plain = f
        .command(&["scan"])
        .current_dir(f.path("github"))
        .output()
        .unwrap();
    assert!(plain.status.success());
    assert!(String::from_utf8_lossy(&plain.stdout).contains("No symlinks found"));
    let recursive = f.ok(&["scan", "-R", "./github"]);
    let text = String::from_utf8(recursive.stdout).unwrap();
    assert!(text.contains("1 symlink found: 0 managed, 1 unmanaged"));
    assert!(!text.contains("/./"));
    let services = String::from_utf8(f.ok(&["scan", "-R", "Library/Services"]).stdout).unwrap();
    assert!(services.contains("1 symlink found: 1 managed, 0 unmanaged"));
    assert!(!services.contains("P.workflow"));
    f.ok(&["check"]);
}

#[test]
fn dry_run_shares_plan_without_false_missing_warning_or_writes() {
    let f = Fixture::new();
    fs::write(f.path("source"), "ok").unwrap();
    let output = f.ok(&["-np", "source", "deep/link"]);
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("target is missing"));
    assert!(!f.path("deep").exists());
    assert!(!f.path("config").exists());
    symlink("old", f.path("link")).unwrap();
    f.ok(&["-fn", "source", "link"]);
    assert_eq!(f.target("link"), Path::new("old"));
    assert!(!f.path("config").exists());
}

#[test]
fn force_recovery_keeps_command_authority_and_finishes_each_stage() {
    for stage in ["prepared", "moved", "linked", "committed", "cleaned"] {
        let f = Fixture::new();
        f.ok(&["old", "link"]);
        f.crash(&["-f", "new", "link"], stage);
        assert_eq!(f.run(&["fix", "-f"]).status.code(), Some(2));
        assert_eq!(f.run(&["new", "link"]).status.code(), Some(2));
        f.ok(&["-f", "new", "link"]);
        assert_eq!(f.target("link"), f.path("new"));
        assert!(!f
            .registry_path()
            .with_file_name("links.toml.slink-pending")
            .exists());
    }
}

#[test]
fn unregister_does_not_require_an_inspectable_link_parent() {
    let f = Fixture::new();
    f.write_entries(&[("missing/../link", "source")]);
    f.ok(&["remove", "-k", "missing/../link"]);
    assert_eq!(f.entries(), 0);
    assert!(!f.path("missing").exists());
}
