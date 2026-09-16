mod support;
use std::{fs, os::unix::fs::symlink};
use support::Fixture;

#[test]
fn creation_adoption_and_removal_share_the_human_layout() {
    let f = Fixture::new();
    f.write("source", "data");
    for (args, label) in [
        (vec!["source", "~/link"], "created"),
        (vec!["adopt", "~/link"], "unchanged"),
        (vec!["unregister", "~/link"], "unregistered"),
        (vec!["adopt", "~/link"], "registered"),
        (vec!["remove", "~/link"], "removed"),
    ] {
        let output = f.ok(&args);
        assert!(output.stderr.is_empty());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("✓ ~/link — {label}\n  → {:?}\n", f.path("source"))
        );
    }
    assert!(fs::symlink_metadata(f.path("home/link")).is_err());
    assert_eq!(fs::read_to_string(f.path("source")).unwrap(), "data");
}

#[test]
fn fix_summarizes_healthy_unchanged_links_but_keeps_target_problems_visible() {
    let f = Fixture::new();
    f.write("source", "data");
    f.ok(&["source", "~/healthy"]);
    f.ok(&["source", "~/missing"]);
    f.ok(&["future", "~/broken"]);
    fs::remove_file(f.path("home/missing")).unwrap();

    let output = f.ok(&["fix"]);
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("~/healthy"));
    assert!(text.contains("✓ ~/missing — created\n"));
    assert!(text.contains("! ~/broken — unchanged; target is missing\n"));
    assert!(text.ends_with("1 changed, 2 unchanged, 1 target issue\n"));

    f.write("future", "data");
    assert_eq!(f.ok(&["fix"]).stdout, b"0 changed, 3 unchanged\n");
    assert_eq!(
        f.ok(&["fix", "-n"]).stdout,
        b"0 changes planned, 3 unchanged\n"
    );
}

#[test]
fn target_mismatch_failures_share_check_details_without_changing_links_or_registration() {
    for relative in [false, true] {
        let f = Fixture::new();
        f.write("expected", "expected");
        f.write("home/actual", "actual");
        let actual = if relative {
            "actual".to_owned()
        } else {
            f.path("home/actual").to_str().unwrap().to_owned()
        };
        symlink(&actual, f.path("home/link")).unwrap();
        f.write_entries(&[("home/link", "expected")]);
        f.ok(&["expected", "~/healthy"]);
        let registry = f.registry();
        let details = format!(
            "  expected: {:?}\n  actual:   {actual:?}\n",
            f.path("expected")
        );

        let check = f.run(&["check"]);
        assert_eq!(check.status.code(), Some(1));
        assert!(String::from_utf8(check.stdout).unwrap().contains(&details));
        assert!(check.stderr.is_empty());
        let scan = String::from_utf8(f.ok(&["scan", "home"]).stdout).unwrap();
        let indented_details = details
            .lines()
            .map(|line| format!("  {line}\n"))
            .collect::<String>();
        assert!(scan.contains(&indented_details));

        for (args, summary) in [
            (vec!["fix"], "0 changed, 1 unchanged, 1 failed\n"),
            (
                vec!["fix", "-n"],
                "0 changes planned, 1 unchanged, 1 failed\n",
            ),
            (vec!["expected", "~/link"], ""),
            (vec!["-n", "expected", "~/link"], ""),
        ] {
            let output = f.run(&args);
            assert_eq!(output.status.code(), Some(1), "{args:?}");
            assert_eq!(String::from_utf8(output.stdout).unwrap(), summary);
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                format!(
                    "! ~/link — failed: target differs\n{details}  hint:     use --force (-f) to replace this symlink\n\n"
                )
            );
            assert_eq!(f.target("home/link").to_str().unwrap(), actual);
            assert_eq!(f.registry(), registry);
        }
    }
}

#[test]
fn fix_force_keeps_preview_and_success_output_after_a_mismatch() {
    let f = Fixture::new();
    f.write("expected", "expected");
    f.write("home/actual", "actual");
    symlink("actual", f.path("home/link")).unwrap();
    f.write_entries(&[("home/link", "expected")]);
    let registry = f.registry();

    let preview = f.ok(&["fix", "-fn"]);
    assert!(preview.stderr.is_empty());
    assert_eq!(
        String::from_utf8(preview.stdout).unwrap(),
        format!(
            "○ ~/link — would replace\n  → {:?}\n\n1 change planned, 0 unchanged\n",
            f.path("expected")
        )
    );
    assert_eq!(f.target("home/link").to_str().unwrap(), "actual");
    assert_eq!(f.registry(), registry);

    let fixed = f.ok(&["fix", "-f"]);
    assert!(fixed.stderr.is_empty());
    assert_eq!(
        String::from_utf8(fixed.stdout).unwrap(),
        format!(
            "✓ ~/link — replaced\n  → {:?}\n\n1 changed, 0 unchanged\n",
            f.path("expected")
        )
    );
    assert_eq!(f.target("home/link"), f.path("expected"));
    assert_eq!(f.registry(), registry);
}

