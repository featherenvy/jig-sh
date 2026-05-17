use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{AnswerOpts, FrontendApp};
use crate::context::{
    DEFAULT_CODEX_MARKETPLACE_ID, DEFAULT_CODEX_MARKETPLACE_SOURCE,
    default_codex_marketplace_plugins, validate_web_package_manager,
};

#[derive(Clone, Debug, Serialize)]
pub(super) struct RenderAnswers {
    repo_name: String,
    default_branch: String,
    ci_github_runner: String,
    jig_version: String,
    template_source_url: String,
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
    legacy_dev_command: Option<String>,
    rust_fmt_check_command: String,
    rust_clippy_command: String,
    rust_test_command: String,
    rust_test_locked_command: String,
    web_package_manager: String,
    web_install_command: String,
    web_run_command: String,
    typescript_lint_command: String,
    typescript_typecheck_command: String,
    typescript_build_command: String,
    typescript_coverage_command: String,
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
        let mut raw = RawAnswers::from_file(path)?;
        raw.normalize_legacy_sqlx_disabled_schema_dump();
        raw.normalize_legacy_generated_cargo_command_defaults();
        raw.resolve(None)
    }

    pub(super) fn default_branch(&self) -> &str {
        &self.default_branch
    }

    pub(super) fn template_source_url(&self) -> &str {
        &self.template_source_url
    }

    pub(super) fn rust_crate_roots(&self) -> &[String] {
        &self.rust_crate_roots
    }

    pub(super) fn frontend_apps(&self) -> &[FrontendApp] {
        &self.frontend_apps
    }

    pub(super) fn web_package_manager(&self) -> &str {
        &self.web_package_manager
    }

    pub(super) fn has_legacy_dev_command(&self) -> bool {
        self.legacy_dev_command.is_some()
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawAnswers {
    repo_name: Option<String>,
    default_branch: Option<String>,
    ci_github_runner: Option<String>,
    jig_version: Option<String>,
    template_source_url: Option<String>,
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

    fn normalize_legacy_sqlx_disabled_schema_dump(&mut self) {
        if self.sqlx_enabled == Some(false) && self.schema_dump_enabled == Some(true) {
            self.schema_dump_enabled = Some(false);
        }
    }

    fn normalize_legacy_generated_cargo_command_defaults(&mut self) {
        normalize_legacy_command_default(&mut self.bootstrap_command, "cargo fetch");
        normalize_legacy_command_default(
            &mut self.rust_fmt_check_command,
            "cargo fmt --all -- --check",
        );
        normalize_legacy_command_default(
            &mut self.rust_clippy_command,
            "cargo clippy --workspace --all-targets --locked -- -D warnings",
        );
        normalize_legacy_command_default(&mut self.rust_test_command, "cargo test --workspace");
        normalize_legacy_command_default(
            &mut self.rust_test_locked_command,
            "cargo test --workspace --locked",
        );
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
        if !sqlx_enabled && self.schema_dump_enabled == Some(true) {
            bail!(
                "schema_dump_enabled cannot be true when sqlx_enabled is false; enable SQLx or set schema_dump_enabled = false"
            );
        }

        let frontend_apps = self.frontend_apps.unwrap_or_default();
        validate_frontend_apps(&frontend_apps)?;
        let legacy_dev_command = self.dev_command.filter(|value| !value.trim().is_empty());

        let web_package_manager = self.web_package_manager.unwrap_or_else(|| "bun".into());
        validate_web_package_manager(&web_package_manager)?;
        let web_install_command = web_install_command(&web_package_manager).to_string();
        let web_run_command = web_run_command(&web_package_manager).to_string();
        let schema_dump_enabled = if sqlx_enabled {
            self.schema_dump_enabled.unwrap_or(true)
        } else {
            false
        };
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
        let migration_add_command = self.migration_add_command;

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
            sqlx_enabled,
            rust_crate_roots: self
                .rust_crate_roots
                .unwrap_or_else(|| vec!["crates".into()]),
            rust_migration_dir,
            rust_sqlx_metadata_dir,
            schema_dump_enabled,
            schema_dump_command,
            schema_check_command: self.schema_check_command.unwrap_or_default(),
            sqlx_check_command,
            migration_add_command,
            bootstrap_command: self
                .bootstrap_command
                .unwrap_or_else(|| optional_cargo_command("cargo fetch", "bootstrap")),
            contract_check_command: self.contract_check_command.unwrap_or_default(),
            legacy_dev_command,
            rust_fmt_check_command: self
                .rust_fmt_check_command
                .unwrap_or_else(|| optional_cargo_command("cargo fmt --all -- --check", "fmt")),
            rust_clippy_command: self.rust_clippy_command.unwrap_or_else(|| {
                optional_cargo_command(
                    "cargo clippy --workspace --all-targets --locked -- -D warnings",
                    "clippy",
                )
            }),
            rust_test_command: self
                .rust_test_command
                .unwrap_or_else(|| optional_cargo_command("cargo test --workspace", "test")),
            rust_test_locked_command: self.rust_test_locked_command.unwrap_or_else(|| {
                optional_cargo_command("cargo test --workspace --locked", "test-locked")
            }),
            web_package_manager,
            web_install_command,
            web_run_command,
            typescript_lint_command: "scripts/check-webapps.sh lint".into(),
            typescript_typecheck_command: "scripts/check-webapps.sh typecheck".into(),
            typescript_build_command: "scripts/check-webapps.sh build".into(),
            typescript_coverage_command: "scripts/check-webapps.sh coverage".into(),
            frontend_apps,
            agent_tooling: self.agent_tooling.unwrap_or_default(),
        })
    }
}

