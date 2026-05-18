use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
#[cfg(test)]
use toml::{Table, Value as TomlValue};

use super::AnswerOpts;
use super::answers::{AnswerResolution, RenderAnswers};
use super::renderer::{RenderStageRequest, stage_render};
use super::sync::ApplyRenderReport;
use super::sync::{ApplyRenderOptions, apply_staged_render};
use super::template_source::PreparedTemplateSource;
#[cfg(test)]
use super::template_source::PrivateAnswerOverrides;
#[cfg(test)]
use super::{TEMPLATE_LOCAL_PATH_KEY, TEMPLATE_MODE_KEY};
use crate::progress::CliProgress;

const ANSWERS_DETAIL: &str = ".jig.toml values and command defaults";
const REQUIRED_FRONTEND_SCRIPTS: &[&str] = &["lint", "typecheck", "build:bundle", "test:coverage"];

pub(super) struct BootstrapCopyRequest<'a> {
    pub(super) destination: &'a Path,
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a AnswerOpts,
    pub(super) use_defaults: bool,
    pub(super) force: bool,
    pub(super) seed_repo_path: Option<&'a Path>,
    pub(super) progress: CliProgress,
}

pub(super) struct BootstrapCopyResult {
    pub(super) default_branch: Option<String>,
    pub(super) bootstrap_command_configured: bool,
    pub(super) frontend_apps_configured: bool,
    pub(super) codex_skills_configured: bool,
    pub(super) sqlx_enabled: bool,
    pub(super) schema_dump_enabled: bool,
    pub(super) apply_report: ApplyRenderReport,
    pub(super) notes: Vec<String>,
}

pub(super) fn render_and_copy_bootstrap_template(
    request: BootstrapCopyRequest<'_>,
) -> Result<BootstrapCopyResult> {
    request.progress.step("resolve answers", ANSWERS_DETAIL);
    let answer_resolution = request
        .progress
        .log_blocked_on_err(AnswerResolution::from_opts(
            request.answers,
            request.destination,
            request.use_defaults,
        ))?;
    let (answers, mut notes) = answer_resolution.into_parts();
    if request.seed_repo_path.is_some() && !answers.frontend_apps().is_empty() {
        request
            .progress
            .step("validate web apps", "package.json scripts for CI checks");
        request
            .progress
            .log_blocked_on_err(validate_frontend_app_scripts(request.destination, &answers))?;
    }
    let staged = stage_render(RenderStageRequest {
        template: request.template,
        answers: &answers,
        seed_repo_path: request.seed_repo_path,
        progress: request.progress,
    })?;

    let apply_report = apply_staged_render(
        &staged,
        request.destination,
        ApplyRenderOptions {
            force: request.force,
            allow_answers_overwrite: false,
            conflict_message: "Adopt would overwrite template-managed paths. No files were changed. Re-run with --force or clear these paths first:",
            progress: request.progress,
        },
    )?;

    if answers.has_legacy_dev_command() {
        notes.push(
            "Preserved deprecated dev_command for migration; generated commands ignore it. Move that value into [dev] / [[dev.apps]] when ready."
                .into(),
        );
    }

    Ok(BootstrapCopyResult {
        default_branch: Some(answers.default_branch().to_string()),
        bootstrap_command_configured: answers.bootstrap_command_configured(),
        frontend_apps_configured: !answers.frontend_apps().is_empty(),
        codex_skills_configured: answers.codex_skills_configured(),
        sqlx_enabled: answers.sqlx_enabled(),
        schema_dump_enabled: answers.schema_dump_enabled(),
        apply_report,
        notes,
    })
}

