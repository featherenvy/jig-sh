use serde_json::json;
use tempfile::tempdir;

use super::*;

#[test]
fn supported_contract_versions_are_one_through_three() {
    for version in 1..=3 {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
"#,
        )
        .unwrap();

        let manifest = if version == 1 {
            json!({
                "contract_version": version,
                "tool_namespace": "jig",
                "jig_version": "0.2.0-beta.1",
                "required_make_targets": ["bootstrap"],
                "tools": [],
            })
        } else {
            json!({
                "contract_version": version,
                "tool_namespace": "jig",
                "jig_version": "0.2.0-beta.1",
                "required_commands": ["bootstrap_command"],
                "tools": [],
            })
        };
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        assert_eq!(ctx.contract_version(), version);
    }

    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 4,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from_root(temp.path().to_path_buf())
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported jig contract version: 4"));
}
