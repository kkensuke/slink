mod support;

use std::{
    fs,
    os::unix::fs::{symlink, MetadataExt},
    path::Path,
};
use support::Fixture;

#[test]
fn unregister_preserves_symlinks_files_and_directories() {
    let f = Fixture::new();
    f.write("source", "source data");
    f.write("other", "other data");
    symlink("source", f.path("matching")).unwrap();
    symlink("other", f.path("different")).unwrap();
    f.write("file", "keep file");
    fs::create_dir(f.path("directory")).unwrap();
    f.write("directory/child", "keep child");
    f.write_entries(&[
        ("matching", "source"),
        ("different", "source"),
        ("missing", "source"),
        ("file", "source"),
        ("directory", "source"),
    ]);
    let inode = fs::symlink_metadata(f.path("matching")).unwrap().ino();

    let output = f.ok(&[
        "unregister",
        "matching",
        "different",
        "missing",
        "file",
        "directory",
    ]);

    assert_eq!(f.entries(), 0);
    assert_eq!(f.target("matching"), Path::new("source"));
    assert_eq!(f.target("different"), Path::new("other"));
    assert_eq!(
        fs::symlink_metadata(f.path("matching")).unwrap().ino(),
        inode
    );
    assert!(fs::symlink_metadata(f.path("missing")).is_err());
    assert_eq!(fs::read_to_string(f.path("file")).unwrap(), "keep file");
    assert_eq!(
        fs::read_to_string(f.path("directory/child")).unwrap(),
        "keep child"
    );
    assert_eq!(fs::read_to_string(f.path("source")).unwrap(), "source data");
    assert_eq!(fs::read_to_string(f.path("other")).unwrap(), "other data");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("— unregistered").count(), 5);
    assert!(stdout.ends_with("5 changed, 0 unchanged\n"));
}

#[test]
fn unregister_previews_a_batch_and_only_unregisters_selected_links() {
    let f = Fixture::new();
    f.write("source", "keep");
    for name in ["a", "b", "untouched"] {
        f.ok(&["source", name]);
    }
    let before = f.registry();

    let preview = f.ok(&["unregister", "-n", "a", "b"]);
    let stdout = String::from_utf8(preview.stdout).unwrap();
    assert_eq!(stdout.matches("would unregister").count(), 2);
    assert_eq!(f.registry(), before);
    assert!(!f.path("config/slink/links.toml.slink-pending").exists());

    f.ok(&["unregister", "a", "b"]);
    assert_eq!(f.entries(), 1);
    for name in ["a", "b", "untouched"] {
        assert_eq!(f.target(name), f.path("source"));
    }
    f.ok(&["check", "untouched"]);
    assert_eq!(f.run(&["check", "a"]).status.code(), Some(2));
}

#[test]
fn failed_removal_keeps_the_registration_until_explicitly_unregistered() {
    let f = Fixture::new();
    f.ok(&["source", "link"]);
    fs::remove_file(f.path("link")).unwrap();
    symlink("different", f.path("link")).unwrap();
    let before = f.registry();

    let output = f.run(&["remove", "link"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(f.registry(), before);
    assert_eq!(f.target("link"), Path::new("different"));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("use unregister"));

    f.ok(&["unregister", "link"]);
    assert_eq!(f.entries(), 0);
    assert_eq!(f.target("link"), Path::new("different"));
}

#[test]
fn unregister_cannot_discard_pending_removal() {
    let f = Fixture::new();
    f.ok(&["source", "link"]);
    f.crash(&["remove", "link"], "moved");
    let before = f.registry();
    let pending_path = f.path("config/slink/links.toml.slink-pending");
    let pending = fs::read(&pending_path).unwrap();

    for args in [vec!["unregister", "link"], vec!["unregister", "-n", "link"]] {
        assert_eq!(f.run(&args).status.code(), Some(2));
        assert_eq!(f.registry(), before);
        assert_eq!(fs::read(&pending_path).unwrap(), pending);
        assert!(fs::symlink_metadata(f.path("link")).is_err());
    }
    f.ok(&["remove", "link"]);
    assert_eq!(f.entries(), 0);
    assert!(!pending_path.exists());
}

#[test]
fn unregister_requires_paths_and_accepts_only_dry_run() {
    let f = Fixture::new();
    f.ok(&["source", "link"]);
    let before = f.registry();

    for args in [
        vec!["unregister"],
        vec!["unregister", "-f", "link"],
        vec!["unregister", "-p", "link"],
        vec!["unregister", "-R", "link"],
        vec!["unregister", "-o", "tsv", "link"],
        vec!["remove", "-k", "link"],
        vec!["remove", "--keep-link", "link"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
        assert_eq!(f.registry(), before);
        assert_eq!(f.target("link"), f.path("source"));
    }

    let help = String::from_utf8(f.ok(&["--help"]).stdout).unwrap();
    assert!(help.contains("slink unregister [-n] <link ...>"));
    assert!(help.contains("slink remove [-n] <link ...>"));
    assert!(!help.contains("--keep-link"));
}
