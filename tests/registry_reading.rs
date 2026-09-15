mod support;
use std::{fs, os::unix::fs::symlink};
use support::Fixture;

#[test]
fn new_entries_align_keys_and_existing_entries_keep_their_formatting() {
    for newline in ["\n", "\r\n"] {
        let f = Fixture::new();
        f.write("source", "one");
        f.write("other", "two");
        symlink("other", f.path("manual")).unwrap();
        let original = format!(
            "# hand edited\n[[link]]\nlink = '{}' # location\ntarget    = {:?}\n",
            f.path("manual").display(),
            f.path("source").to_str().unwrap(),
        )
        .replace('\n', newline);
        f.write_registry(&original);

        f.ok(&["adopt", "manual"]);
        let updated = original.replace(
            f.path("source").to_str().unwrap(),
            f.path("other").to_str().unwrap(),
        );
        assert_eq!(f.registry(), updated);

        f.ok(&["source", "created"]);
        symlink("source", f.path("adopted")).unwrap();
        f.ok(&["adopt", "adopted"]);
        let registry = f.registry();
        assert!(registry.starts_with(&updated));
        for name in ["created", "adopted"] {
            assert!(registry.contains(&format!(
                "link   = {:?}{newline}target = {:?}{newline}",
                f.path(name).to_str().unwrap(),
                f.path("source").to_str().unwrap(),
            )));
        }
        if newline == "\r\n" {
            assert!(!registry.replace("\r\n", "").contains('\n'));
        }
    }
}

#[test]
fn empty_registries_support_listing_and_mutation() {
    let f = Fixture::new();
    for text in ["", "# no entries\n"] {
        f.write_registry(text);
        assert_eq!(f.ok(&["list"]).stdout, b"0 links\n");
        assert_eq!(f.ok(&["check"]).stdout, b"OK 0 links\n");
        assert_eq!(f.registry(), text);
    }
    f.write("source", "data");
    f.ok(&["source", "link"]);
    f.ok(&["check"]);
    f.ok(&["remove", "link"]);
    assert_eq!(f.entries(), 0);
    f.ok(&["check"]);
    assert_eq!(fs::read_to_string(f.path("source")).unwrap(), "data");
}

#[test]
fn list_shows_all_stored_strings_and_check_identifies_invalid_paths() {
    let f = Fixture::new();
    for (field, value) in [
        ("target", "PhD"),
        ("target", "~/source"),
        ("target", ""),
        ("target", "a\0b"),
        ("link", "relative"),
        ("link", "~/link"),
        ("link", ""),
        ("link", "/bad/"),
    ] {
        f.write_entries(&[
            ("first", "source"),
            ("middle", "source"),
            ("last", "source"),
        ]);
        let mut doc = f.registry().parse::<toml_edit::DocumentMut>().unwrap();
        doc["link"]
            .as_array_of_tables_mut()
            .unwrap()
            .get_mut(1)
            .unwrap()[field] = toml_edit::value(value);
        f.write_registry(&doc.to_string());
        let before = f.registry();

        let human = f.ok(&["list"]);
        assert!(human.stdout.starts_with(b"3 links\n"));
        assert!(human.stderr.is_empty());
        let tsv = f.ok(&["list", "-o", "tsv"]);
        assert!(tsv.stderr.is_empty());
        let output = String::from_utf8(tsv.stdout).unwrap();
        let rows: Vec<Vec<String>> = output
            .lines()
            .skip(1)
            .map(|line| {
                line.split('\t')
                    .map(|cell| serde_json::from_str(cell).unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1][usize::from(field == "target")], value);
        assert_eq!(rows[2][0], f.path("last").to_str().unwrap());

        let check = f.run(&["check"]);
        assert_eq!(check.status.code(), Some(2));
        let error = String::from_utf8(check.stderr).unwrap();
        assert!(error.contains(f.registry_path().to_str().unwrap()));
        assert!(error.contains("[[link]] entry 2"), "{error}");
        assert_eq!(f.registry(), before);
        assert!(!f.path("config/slink/links.toml.slink-lock").exists());
    }
}

#[test]
fn listing_invalid_paths_does_not_relax_mutation_validation() {
    let f = Fixture::new();
    f.write("source", "keep");
    symlink(f.path("source"), f.path("link")).unwrap();
    f.write_entries(&[("link", "source"), ("bad", "source")]);
    let mut doc = f.registry().parse::<toml_edit::DocumentMut>().unwrap();
    doc["link"]
        .as_array_of_tables_mut()
        .unwrap()
        .get_mut(1)
        .unwrap()["target"] = toml_edit::value("PhD");
    f.write_registry(&doc.to_string());
    let before = f.registry();
    f.ok(&["list"]);
    for args in [
        vec!["other", "new-link"],
        vec!["-f", "other", "link"],
        vec!["fix", "-f"],
        vec!["fix", "-n"],
        vec!["adopt", "link"],
        vec!["remove", "link"],
        vec!["unregister", "link"],
        vec!["scan"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
        assert_eq!(f.registry(), before);
        assert_eq!(f.target("link"), f.path("source"));
        assert!(!f.path("new-link").exists());
        assert!(!f.path("config/slink/links.toml.slink-lock").exists());
        assert!(!f.path("config/slink/links.toml.slink-pending").exists());
    }
    assert_eq!(fs::read_to_string(f.path("source")).unwrap(), "keep");
}

#[test]
fn unreadable_entry_structure_is_reported_without_a_partial_list() {
    let f = Fixture::new();
    for invalid in [
        "target = [",
        "target = 42\n",
        "# missing target\n",
        "target = '/source'\nextra = true\n",
    ] {
        f.write_registry(&format!(
            "[[link]]\nlink = '/first'\ntarget = '/source'\n\n[[link]]\nlink = '/second'\n{invalid}"
        ));
        for args in [vec!["list"], vec!["list", "-otsv"], vec!["check"]] {
            let output = f.run(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8(output.stderr)
                .unwrap()
                .contains(f.registry_path().to_str().unwrap()));
        }
    }
}
