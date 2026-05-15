use super::*;
use crate::test_env::{EnvVarGuard, lock_env};
use serde_json::json;
use std::sync::Mutex;
use tempfile::tempdir;

static CWD_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn runtime_commands_still_require_adopted_repo_context() {
    let temp = tempdir().unwrap();
    let error = find_repo_root_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("Could not find repo root containing .jig.toml"));
}

#[test]
fn load_optional_returns_none_outside_adopted_repo() {
    let _guard = CWD_LOCK.lock().unwrap();
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();
    let result = RepoContext::load_optional();
    std::env::set_current_dir(original).unwrap();
    assert!(result.unwrap().is_none());
}

#[test]
fn load_optional_ignores_stale_jig_repo_root() {
    let _guard = CWD_LOCK.lock().unwrap();
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing");
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing);
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();
    let result = RepoContext::load_optional();
    std::env::set_current_dir(original).unwrap();

    assert!(result.unwrap().is_none());
}

#[test]
fn supported_command_keys_are_backed_by_repo_config() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
bootstrap_command = "cargo fetch"
contract_check_command = "scripts/jig contract-check"
migration_add_command = "scripts/jig migration-add \"$NAME\""
rust_clippy_command = "cargo clippy"
rust_fmt_check_command = "cargo fmt --check"
rust_test_command = "cargo test"
rust_test_locked_command = "cargo test --locked"
schema_check_command = "scripts/jig schema-check"
schema_dump_command = "scripts/dump-schema.sh"
sqlx_check_command = "cargo sqlx prepare --check"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_commands": SUPPORTED_COMMAND_KEYS,
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for key in SUPPORTED_COMMAND_KEYS {
        assert!(ctx.command_for_key(key).is_ok(), "{key}");
    }
}

#[test]
fn missing_legacy_contract_check_command_stays_empty() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
rust_fmt_check_command = "cargo fmt --check"
rust_clippy_command = "cargo clippy"
rust_test_command = "cargo test"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_commands": ["contract_check_command"],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = ctx.command_for_key("contract_check_command").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("contract_check_command is empty")
    );
}

#[test]
fn legacy_work_checks_become_required_check_gates() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[work]
checks = ["jig.contract_check"]
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.contract_check",
                    "kind": "make",
                    "description": "Run make contract-check.",
                    "target": "contract-check"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let gates = ctx.work_gates();
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].id, "contract-check");
    assert_eq!(gates[0].kind, "check");
    assert_eq!(gates[0].tool.as_deref(), Some("jig.contract_check"));
    assert!(gates[0].required);
}

#[test]
fn missing_agent_tooling_uses_jig_skills_defaults() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let marketplaces = ctx.codex_marketplaces();
    assert_eq!(marketplaces.len(), 1);
    assert_eq!(marketplaces[0].id, "jig-skills");
    assert_eq!(marketplaces[0].source, "bpcakes/jig-skills");
    assert_eq!(
        marketplaces[0].plugins,
        vec![
            "jig-rust@jig-skills",
            "jig-swift@jig-skills",
            "jig-typescript@jig-skills",
            "jig-exec-plans@jig-skills",
        ]
    );
}

#[test]
fn explicit_agent_tooling_config_is_loaded() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"
plugins = ["local-rust@local-skills"]
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let marketplaces = ctx.codex_marketplaces();
    assert_eq!(marketplaces.len(), 1);
    assert_eq!(marketplaces[0].id, "local-skills");
    assert_eq!(marketplaces[0].source, "../jig-skills");
    assert_eq!(marketplaces[0].plugins, vec!["local-rust@local-skills"]);
}

#[test]
fn dev_config_defaults_and_apps_are_loaded() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
dev_command = "cargo run"
web_package_manager = "pnpm"

[dev]
proxy_port = 1555
https = true
workspace_discovery = true

[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4545

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["pnpm", "run", "dev"]
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert_eq!(ctx.web_package_manager(), "pnpm");
    assert_eq!(ctx.dev_config().proxy_port, 1555);
    assert!(ctx.dev_config().https);
    assert!(ctx.dev_config().workspace_discovery);
    assert_eq!(ctx.dev_config().apps.len(), 2);
    assert_eq!(ctx.dev_config().apps[0].name, "api");
    assert_eq!(ctx.dev_config().apps[0].port, Some(4545));
    assert_eq!(ctx.dev_config().apps[1].argv, vec!["pnpm", "run", "dev"]);
}

#[test]
fn duplicate_dev_app_names_are_rejected_at_config_load() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
command = "bun run dev"

[[dev.apps]]
name = "web"
command = "bun run dev"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Duplicate dev app name"));
}

#[test]
fn unsupported_web_package_manager_is_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "/tmp/run-anything"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("Unsupported web_package_manager"));
}

#[test]
fn template_dev_defaults_match_runtime_defaults() {
    let template = include_str!("../../../../templates/project/.jig.toml.jinja");
    let defaults = DevConfig::default();

    assert!(template.contains(&format!("proxy_port = {}", defaults.proxy_port)));
    assert!(template.contains(&format!("https_port = {}", defaults.https_port.unwrap())));
    assert!(template.contains(&format!("https = {}", defaults.https)));
    assert!(template.contains(&format!("http2 = {}", defaults.http2)));
    assert!(template.contains(&format!("lan = {}", defaults.lan)));
    assert!(template.contains(&format!(r#"tld = "{}""#, defaults.tld)));
    assert!(template.contains(&format!(
        "workspace_discovery = {}",
        defaults.workspace_discovery
    )));
}

#[test]
fn unknown_dev_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
proxy_port = 1555
proxy_por = 1556
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_por"));
}

#[test]
fn unknown_dev_app_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
command = "bun run dev"
commmand = "typo"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("commmand"));
}

#[test]
fn unknown_top_level_config_fields_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
proxy_porrt = 1355
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field"));
    assert!(error.contains("proxy_porrt"));
}

#[test]
fn legacy_work_checks_are_merged_with_explicit_gates() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[work]
checks = ["jig.contract_check", "jig.test"]

[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"
required = false
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check", "test"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.contract_check",
                    "kind": "make",
                    "description": "Run make contract-check.",
                    "target": "contract-check"
                },
                {
                    "name": "jig.test",
                    "kind": "make",
                    "description": "Run make test.",
                    "target": "test"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let gates = ctx.work_gates();
    assert_eq!(gates.len(), 2);
    assert_eq!(gates[0].id, "contract");
    assert_eq!(gates[0].tool.as_deref(), Some("jig.contract_check"));
    assert!(!gates[0].required);
    assert_eq!(gates[1].id, "test");
    assert_eq!(gates[1].tool.as_deref(), Some("jig.test"));
    assert!(gates[1].required);
}

#[test]
fn unsupported_work_refinements_are_rejected() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".agent")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[work.refinements]]
id = "rust-simplify"
skill = "jig-rust:rust-simplify"
"#,
    )
    .unwrap();
    fs::write(
        temp.path().join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
    assert!(error.contains("work.refinements is not supported yet"));
    assert!(error.contains("rust-simplify"));
}
