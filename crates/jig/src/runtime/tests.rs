use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use crate::test_env::{EnvVarGuard, lock_env};

use super::*;

mod agent;
mod common;
mod mcp;
mod work;

use common::*;

#[cfg(feature = "dev-proxy")]
#[test]
fn dispatch_routes_proxy_list_through_dev_proxy_feature() {
    use crate::cli::{CommandKind, ProxyCommand, ProxyListOpts, ProxyRuntimeOpts};

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let state_dir = temp.path().join("missing-proxy-state");

    let output = dispatch(
        &ctx,
        CommandKind::Proxy(ProxyCommand::List(ProxyListOpts {
            raw: false,
            proxy: ProxyRuntimeOpts {
                state_dir: Some(state_dir.clone()),
                ..ProxyRuntimeOpts::default()
            },
        })),
    )
    .unwrap();

    assert_eq!(output["ok"].as_bool(), Some(true));
    assert_eq!(
        output["state_dir"].as_str(),
        Some(state_dir.to_str().unwrap())
    );
    assert!(output["routes"].as_array().unwrap().is_empty());
    assert!(!state_dir.exists());
}

#[test]
fn tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_test_command = "printf 'command tool ran\n'"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["rust_test_command"],
            "tools": [
                {
                    "name": "jig.test",
                    "kind": "command",
                    "description": "Run configured test command.",
                    "command": "rust_test_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: true,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn make_tool_no_receipt_skips_receipt_append() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("Makefile"),
        "custom-check:\n\t@printf 'make target ran\\n'\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_make_targets": ["custom-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.run_target",
                    "kind": "make",
                    "description": "Run an arbitrary declared make target."
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::RunTarget(crate::cli::RunTargetOpts {
            name: "custom-check".into(),
            tool: crate::cli::ToolOpts {
                plan_id: None,
                no_receipt: true,
            },
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert_eq!(output["result"]["stdout"], "make target ran\n");
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn native_tool_no_receipt_skips_receipt_append() {
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
jig_version = "0.2.0-beta.1"
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
            "jig_version": "0.2.0-beta.1",
            "required_make_targets": ["fmt-check", "clippy", "test", "contract-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.fmt_check",
                    "kind": "make",
                    "description": "Run fmt.",
                    "target": "fmt-check"
                },
                {
                    "name": "jig.clippy",
                    "kind": "make",
                    "description": "Run clippy.",
                    "target": "clippy"
                },
                {
                    "name": "jig.test",
                    "kind": "make",
                    "description": "Run tests.",
                    "target": "test"
                },
                {
                    "name": "jig.contract_check",
                    "kind": "native",
                    "description": "Run native contract check."
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Contract(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: true,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["receipt_id"], serde_json::Value::Null);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
    assert!(!temp.path().join(".agent/state/receipts.jsonl").exists());
}

#[test]
fn failed_tool_error_remains_primary_when_receipt_append_fails() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_test_command = "printf 'tool failed stdout\n'; printf 'tool failed stderr\n' >&2; exit 7"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["rust_test_command"],
            "tools": [
                {
                    "name": "jig.test",
                    "kind": "command",
                    "description": "Run configured test command.",
                    "command": "rust_test_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join(".agent/state"), "not a directory").unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Test(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.test failed with status 7"), "{error}");
    assert!(error.contains("command key: rust_test_command"), "{error}");
    assert!(error.contains("tool failed stdout"), "{error}");
    assert!(error.contains("tool failed stderr"), "{error}");
    assert!(error.contains("receipt recording also failed"), "{error}");
}
