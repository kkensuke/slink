mod support;

use std::fs;
use support::Fixture;

fn normalized_output(f: &Fixture, output: std::process::Output) -> (String, String) {
    let root = f.root.to_string_lossy();
    (
        String::from_utf8(output.stdout)
            .unwrap()
            .replace(root.as_ref(), "$ROOT"),
        String::from_utf8(output.stderr)
            .unwrap()
            .replace(root.as_ref(), "$ROOT"),
    )
}

fn setup_chain(f: &Fixture, entries: &[(&str, &str)]) {
    f.write("source", "ok");
    fs::create_dir_all(f.path("managed")).unwrap();
    f.write_entries(entries);
}

fn run_chain(entries: &[(&str, &str)]) -> (String, String) {
    let f = Fixture::new();
    setup_chain(&f, entries);

    let output = f.ok(&["fix"]);
    assert_eq!(f.target("managed/base"), f.path("source"));
    assert_eq!(f.target("top"), f.path("managed/base"));

    normalized_output(&f, output)
}

fn preview_chain(entries: &[(&str, &str)]) -> (String, String) {
    let f = Fixture::new();
    setup_chain(&f, entries);

    let output = f.ok(&["fix", "-n"]);
    assert!(fs::symlink_metadata(f.path("managed/base")).is_err());
    assert!(fs::symlink_metadata(f.path("top")).is_err());

    normalized_output(&f, output)
}

#[test]
fn fix_chained_links_is_independent_of_registry_order() {
    let dependency_first = run_chain(&[("managed/base", "source"), ("top", "managed/base")]);
    let dependent_first = run_chain(&[("top", "managed/base"), ("managed/base", "source")]);

    assert_eq!(dependent_first, dependency_first);
    assert!(!dependent_first.0.contains("target is missing"));
    assert!(!dependent_first.1.contains("target MISSING"));
}

#[test]
fn dry_run_chained_links_uses_the_projected_final_state() {
    let dependency_first = preview_chain(&[("managed/base", "source"), ("top", "managed/base")]);
    let dependent_first = preview_chain(&[("top", "managed/base"), ("managed/base", "source")]);

    assert_eq!(dependent_first, dependency_first);
    assert!(dependent_first.0.contains("would create"));
    assert!(!dependent_first.0.contains("target is missing"));
    assert!(dependent_first.1.is_empty());
}

#[test]
fn dry_run_resolves_suffix_through_a_projected_managed_link() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("source-dir")).unwrap();
    f.write("source-dir/child", "ok");
    fs::create_dir_all(f.path("managed")).unwrap();
    f.write_entries(&[
        ("managed/base", "source-dir"),
        ("top", "managed/base/child"),
    ]);

    let output = f.ok(&["fix", "-n"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("target is missing"), "stdout={stdout:?}");
    assert!(fs::symlink_metadata(f.path("managed/base")).is_err());
    assert!(fs::symlink_metadata(f.path("top")).is_err());
}
