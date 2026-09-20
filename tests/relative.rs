use std::{fs, os::unix::fs::symlink};

mod support;
use support::Fixture;

fn registered_target(f: &Fixture) -> String {
    let doc = f.registry().parse::<toml_edit::DocumentMut>().unwrap();
    doc["link"].as_array_of_tables().unwrap().get(0).unwrap()["target"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn write_raw_entry(f: &Fixture, link: &str, target: &str) {
    let mut doc = toml_edit::DocumentMut::new();
    let mut tables = toml_edit::ArrayOfTables::new();
    let mut table = toml_edit::Table::new();
    table["link"] = toml_edit::value(f.path(link).to_str().unwrap());
    table["target"] = toml_edit::value(target);
    tables.push(table);
    doc["link"] = toml_edit::Item::ArrayOfTables(tables);
    f.write_registry(&doc.to_string());
}

#[test]
fn relative_create_registers_and_restores_the_same_target_text() {
    let f = Fixture::new();
    f.write("source", "data");

    f.ok(&["-r", "source", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../source");
    assert_eq!(registered_target(&f), "../source");

    fs::remove_file(f.path("home/link")).unwrap();
    f.ok(&["fix", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../source");
}

#[test]
fn adopt_canonicalizes_registration_without_rewriting_the_symlink() {
    let f = Fixture::new();
    fs::create_dir(f.path("dir")).unwrap();
    symlink("../dir/./", f.path("home/link")).unwrap();

    f.ok(&["adopt", "home/link"]);

    assert_eq!(f.target("home/link").to_str().unwrap(), "../dir/./");
    assert_eq!(registered_target(&f), "../dir/");
}

#[test]
fn canonical_directory_suffix_matches_and_force_enforces_registered_text() {
    let f = Fixture::new();
    fs::create_dir(f.path("dir")).unwrap();
    symlink("../dir/.", f.path("home/link")).unwrap();
    write_raw_entry(&f, "home/link", "../dir/");

    assert!(f.run(&["check"]).status.success());

    f.ok(&["fix", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../dir/.");

    f.ok(&["fix", "-f", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../dir/");
}

#[test]
fn force_can_convert_an_equivalent_absolute_link_to_relative() {
    let f = Fixture::new();
    f.write("source", "data");
    symlink(f.path("source"), f.path("home/link")).unwrap();

    f.ok(&["-r", "source", "home/link"]);
    assert_eq!(f.target("home/link"), f.path("source"));
    assert_eq!(registered_target(&f), "../source");

    f.ok(&["-rf", "source", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../source");
}

#[test]
fn relative_generation_uses_the_physical_link_parent_and_missing_parents() {
    let f = Fixture::new();
    fs::create_dir(f.path("real")).unwrap();
    f.write("real/source", "data");
    symlink("real", f.path("alias")).unwrap();

    f.ok(&["-r", "real/source", "alias/link"]);
    assert_eq!(f.target("real/link").to_str().unwrap(), "source");

    f.write("source", "data");
    f.ok(&["-rp", "source", "missing/sub/link"]);
    assert_eq!(
        f.target("missing/sub/link").to_str().unwrap(),
        "../../source"
    );
}

#[test]
fn relative_create_recovery_requires_the_same_representation_option() {
    let f = Fixture::new();
    f.write("source", "data");
    f.crash(&["-r", "source", "home/link"], "linked");

    let wrong = f.run(&["source", "home/link"]);
    assert_eq!(wrong.status.code(), Some(2));
    assert!(f.path("config/slink/links.toml.slink-pending").exists());

    f.ok(&["-r", "source", "home/link"]);
    assert_eq!(f.target("home/link").to_str().unwrap(), "../source");
    assert!(!f.path("config/slink/links.toml.slink-pending").exists());
}

#[test]
fn relative_is_create_only() {
    let f = Fixture::new();
    assert_eq!(f.run(&["fix", "-r"]).status.code(), Some(2));
    assert_eq!(f.run(&["adopt", "-r", "home/link"]).status.code(), Some(2));
}
