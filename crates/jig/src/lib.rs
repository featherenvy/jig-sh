mod agent_guides;
mod bootstrap;
mod cli;
mod context;
#[cfg(feature = "dev-proxy")]
mod dev_proxy;
#[cfg(not(feature = "dev-proxy"))]
mod dev_proxy {
    // Keep the CLI surface parseable in `--no-default-features` binaries while
    // returning a direct runtime error for commands that require proxy support.
    pub(crate) mod commands {
        use anyhow::{Result, bail};
        use serde_json::Value;

        use crate::cli::{DevOpts, ProxyCommand};
        use crate::context::RepoContext;

        pub(crate) fn dev(_ctx: &RepoContext, _opts: DevOpts) -> Result<Value> {
            bail!(
                "`jig dev` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn dev_without_context(_opts: DevOpts) -> Result<Value> {
            bail!(
                "`jig dev` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn proxy(_ctx: &RepoContext, _command: ProxyCommand) -> Result<Value> {
            bail!(
                "`jig proxy` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }

        pub(crate) fn proxy_without_context(_command: ProxyCommand) -> Result<Value> {
            bail!(
                "`jig proxy` is unavailable because this binary was built without the `dev-proxy` feature"
            )
        }
    }
}
mod git_receipts;
mod mcp;
mod policy;
mod process;
mod progress;
mod runtime;
mod state;
#[cfg(test)]
mod test_env;
mod tool_defs;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}

pub fn error_is_structured_command_failure(error: &anyhow::Error) -> bool {
    cli::is_structured_json_failure(error)
}

#[cfg(all(test, not(feature = "dev-proxy")))]
mod no_dev_proxy_feature_tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn write_minimal_repo(root: &std::path::Path) {
        fs::create_dir_all(root.join(".agent")).unwrap();
        fs::write(
            root.join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
"#,
        )
        .unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.2.0-beta.1",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn runtime_dispatch_reports_proxy_disabled_without_dev_proxy_feature() {
        let temp = tempdir().unwrap();
        write_minimal_repo(temp.path());
        let ctx = context::RepoContext::load_from(temp.path()).unwrap();

        let error = runtime::dispatch(
            &ctx,
            cli::CommandKind::Proxy(cli::ProxyCommand::List(cli::ProxyListOpts::default())),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("without the `dev-proxy` feature"));
    }

    #[test]
    fn dev_without_context_reports_proxy_disabled_without_repo_lookup() {
        let error = dev_proxy::commands::dev_without_context(cli::DevOpts {
            apps: Vec::new(),
            discover_workspace: false,
            no_proxy: false,
            proxy: cli::ProxyRuntimeOpts::default(),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("without the `dev-proxy` feature"));
    }
}
