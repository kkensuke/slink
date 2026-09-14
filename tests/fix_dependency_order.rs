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

#[test]
fn dry_run_reports_a_shared_missing_parent_once() {
    let f = Fixture::new();
    f.write("source", "ok");
    f.write_entries(&[("shared/a", "source"), ("shared/b", "source")]);

    let output = f.ok(&["fix", "-n", "-p"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(stdout.matches("parent: ").count(), 1, "stdout={stdout:?}");
    assert!(stdout.contains("(would create)"));
    assert!(!f.path("shared").exists());
}

#[test]
fn fix_orders_a_managed_path_dependency_before_its_dependent() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("source-dir")).unwrap();
    f.write("source-dir/child", "ok");
    fs::create_dir_all(f.path("managed")).unwrap();
    f.write_entries(&[
        ("top", "managed/base/child"),
        ("managed/base", "source-dir"),
    ]);

    let output = f.ok(&["fix"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let base = stdout.find("managed/base").unwrap();
    let top = stdout.find("top").unwrap();

    assert!(base < top, "stdout={stdout:?}");
}

#[test]
fn dry_run_preserves_parent_traversal_after_a_projected_symlink() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("source-dir/subdir")).unwrap();
    f.write("source-dir/sibling", "ok");
    fs::create_dir_all(f.path("managed")).unwrap();
    f.write_entries(&[
        ("managed/base", "source-dir/subdir"),
        ("top", "managed/base/../sibling"),
    ]);

    let output = f.ok(&["fix", "-n"]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains("target is missing"), "stdout={stdout:?}");
}

#[test]
fn fix_orders_every_managed_link_traversed_by_one_target() {
    let f = Fixture::new();
    fs::create_dir_all(f.path("subdir")).unwrap();
    fs::create_dir_all(f.path("source-z")).unwrap();
    f.write("source-z/child", "ok");
    f.write_entries(&[
        ("m-top", "a-link/../z-link/child"),
        ("z-link", "source-z"),
        ("a-link", "subdir"),
    ]);

    let output = f.ok(&["fix"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let a = stdout.find("a-link").unwrap();
    let z = stdout.find("z-link").unwrap();
    let top = stdout.find("m-top").unwrap();

    assert!(a < top, "stdout={stdout:?}");
    assert!(z < top, "stdout={stdout:?}");
}

#[test]
fn fix_dependency_order_uses_the_planned_replacement_target() {
    use std::os::unix::fs::symlink;

    let f = Fixture::new();
    fs::create_dir_all(f.path("old/place")).unwrap();
    fs::create_dir_all(f.path("new/place")).unwrap();
    fs::create_dir_all(f.path("source-z")).unwrap();
    f.write("source-z/child", "ok");
    symlink(f.path("old/place"), f.path("a-link")).unwrap();
    f.write_entries(&[
        ("m-top", "a-link/../z-link/child"),
        ("z-link", "source-z"),
        ("a-link", "new/place"),
    ]);

    let output = f.ok(&["fix", "-f"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let a = stdout.find("a-link").unwrap();
    let z = stdout.find("z-link").unwrap();
    let top = stdout.find("m-top").unwrap();

    assert!(a < top, "stdout={stdout:?}");
    assert!(z < top, "stdout={stdout:?}");
    assert_eq!(f.target("a-link"), f.path("new/place"));
}
