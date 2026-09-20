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
    for entry in doc["link"].as_array_of_tables().unwrap() {
        assert!(Path::new(entry["link"].as_str().unwrap()).is_absolute());
        assert!(Path::new(entry["target"].as_str().unwrap()).is_absolute());
    }
}

#[test]
fn command_options_are_validated_and_output_formats_can_be_selected() {
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
fn registry_requires_absolute_links_but_allows_relative_targets() {
    let f = Fixture::new();
    for value in ["relative", "~/link"] {
        f.write_registry(&format!(
            "[[link]]\nlink = {value:?}\ntarget = {:?}\n",
            f.path("source")
        ));
        let before = f.registry();
        let output = f.run(&["fix"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("registry link must be an absolute path"));
        assert_eq!(before, f.registry());
    }

    f.write("source", "data");
    symlink("../source", f.path("home/link")).unwrap();
    f.write_registry(&format!(
        "[[link]]\nlink = {:?}\ntarget = '../source'\n",
        f.path("home/link")
    ));
    f.ok(&["check"]);
}

#[test]
fn relative_registry_targets_do_not_depend_on_the_working_directory() {
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
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    assert!(second.stderr.is_empty());
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
fn adopt_preserves_relative_links_updates_registry_and_fix_restores_relative() {
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
    assert_eq!(fields[1], "MATCH");
    assert_eq!(
        serde_json::from_str::<String>(fields[2]).unwrap(),
        "../python"
    );
    assert_eq!(
        serde_json::from_str::<String>(fields[4]).unwrap(),
        "../python"
    );
    fs::remove_file(f.path("alias/python3")).unwrap();
    f.ok(&["fix"]);
    assert_eq!(f.target("alias/python3"), Path::new("../python"));
    fs::remove_file(f.path("alias/python3")).unwrap();
    symlink("different", f.path("alias/python3")).unwrap();
    f.ok(&["adopt", "alias/python3"]);
    assert!(f.registry().contains("target = \"different\""));
    f.ok(&["remove", "alias/python3"]);
}

#[test]
fn readlink_tilde_is_literal_and_target_directory_suffixes_survive() {
    let f = Fixture::new();
    symlink("~/source", f.path("existing")).unwrap();
    f.ok(&["adopt", "existing"]);
    assert!(f.registry().contains("target = \"~/source\""));
    fs::write(f.path("file"), "data").unwrap();
    let canonical = format!("{}/", f.path("file").display());
    for (target, link) in [("file/", "slash"), ("file/.", "dot")] {
        let created = String::from_utf8(f.ok(&[target, link]).stdout).unwrap();
        assert!(created.contains(&format!("  → {canonical:?}\n")));
        assert_eq!(f.target(link).to_str().unwrap(), canonical);
        let checked = f.run(&["check", link]);
        assert_eq!(checked.status.code(), Some(1));
        assert!(String::from_utf8(checked.stdout)
            .unwrap()
            .contains(&format!("  target: {canonical:?}\n")));
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
    f.ok(&["unregister", "missing/../link"]);
    assert_eq!(f.entries(), 0);
    assert!(!f.path("missing").exists());
}


#[test]
fn registry_home_expressions_validate_to_concrete_paths() {
    let f = Fixture::new();
    fs::write(f.path("home/source"), "ok").unwrap();
    f.write_registry(
        "[[link]]\nlink = \"${HOME}/link\"\ntarget = \"${HOME}/source\"\n",
    );

    f.ok(&["fix"]);
    assert_eq!(f.target("home/link"), f.path("home/source"));
    assert!(f.registry().contains("link = \"${HOME}/link\""));
    assert!(f.registry().contains("target = \"${HOME}/source\""));
}

#[test]
fn home_expression_write_style_applies_only_to_written_entries() {
    let f = Fixture::new();
    fs::write(f.path("home/source"), "ok").unwrap();
    let old_link = f.path("old-link");
    let old_target = f.path("old-target");
    f.write_registry(&format!(
        "[format]\nhome = \"expression\"\n\n[[link]]\nlink = {:?}\ntarget = {:?}\n",
        old_link.to_str().unwrap(),
        old_target.to_str().unwrap(),
    ));

    f.ok(&["~/source", "~/new-link"]);
    let registry = f.registry();

    assert!(registry.contains(&format!("link = {:?}", old_link.to_str().unwrap())));
    assert!(registry.contains(&format!("target = {:?}", old_target.to_str().unwrap())));
    assert!(registry.contains("link   = \"${HOME}/new-link\""));
    assert!(registry.contains("target = \"${HOME}/source\""));
    assert_eq!(f.target("home/new-link"), f.path("home/source"));
}

#[test]
fn home_expression_write_style_never_changes_relative_target_style() {
    let f = Fixture::new();
    fs::create_dir(f.path("home/bin")).unwrap();
    fs::write(f.path("home/source"), "ok").unwrap();
    f.write_registry("[format]\nhome = \"expression\"\n");

    f.ok(&["-r", "~/source", "~/bin/link"]);

    let registry = f.registry();
    assert!(registry.contains("link   = \"${HOME}/bin/link\""));
    assert!(registry.contains("target = \"../source\""));
    assert_eq!(f.target("home/bin/link"), Path::new("../source"));
}

#[test]
fn reserved_home_target_spellings_are_rejected_for_registry_and_relative_create() {
    let f = Fixture::new();
    let reserved = "./${HOME}/foo";
    f.write_registry(&format!(
        "[[link]]\nlink = {:?}\ntarget = {:?}\n",
        f.path("link").to_str().unwrap(),
        reserved,
    ));
    let check = f.run(&["check"]);
    assert_eq!(check.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&check.stderr).contains("reserved registry HOME syntax"));

    fs::remove_file(f.registry_path()).unwrap();
    let create = f.run(&["-r", "${HOME}/foo", "link"]);
    assert_eq!(create.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&create.stderr).contains("reserved registry HOME syntax"));
    assert!(!f.path("link").exists());
}

#[test]
fn adopt_reencodes_reserved_home_target_without_rewriting_existing_symlink() {
    let f = Fixture::new();
    symlink("${HOME}/foo", f.path("link")).unwrap();

    f.ok(&["adopt", "link"]);
    assert_eq!(f.target("link"), Path::new("${HOME}/foo"));

    let safe = f.path("${HOME}/foo");
    assert!(f.registry().contains(&format!("target = {:?}", safe.to_str().unwrap())));

    fs::remove_file(f.path("link")).unwrap();
    f.ok(&["fix", "link"]);
    assert_eq!(f.target("link"), safe);
}

#[test]
fn expression_style_can_serialize_safe_adopted_absolute_target_under_home() {
    let f = Fixture::new();
    symlink("${HOME}/foo", f.path("home/link")).unwrap();
    f.write_registry("[format]\nhome = \"expression\"\n");

    f.ok(&["adopt", "~/link"]);
    assert_eq!(f.target("home/link"), Path::new("${HOME}/foo"));
    assert!(f
        .registry()
        .contains("target = \"${HOME}/${HOME}/foo\""));
}
