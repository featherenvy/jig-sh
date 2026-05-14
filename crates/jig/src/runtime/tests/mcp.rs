use super::*;

#[test]
fn mcp_call_dispatches_make_tool_declared_only_in_manifest() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["target"], "custom-check");
    assert_eq!(output["result"]["stdout"], "manifest target ran\n");
}

#[test]
fn mcp_exposes_read_only_agent_doctor_tool() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(codex_home.join("config.toml"), "").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = call_tool(&ctx, tool::AGENT_DOCTOR, json!({})).unwrap();

    assert_eq!(output["command"], "agent doctor");
    assert_eq!(output["codex"]["available"], true);
}

#[test]
fn mcp_does_not_expose_dev_or_proxy_commands() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    for name in [
        "jig.dev",
        "jig.proxy",
        "jig.proxy_start",
        "jig.proxy_cert_trust",
    ] {
        let error = call_tool(&ctx, name, json!({})).unwrap_err().to_string();
        assert!(error.contains("Unsupported tool"));
    }
}

#[test]
fn mcp_work_tools_deserialize_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_START,
        json!({
            "title": "Typed MCP request",
            "body": "Use serde for tool arguments"
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output["plan"]["plan_id"].as_str().is_some());
}

#[test]
fn mcp_work_tools_tolerate_null_optional_defaults() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let check = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": null
        }),
    )
    .unwrap();
    let receipts = call_tool(
        &ctx,
        tool::WORK_RECEIPTS,
        json!({
            "failed_only": null,
            "limit": null
        }),
    )
    .unwrap();

    assert_eq!(check["ok"], true);
    assert_eq!(receipts["ok"], true);
    assert!(!receipts["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_work_tools_reject_invalid_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, tool::WORK_START, json!({ "body": "missing title" })).unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("Invalid work tool arguments"));
    assert!(error.contains("missing field `title`"));
}
