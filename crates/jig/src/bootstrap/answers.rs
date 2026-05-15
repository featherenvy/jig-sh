use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{AnswerOpts, FrontendApp};
use crate::context::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_SOURCE,
    default_codex_marketplace_plugins,
};

#[derive(Clone, Debug, Serialize)]
pub(super) struct RenderAnswers {
    repo_name: String,
    default_branch: String,
    ci_github_runner: String,
    jig_version: String,
    template_source_url: String,
    makefile_enabled: bool,
    sqlx_enabled: bool,
    rust_crate_roots: Vec<String>,
    rust_migration_dir: Option<String>,
    rust_sqlx_metadata_dir: Option<String>,
    schema_dump_enabled: bool,
    schema_dump_command: String,
    schema_check_command: String,
    sqlx_check_command: String,
    migration_add_command: Option<String>,
    bootstrap_command: String,
    contract_check_command: String,
    dev_command: String,
    rust_fmt_check_command: String,
    rust_clippy_command: String,
    rust_test_command: String,
    rust_test_locked_command: String,
    web_package_manager: String,
    frontend_apps: Vec<FrontendApp>,
    agent_tooling: AgentToolingAnswers,
}

impl RenderAnswers {
    pub(super) fn from_opts(opts: &AnswerOpts, destination: &Path) -> Result<Self> {
        let mut raw = if let Some(path) = opts.answers_file.as_deref() {
            RawAnswers::from_file(path)?
        } else {
            RawAnswers::default()
        };
        raw.merge_opts(opts);
        raw.resolve(default_repo_name(destination))
    }

    pub(super) fn from_answers_file(path: &Path) -> Result<Self> {
        RawAnswers::from_file(path)?.resolve(None)
    }

    pub(super) fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub(super) fn template_source_url(&self) -> &str {
        &self.template_source_url
    }

    pub(super) fn makefile_enabled(&self) -> bool {
        self.makefile_enabled
    }