fn validate_frontend_app_scripts(destination: &Path, answers: &RenderAnswers) -> Result<()> {
    for app in answers.frontend_apps() {
        let app_dir = destination.join(&app.dir);
        let package_path = app_dir.join("package.json");
        if !package_path.is_file() {
            bail!(
                "Configured frontend app '{}' in {} is missing package.json. Add the app package.json, or remove the entry from frontend_apps until web CI checks are ready.",
                app.name,
                app.dir
            );
        }
        let package = fs::read_to_string(&package_path)
            .with_context(|| format!("Failed to read {}", package_path.display()))?;
        let package: JsonValue = serde_json::from_str(&package)
            .with_context(|| format!("Failed to parse {}", package_path.display()))?;
        let scripts = package
            .get("scripts")
            .and_then(JsonValue::as_object)
            .cloned()
            .unwrap_or_default();
        let missing = REQUIRED_FRONTEND_SCRIPTS
            .iter()
            .copied()
            .filter(|script| {
                !matches!(
                    scripts.get(*script).and_then(JsonValue::as_str),
                    Some(command) if !command.trim().is_empty()
                )
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            bail!(
                "Configured frontend app '{}' in {} is missing package.json scripts required by generated web CI: {}. Add those scripts, or remove the entry from frontend_apps until the app is CI-ready.",
                app.name,
                app.dir,
                missing.join(", ")
            );
        }
        validate_frontend_app_lockfile(destination, &app_dir, answers.web_package_manager(), app)?;
    }
    Ok(())
}

fn validate_frontend_app_lockfile(
    destination: &Path,
    app_dir: &Path,
    package_manager: &str,
    app: &super::FrontendApp,
) -> Result<()> {
    let lockfiles = frontend_lockfile_names(package_manager);
    let has_repo_lockfile = destination.join("package.json").is_file()
        && lockfiles
            .iter()
            .any(|lockfile| destination.join(lockfile).is_file());
    let has_app_lockfile = lockfiles
        .iter()
        .any(|lockfile| app_dir.join(lockfile).is_file());
    if has_repo_lockfile || has_app_lockfile {
        return Ok(());
    }

    bail!(
        "Configured frontend app '{}' in {} does not have a lockfile for {} at the repo root or app directory. Add one, or remove the entry from frontend_apps until web CI is ready.",
        app.name,
        app.dir,
        package_manager
    )
}

fn frontend_lockfile_names(package_manager: &str) -> &'static [&'static str] {
    match package_manager {
        "bun" => &["bun.lock", "bun.lockb"],
        "npm" => &["package-lock.json"],
        "pnpm" => &["pnpm-lock.yaml"],
        "yarn" => &["yarn.lock"],
        _ => unreachable!("web package manager was already validated"),
    }
}

#[cfg(test)]
pub(super) fn seed_answers_toml(
    opts: &AnswerOpts,
    private_answers: &PrivateAnswerOverrides,
) -> TomlValue {
    let mut mapping = Table::new();
    insert_string(&mut mapping, "repo_name", opts.repo_name.as_deref());
    insert_string(
        &mut mapping,
        "default_branch",
        opts.default_branch.as_deref(),
    );
    insert_string(
        &mut mapping,
        "ci_github_runner",
        opts.ci_github_runner.as_deref(),
    );
    insert_string(&mut mapping, "jig_version", opts.jig_version.as_deref());
    insert_string(
        &mut mapping,
        "template_source_url",
        opts.template_source_url.as_deref(),
    );
    insert_bool(&mut mapping, "sqlx_enabled", opts.sqlx_enabled);
    insert_string(
        &mut mapping,
        "rust_migration_dir",
        opts.rust_migration_dir.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_sqlx_metadata_dir",
        opts.rust_sqlx_metadata_dir.as_deref(),
    );
    insert_bool(
        &mut mapping,
        "schema_dump_enabled",
        opts.schema_dump_enabled,
    );
    insert_string(
        &mut mapping,
        "schema_dump_command",
        opts.schema_dump_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "schema_check_command",
        opts.schema_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "sqlx_check_command",
        opts.sqlx_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "migration_add_command",
        opts.migration_add_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "bootstrap_command",
        opts.bootstrap_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "contract_check_command",
        opts.contract_check_command.as_deref(),
    );
    insert_string(&mut mapping, "dev_command", opts.dev_command.as_deref());
    insert_string(
        &mut mapping,
        "rust_fmt_check_command",
        opts.rust_fmt_check_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_clippy_command",
        opts.rust_clippy_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_test_command",
        opts.rust_test_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "rust_test_locked_command",
        opts.rust_test_locked_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "web_package_manager",
        opts.web_package_manager.as_deref(),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_MODE_KEY,
        private_answers.template_mode_answer(),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_LOCAL_PATH_KEY,
        private_answers.template_local_path_answer(),
    );

    if !opts.rust_crate_roots.is_empty() {
        mapping.insert(
            "rust_crate_roots".into(),
            TomlValue::Array(
                opts.rust_crate_roots
                    .iter()
                    .cloned()
                    .map(TomlValue::String)
                    .collect(),
            ),
        );
    }
    if !opts.frontend_apps.is_empty() {
        mapping.insert(
            "frontend_apps".into(),
            TomlValue::Array(
                opts.frontend_apps
                    .iter()
                    .map(|app| {
                        let mut app_table = Table::new();
                        app_table.insert("name".into(), TomlValue::String(app.name.clone()));
                        app_table.insert("dir".into(), TomlValue::String(app.dir.clone()));
                        app_table.insert(
                            "coverage_threshold".into(),
                            TomlValue::Integer(app.coverage_threshold.into()),
                        );
                        TomlValue::Table(app_table)
                    })
                    .collect(),
            ),
        );
    }

    TomlValue::Table(mapping)
}

#[cfg(test)]
fn insert_string(mapping: &mut Table, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(key.to_string(), TomlValue::String(value.to_string()));
    }
}

#[cfg(test)]
fn insert_bool(mapping: &mut Table, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        mapping.insert(key.to_string(), TomlValue::Boolean(value));
    }
}
