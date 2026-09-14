mod support;

use std::fs;
use support::Fixture;

fn run_chain(entries: &[(&str, &str)]) -> (String, String) {
    let f = Fixture::new();
    fs::create_dir_all(f.path("source").parent().unwrap()).unwrap();
    f.write("source", "ok");
    fs::create_dir_all(f.path("managed")).unwrap();
    f.write_entries(entries);

    let output = f.ok(&["fix"]);
    assert_eq!(f.target("managed/base"), f.path("source"));
    assert_eq!(f.target("top"), f.path("managed/base"));

    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn fix_chained_links_is_independent_of_registry_order() {
    let dependency_first = run_chain(&[("managed/base", "source"), ("top", "managed/base")]);
    let dependent_first = run_chain(&[("top", "managed/base"), ("managed/base", "source")]);

    assert_eq!(dependent_first, dependency_first);
    assert!(!dependent_first.0.contains("target is missing"));
    assert!(!dependent_first.1.contains("target MISSING"));
}
