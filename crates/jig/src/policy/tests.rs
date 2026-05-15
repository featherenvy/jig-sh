use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::context::RepoContext;
use crate::tool_defs::{kind, tool};

fn write_policy_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_commands": ["rust_test_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_sqlx_policy_repo(root: &Path) {
    write_policy_repo(root);
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
sqlx_enabled = true
rust_migration_dir = "migrations"
rust_crate_roots = ["crates"]
rust_test_command = "cargo test"
"#,
    )
    .unwrap();
}

fn write_schema_policy_repo(root: &Path, schema_dump_command: &str) {
    write_policy_repo(root);
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
sqlx_enabled = true
schema_dump_enabled = true
rust_migration_dir = "migrations"
schema_dump_command = "{}"
rust_test_command = "cargo test"
"#,
            schema_dump_command
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        ),
    )
    .unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {}", args.join(" "));
}

fn init_git(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.com"]);
}

#[test]
fn contract_check_does_not_require_v2_only_tools_for_v1_contracts() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
makefile_enabled = true
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("Makefile"),
        "fmt-check:\nclippy:\ntest:\ncontract-check:\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["fmt-check", "clippy", "test", "contract-check"],
            "tools": [
                {
                    "name": tool::FMT_CHECK,
                    "kind": kind::MAKE,
                    "description": "Run fmt.",
                    "target": "fmt-check"
                },
                {
                    "name": tool::CLIPPY,
                    "kind": kind::MAKE,
                    "description": "Run clippy.",
                    "target": "clippy"
                },
                {
                    "name": tool::TEST,
                    "kind": kind::MAKE,
                    "description": "Run tests.",
                    "target": "test"
                },
                {
                    "name": tool::CONTRACT_CHECK,
                    "kind": kind::MAKE,
                    "description": "Run contract check.",
                    "target": "contract-check"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = contract_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 0, "{}", output.stderr);
}

#[test]
fn migration_immutability_parses_nul_name_status_entries() {
    let bytes = b"A\0migrations/002_added.up.sql\0M\0migrations/001_changed.up.sql\0R100\0migrations/001_old.up.sql\0migrations/001_new.up.sql\0D\0migrations/001_deleted.down.sql\0T\0migrations/001_type.sql\0";

    let violations = migration_immutability_violations(bytes);

    assert_eq!(violations.len(), 4);
    assert!(violations.iter().all(|violation| {
        !violation.contains("002_added") && violation.contains("Existing migration files")
    }));
    assert!(violations.iter().any(|violation| {
        violation.contains("Rename detected (R100)")
            && violation.contains("migrations/001_old.up.sql -> migrations/001_new.up.sql")
    }));
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Change detected (M)")
                && violation.contains("001_changed"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("Change detected (D)")
                && violation.contains("001_deleted"))
    );
    assert!(violations.iter().any(
        |violation| violation.contains("Change detected (T)") && violation.contains("001_type")
    ));
}

#[test]
fn migration_immutability_ignores_truncated_rename_entry() {
    let violations = migration_immutability_violations(b"R100\0migrations/old.sql\0");

    assert!(violations.is_empty());
}

#[test]
fn migration_add_creates_slugged_migration_files() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = migration_add(&ctx, "Create Users!").unwrap();

    assert_eq!(output.exit_status, 0);
    let entries = fs::read_dir(temp.path().join("migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .any(|entry| entry.ends_with("_create_users.up.sql"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.ends_with("_create_users.down.sql"))
    );
}

#[test]
fn migration_add_rejects_when_sqlx_is_disabled() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "create users").unwrap_err();

    assert!(error.to_string().contains("sqlx_enabled = true"));
}

#[test]
fn migration_add_rejects_names_without_slug_content() {
    let temp = tempdir().unwrap();
    write_sqlx_policy_repo(temp.path());
    init_git(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = migration_add(&ctx, "!!!").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must contain at least one alphanumeric")
    );
}

#[test]
fn schema_check_reports_stale_schema_dump() {
    let temp = tempdir().unwrap();
    write_schema_policy_repo(
        temp.path(),
        "mkdir -p docs/schema && printf 'changed\\n' > docs/schema/tables.sql",
    );
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = schema_check(&ctx).unwrap();

    assert_eq!(output.exit_status, 1);
    assert!(output.stderr.contains("Schema dump is stale"));
    assert!(output.stderr.contains("docs/schema"));
}

#[test]
fn check_rust_file_loc_reports_oversized_tracked_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn example() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &CheckRustFileLocOpts {
            staged: false,
            changed_against: None,
            all: true,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_staged_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(
        temp.path().join("crates/app/src/staged.rs"),
        "fn staged() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &CheckRustFileLocOpts {
            staged: true,
            changed_against: None,
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/staged.rs"))
    );
}

#[test]
fn check_rust_file_loc_reports_oversized_changed_against_files() {
    let temp = tempdir().unwrap();
    write_policy_repo(temp.path());
    fs::write(temp.path().join("crates/app/src/lib.rs"), "fn small() {}\n").unwrap();
    init_git(temp.path());
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "baseline", "-q"]);
    let base = super::git_text(temp.path(), &["rev-parse", "HEAD"]).unwrap();
    fs::write(
        temp.path().join("crates/app/src/large.rs"),
        "fn changed() {}\n".repeat(HARD_LIMIT + 1),
    )
    .unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "large", "-q"]);

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = check_rust_file_loc(
        &ctx,
        &CheckRustFileLocOpts {
            staged: false,
            changed_against: Some(base.trim().to_string()),
            all: false,
        },
    )
    .unwrap();

    assert_eq!(output["ok"], false);
    assert!(
        output["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("crates/app/src/large.rs"))
    );
}