    pub(super) fn sqlx_enabled(&self) -> bool {
        self.sqlx_enabled
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawAnswers {
    repo_name: Option<String>,
    default_branch: Option<String>,
    ci_github_runner: Option<String>,
    jig_version: Option<String>,
    template_source_url: Option<String>,
    makefile_enabled: Option<bool>,
    sqlx_enabled: Option<bool>,
    rust_crate_roots: Option<Vec<String>>,
    rust_migration_dir: Option<String>,
    rust_sqlx_metadata_dir: Option<String>,
    schema_dump_enabled: Option<bool>,
    schema_dump_command: Option<String>,
    schema_check_command: Option<String>,
    sqlx_check_command: Option<String>,
    migration_add_command: Option<String>,
    bootstrap_command: Option<String>,
    contract_check_command: Option<String>,
    dev_command: Option<String>,
    rust_fmt_check_command: Option<String>,
    rust_clippy_command: Option<String>,
    rust_test_command: Option<String>,
    rust_test_locked_command: Option<String>,
    web_package_manager: Option<String>,
    frontend_apps: Option<Vec<FrontendApp>>,
    agent_tooling: Option<AgentToolingAnswers>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct AgentToolingAnswers {
    #[serde(default)]
    codex: CodexToolingAnswers,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexToolingAnswers {
    #[serde(default = "default_codex_marketplaces")]
    marketplaces: Vec<CodexMarketplaceAnswers>,
}

impl Default for CodexToolingAnswers {
    fn default() -> Self {
        Self {
            marketplaces: default_codex_marketplaces(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexMarketplaceAnswers {
    id: String,
    source: String,
    #[serde(default)]
    plugins: Vec<String>,
}

impl RawAnswers {
    fn from_file(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
    }

    fn merge_opts(&mut self, opts: &AnswerOpts) {
        merge_option(&mut self.repo_name, opts.repo_name.clone());
        merge_option(&mut self.default_branch, opts.default_branch.clone());
        merge_option(&mut self.ci_github_runner, opts.ci_github_runner.clone());
        merge_option(&mut self.jig_version, opts.jig_version.clone());
        merge_option(
            &mut self.template_source_url,
            opts.template_source_url.clone(),
        );
        merge_option(&mut self.makefile_enabled, opts.makefile_enabled);
        merge_option(&mut self.sqlx_enabled, opts.sqlx_enabled);
        if !opts.rust_crate_roots.is_empty() {
            self.rust_crate_roots = Some(opts.rust_crate_roots.clone());
        }
        merge_option(
            &mut self.rust_migration_dir,
            opts.rust_migration_dir.clone(),
        );
        merge_option(
            &mut self.rust_sqlx_metadata_dir,
            opts.rust_sqlx_metadata_dir.clone(),
        );
        merge_option(&mut self.schema_dump_enabled, opts.schema_dump_enabled);
        merge_option(
            &mut self.schema_dump_command,
            opts.schema_dump_command.clone(),
        );
        merge_option(
            &mut self.schema_check_command,
            opts.schema_check_command.clone(),
        );
        merge_option(
            &mut self.sqlx_check_command,
            opts.sqlx_check_command.clone(),
        );
        merge_option(
            &mut self.migration_add_command,
            opts.migration_add_command.clone(),
        );
        merge_option(&mut self.bootstrap_command, opts.bootstrap_command.clone());
        merge_option(
            &mut self.contract_check_command,
            opts.contract_check_command.clone(),
        );
        merge_option(&mut self.dev_command, opts.dev_command.clone());
        merge_option(
            &mut self.rust_fmt_check_command,
            opts.rust_fmt_check_command.clone(),
        );
        merge_option(
            &mut self.rust_clippy_command,
            opts.rust_clippy_command.clone(),
        );
        merge_option(&mut self.rust_test_command, opts.rust_test_command.clone());
        merge_option(
            &mut self.rust_test_locked_command,
            opts.rust_test_locked_command.clone(),
        );
        merge_option(
            &mut self.web_package_manager,
            opts.web_package_manager.clone(),
        );
        if !opts.frontend_apps.is_empty() {
            self.frontend_apps = Some(opts.frontend_apps.clone());
        }
    }

    fn resolve(self, default_repo_name: Option<String>) -> Result<RenderAnswers> {
        let repo_name = self
            .repo_name
            .filter(|value| !value.is_empty())
            .or(default_repo_name)
            .ok_or_else(|| anyhow::anyhow!("Missing required answer: repo_name"))?;
        let sqlx_enabled = self.sqlx_enabled.unwrap_or(true);
        let rust_migration_dir = self.rust_migration_dir.filter(|value| !value.is_empty());
        if sqlx_enabled && rust_migration_dir.is_none() {
            bail!("Missing required answer when sqlx_enabled is true: rust_migration_dir");
        }

        let web_package_manager = self.web_package_manager.unwrap_or_else(|| "bun".into());
        if web_package_manager != "bun" {
            bail!("Unsupported web_package_manager '{web_package_manager}'. Supported values: bun");
        }
        let schema_dump_enabled = self.schema_dump_enabled.unwrap_or(true);
        let schema_dump_command = self
            .schema_dump_command
            .unwrap_or_else(|| "scripts/dump-schema.sh".into());
        let rust_sqlx_metadata_dir = self.rust_sqlx_metadata_dir.or_else(|| Some(".sqlx".into()));
        let sqlx_check_command = self.sqlx_check_command.unwrap_or_else(|| {
            let metadata_dir = rust_sqlx_metadata_dir.as_deref().unwrap_or(".sqlx");
            format!(
                "SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} cargo sqlx prepare --check --workspace -- --workspace --all-targets",
                shell_quote(metadata_dir)
            )
        });
        let migration_add_command = self.migration_add_command.or_else(|| {
            rust_migration_dir.as_deref().map(|dir| {
                format!(
                    "RUST_MIGRATION_DIR={} scripts/add-migration.sh",
                    shell_quote(dir)
                )
            })
        });

        Ok(RenderAnswers {
            repo_name,
            default_branch: self.default_branch.unwrap_or_else(|| "main".into()),
            ci_github_runner: self
                .ci_github_runner
                .unwrap_or_else(|| "ubuntu-latest".into()),
            jig_version: self
                .jig_version
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into()),
            template_source_url: self.template_source_url.unwrap_or_default(),
            makefile_enabled: self.makefile_enabled.unwrap_or(true),
            sqlx_enabled,
            rust_crate_roots: self
                .rust_crate_roots
                .unwrap_or_else(|| vec!["crates".into()]),
            rust_migration_dir,
            rust_sqlx_metadata_dir,
            schema_dump_enabled,
            schema_dump_command,
            schema_check_command: self
                .schema_check_command
                .unwrap_or_else(|| "scripts/check-schema-dump.sh".into()),
            sqlx_check_command,
            migration_add_command,
            bootstrap_command: self
                .bootstrap_command
                .unwrap_or_else(|| "cargo fetch".into()),
            contract_check_command: self
                .contract_check_command
                .unwrap_or_else(|| "scripts/check-jig-contract.sh".into()),
            dev_command: self.dev_command.unwrap_or_else(|| {
                r#"echo "Define dev_command in .jig.toml" >&2 && exit 1"#.into()
            }),
            rust_fmt_check_command: self
                .rust_fmt_check_command
                .unwrap_or_else(|| "cargo fmt --all -- --check".into()),
            rust_clippy_command: self.rust_clippy_command.unwrap_or_else(|| {
                "cargo clippy --workspace --all-targets --locked -- -D warnings".into()
            }),
            rust_test_command: self
                .rust_test_command
                .unwrap_or_else(|| "cargo test --workspace".into()),
            rust_test_locked_command: self
                .rust_test_locked_command
                .unwrap_or_else(|| "cargo test --workspace --locked".into()),
            web_package_manager,
            frontend_apps: self.frontend_apps.unwrap_or_default(),
            agent_tooling: self.agent_tooling.unwrap_or_default(),
        })
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn default_codex_marketplaces() -> Vec<CodexMarketplaceAnswers> {
    vec![CodexMarketplaceAnswers {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

fn merge_option<T>(target: &mut Option<T>, value: Option<T>) {
    if let Some(value) = value {
        *target = Some(value);
    }
}

fn default_repo_name(destination: &Path) -> Option<String> {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}