fn normalize_legacy_command_default(command: &mut Option<String>, legacy_default: &str) {
    if command.as_deref() == Some(legacy_default) {
        *command = None;
    }
}

fn optional_cargo_command(command: &str, label: &str) -> String {
    let skip_prefix = crate::CARGO_SKIP_OUTPUT_PREFIX;
    let skip_message = shell_quote(&format!("{skip_prefix}{label}."));
    // Runtime command dispatch sets CWD to the repo root, so this guard checks
    // for a root Cargo workspace without blocking harness-only repos.
    format!("if [ -f Cargo.toml ]; then {command}; else printf '%s\\n' {skip_message}; fi")
}

fn validate_frontend_apps(apps: &[FrontendApp]) -> Result<()> {
    let mut names = HashSet::new();
    for app in apps {
        if !is_safe_frontend_app_name(&app.name) {
            bail!(
                "Invalid frontend app name '{}'. Use ASCII letters, numbers, '-' or '_'.",
                app.name
            );
        }
        if !names.insert(app.name.as_str()) {
            bail!("Duplicate frontend app name '{}'", app.name);
        }
        if !is_supported_frontend_app_kind(&app.kind) {
            bail!(
                "Invalid frontend app kind '{}'. Expected 'vite' or 'env-port'.",
                app.kind
            );
        }
        validate_frontend_app_dir(&app.name, &app.dir)?;
    }
    Ok(())
}

fn is_safe_frontend_app_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn is_supported_frontend_app_kind(value: &str) -> bool {
    matches!(value, "vite" | "env-port")
}

fn validate_frontend_app_dir(app_name: &str, value: &str) -> Result<()> {
    if value.is_empty() || value.trim() != value {
        bail!("frontend app '{app_name}' dir must be a non-empty relative path");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!(
            "frontend app '{app_name}' dir '{}' contains unsupported characters. Use a repo-relative path with ASCII letters, numbers, '/', '.', '-' or '_'; use forward slashes on every platform.",
            value
        );
    }

    let path = Path::new(value);
    if path.is_absolute() {
        bail!("frontend app '{app_name}' dir '{}' must be relative", value);
    }
    if value.split('/').any(|segment| segment.is_empty()) {
        bail!(
            "frontend app '{app_name}' dir '{}' must not contain empty path components",
            value
        );
    }
    if value.split('/').any(|segment| segment == ".") {
        bail!(
            "frontend app '{app_name}' dir '{}' must not contain '.' path components",
            value
        );
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                bail!(
                    "frontend app '{app_name}' dir '{}' must not contain '.' path components",
                    value
                );
            }
            Component::ParentDir => {
                bail!(
                    "frontend app '{app_name}' dir '{}' must not contain '..'",
                    value
                );
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("frontend app '{app_name}' dir '{}' must be relative", value);
            }
        }
    }
    Ok(())
}

fn web_install_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun install --frozen-lockfile",
        "pnpm" => "pnpm install --frozen-lockfile",
        "npm" => "npm ci",
        "yarn" => "yarn install --frozen-lockfile",
        _ => unreachable!("web package manager was already validated"),
    }
}

fn web_run_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun run",
        "pnpm" => "pnpm run",
        "npm" => "npm run",
        "yarn" => "yarn run",
        _ => unreachable!("web package manager was already validated"),
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