#[test]
fn target_mismatch_paths_keep_quotes_backslashes_and_controls_unambiguous() {
    let f = Fixture::new();
    let expected = "expected\\n\"target";
    let actual = "actual\n\t\u{1b}\u{7f}target";
    symlink(actual, f.path("home/link\nname")).unwrap();
    f.write_entries(&[("home/link\nname", expected)]);
    let registry = f.registry();

    let output = f.run(&["fix"]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    let expected_path = serde_json::to_string(f.path(expected).to_str().unwrap()).unwrap();
    assert_eq!(
        error,
        format!(
            concat!(
                "! ~/link\\nname — failed: target differs\n",
                "  expected: {}\n",
                "  actual:   \"actual\\n\\t\\u001b\\u007ftarget\"\n",
                "  hint:     use --force (-f) to replace this symlink\n\n"
            ),
            expected_path
        )
    );
    assert_eq!(f.target("home/link\nname").to_str().unwrap(), actual);
    assert_eq!(f.registry(), registry);
}

#[test]
fn parent_creation_and_replacement_report_execution_and_preview() {
    for source_exists in [true, false] {
        let f = Fixture::new();
        if source_exists {
            f.write("source", "data");
        }
        let output = f.ok(&["-np", "source", "~/sub/link"]);
        let text = String::from_utf8(output.stdout).unwrap();
        let marker = if source_exists { "○" } else { "!" };
        assert!(text.starts_with(&format!("{marker} ~/sub/link — would create")));
        assert!(text.contains("  parent: ~/sub (would create)\n"));
        assert!(!f.path("home/sub").exists());
        assert!(!f.path("config").exists());

        let output = f.ok(&["-p", "source", "~/sub/link"]);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("  parent: ~/sub (created)\n"));
        assert!(!text.contains("(would create)"));
        assert!(f.path("home/sub").is_dir());
        f.write("other", "different");
        let registry = f.registry();
        let output = f.ok(&["-fn", "other", "~/sub/link"]);
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("○ ~/sub/link — would replace\n"));
        assert_eq!(f.registry(), registry);
        assert_eq!(f.target("home/sub/link"), f.path("source"));
        let output = f.ok(&["-f", "other", "~/sub/link"]);
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("✓ ~/sub/link — replaced\n"));
        assert_eq!(f.target("home/sub/link"), f.path("other"));
    }
}

#[test]
fn failed_items_do_not_look_successful_and_batch_counts_reflect_results() {
    let f = Fixture::new();
    f.write("source", "data");
    f.ok(&["source", "~/good"]);
    f.ok(&["source", "~/conflict"]);
    fs::remove_file(f.path("home/good")).unwrap();
    fs::remove_file(f.path("home/conflict")).unwrap();
    f.write("home/conflict", "keep");

    let output = f.run(&["fix", "-f"]);
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("✓ ~/good — created\n"));
    assert!(!text.contains("~/conflict"));
    assert!(text.ends_with("1 changed, 0 unchanged, 1 failed\n"));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.starts_with("! ~/conflict — failed\n  reason: expected a symlink:"));
    assert!(!error.contains('\t'));
    assert_eq!(fs::read_to_string(f.path("home/conflict")).unwrap(), "keep");
}

#[test]
fn completed_results_are_printed_only_after_the_transaction_returns() {
    for stage in ["prepared", "linked", "committed", "cleaned"] {
        let f = Fixture::new();
        f.write("source", "data");
        let output = f
            .command(&["source", "~/link"])
            .env("SLINK_TEST_CRASH", stage)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(99));
        assert!(output.stdout.is_empty(), "{stage}");
        let output = f.ok(&["source", "~/link"]);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.starts_with("✓ ~/link — recovered creation\n"),
            "{stage}: {text}"
        );
        assert_eq!(f.target("home/link"), f.path("source"));
    }
}

#[test]
fn target_warnings_and_control_characters_use_the_shared_display_rules() {
    let f = Fixture::new();
    let output = f.ok(&["missing\t\u{1b}target", "~/link\nname"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("! ~/link\\nname — created; target is missing\n"));
    assert!(!text.contains('\t'));
    assert!(!text.contains('\u{1b}'));
    assert_eq!(text.lines().count(), 2);
    assert!(output.stderr.is_empty());

    f.write("home/bad\n\u{1b}name", "keep");
    let output = f.run(&["missing", "~/bad\n\u{1b}name"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert_eq!(error.lines().count(), 3);
    assert!(!error.contains('\u{1b}'));
}

#[test]
fn recovery_failures_use_the_same_error_block_and_keep_the_pending_operation() {
    let f = Fixture::new();
    f.write("source", "data");
    f.crash(&["source", "~/link"], "linked");
    fs::remove_file(f.path("home/link")).unwrap();
    f.write("home/link", "keep");
    let output = f.run(&["source", "~/link"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .starts_with("! ~/link — failed\n  reason: recovery stopped"));
    assert_eq!(fs::read_to_string(f.path("home/link")).unwrap(), "keep");
    assert!(f.path("config/slink/links.toml.slink-pending").exists());
}

#[test]
fn adopted_relative_targets_are_reported_as_the_registered_absolute_reference() {
    let f = Fixture::new();
    f.write("home/source", "data");
    symlink("source", f.path("home/link")).unwrap();
    let output = f.ok(&["adopt", "~/link"]);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("✓ ~/link — registered\n  → {:?}\n", f.path("home/source"))
    );
    assert_eq!(f.target("home/link").to_str().unwrap(), "source");
}
