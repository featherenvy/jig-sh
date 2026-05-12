use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_yaml::{Mapping, Value as YamlValue};

use answers::RenderAnswers;
#[cfg(test)]
use file_copy::create_symlink;
use git::init_git_repo;
#[cfg(test)]
use git::{git, git_stdout};
#[cfg(test)]
use initial_copy::seed_answers_yaml;
use initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
#[cfg(test)]
use preview_seed::seed_preview_workspace;
use renderer::stage_render;
#[cfg(test)]
use sync::rendered_conflicts;
use sync::{ApplyRenderOptions, apply_staged_render};
#[cfg(test)]
use template_source::PrivateAnswerOverrides;
use template_source::{
    prepare_template_source, prepare_update_template_source, read_stored_template_state,
};

mod answers;
mod file_copy;
mod git;
mod initial_copy;
mod managed_paths;
mod preview_seed;
mod renderer;
mod staged_render;
mod sync;
mod template_source;

const ANSWERS_FILE: &str = ".jig.yml";
const GIT_BIN_ENV: &str = "JIG_GIT_BIN";
// Legacy conflict helpers keep these in sync with template task side effects.
#[cfg(test)]
const ALWAYS_TASK_MUTATED_PATHS: &[&str] = &[".jig.yml", "agent-map.md"];
const TEMPLATE_MODE_KEY: &str = "_template_mode";
const TEMPLATE_LOCAL_PATH_KEY: &str = "_template_local_path";

#[derive(Args, Clone, Debug, Default)]
pub struct AnswerOpts {
    #[arg(long)]
    pub answers_file: Option<PathBuf>,
    #[arg(long)]
    pub repo_name: Option<String>,
    #[arg(long)]
    pub default_branch: Option<String>,
    #[arg(long)]
    pub ci_github_runner: Option<String>,
    #[arg(long)]
    pub jig_version: Option<String>,
    #[arg(long)]
    pub template_source_url: Option<String>,
    #[arg(long)]
    pub sqlx_enabled: Option<bool>,
    #[arg(long = "rust-crate-root")]
    pub rust_crate_roots: Vec<String>,
    #[arg(long)]
    pub rust_migration_dir: Option<String>,
    #[arg(long)]
    pub rust_sqlx_metadata_dir: Option<String>,
    #[arg(long)]
    pub schema_dump_enabled: Option<bool>,
    #[arg(long)]
    pub schema_dump_command: Option<String>,
    #[arg(long)]
    pub migration_add_command: Option<String>,
    #[arg(long)]
    pub bootstrap_command: Option<String>,
    #[arg(long)]
    pub dev_command: Option<String>,
    #[arg(long)]
    pub rust_fmt_check_command: Option<String>,
    #[arg(long)]
    pub rust_clippy_command: Option<String>,
    #[arg(long)]
    pub rust_test_command: Option<String>,
    #[arg(long)]
    pub rust_test_locked_command: Option<String>,
    #[arg(long)]
    pub web_package_manager: Option<String>,
    #[arg(long = "frontend-app", value_parser = parse_frontend_app)]
    pub frontend_apps: Vec<FrontendApp>,
}

#[derive(Args, Clone, Debug)]
pub struct InitOpts {
    pub path: PathBuf,
    #[arg(long)]
    pub template: String,
    #[arg(long, value_enum)]
    pub template_mode: Option<TemplateMode>,
    #[arg(long)]
    pub vcs_ref: Option<String>,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub defaults: bool,
    #[arg(long)]
    pub no_input: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
pub struct AdoptOpts {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub template: String,
    #[arg(long, value_enum)]
    pub template_mode: Option<TemplateMode>,
    #[arg(long)]
    pub vcs_ref: Option<String>,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub defaults: bool,
    #[arg(long)]
    pub no_input: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
pub struct UpdateOpts {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub template: Option<String>,
    #[arg(long, value_enum)]
    pub template_mode: Option<TemplateMode>,
    #[arg(long)]
    pub recopy: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub vcs_ref: Option<String>,
    #[arg(long)]
    pub defaults: bool,
    #[arg(long)]
    pub no_input: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateMode {
    Committed,
}

impl TemplateMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
        }
    }
}

pub fn run_init(opts: InitOpts) -> Result<Value> {
    let destination = absolute_path(&opts.path)?;
    validate_init_destination(&destination, opts.force)?;
    let template =
        prepare_template_source(&opts.template, opts.template_mode, opts.vcs_ref.as_deref())?;

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &opts.answers,
        force: opts.force,
        seed_repo_path: None,
    })?;
    let default_branch = copy_result
        .default_branch
        .ok_or_else(|| anyhow::anyhow!("Missing default_branch in staged {}", ANSWERS_FILE))?;
    let git_initialized = init_git_repo(&destination, &default_branch)?;

    Ok(json!({
        "ok": true,
        "command": "init",
        "render_mode": "copy",
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": git_initialized,
    }))
}

pub fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    let destination = absolute_path(&opts.path)?;
    validate_adopt_destination(&destination)?;
    let template =
        prepare_template_source(&opts.template, opts.template_mode, opts.vcs_ref.as_deref())?;

    render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &opts.answers,
        force: opts.force,
        seed_repo_path: Some(&destination),
    })?;

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "render_mode": "copy",
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
    }))
}

pub fn run_update(opts: UpdateOpts) -> Result<Value> {
    let destination = absolute_path(&opts.path)?;
    validate_update_destination(&destination)?;
    let mode = if opts.recopy { "recopy" } else { "update" };
    let answers_path = destination.join(ANSWERS_FILE);
    let stored = read_stored_template_state(&answers_path)?;
    let update_template = prepare_update_template_source(&opts, &stored)?;
    let Some(update_template) = update_template else {
        bail!(
            "Missing template source metadata in {ANSWERS_FILE}. Re-adopt the repo before running jig update."
        );
    };
    let answers = RenderAnswers::from_answers_file(&answers_path)?;
    let staged = stage_render(&update_template, &answers, Some(&destination))?;
    apply_staged_render(
        &staged,
        &destination,
        ApplyRenderOptions {
            force: opts.force,
            allow_answers_overwrite: true,
            conflict_message: "Update would overwrite or remove template-managed paths. Re-run with --force to accept the rendered output:",
        },
    )?;

    Ok(json!({
        "ok": true,
        "command": "update",
        "render_mode": mode,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
    }))
}

#[cfg(test)]
fn read_optional_answer_bool(answers_path: &Path, key: &str) -> Result<Option<bool>> {
    let answers = read_answers_yaml(answers_path)?;
    Ok(answers.get(key).and_then(YamlValue::as_bool))
}

#[cfg(test)]
fn read_optional_answer_string(answers_path: &Path, key: &str) -> Result<Option<String>> {
    let answers = read_answers_yaml(answers_path)?;
    Ok(answers
        .get(key)
        .and_then(YamlValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty()))
}

fn read_answers_yaml(path: &Path) -> Result<Mapping> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let yaml: YamlValue = serde_yaml::from_str(&text)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    yaml.as_mapping()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Expected mapping in {}", path.display()))
}

#[cfg(test)]
fn write_answers_yaml(path: &Path, mapping: &Mapping) -> Result<()> {
    let yaml = serde_yaml::to_string(&YamlValue::Mapping(mapping.clone()))
        .with_context(|| format!("Failed to serialize {}", path.display()))?;
    fs::write(path, yaml).with_context(|| format!("Failed to write {}", path.display()))
}

fn parse_frontend_app(value: &str) -> Result<FrontendApp, String> {
    let parts = value.splitn(3, ':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("expected <name>:<dir>:<coverage_threshold>".into());
    }

    let coverage_threshold = parts[2]
        .parse::<u32>()
        .map_err(|_| "coverage_threshold must be a non-negative integer".to_string())?;

    Ok(FrontendApp {
        name: parts[0].to_string(),
        dir: parts[1].to_string(),
        coverage_threshold,
    })
}

fn validate_init_destination(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !path.is_dir() {
        bail!("Init destination is not a directory: {}", path.display());
    }
    if !path.exists() {
        return Ok(());
    }

    let first_entry = fs::read_dir(path)?
        .next()
        .transpose()
        .with_context(|| format!("Failed to enumerate {}", path.display()))?;
    if first_entry.is_none() || force {
        return Ok(());
    }

    bail!(
        "Init destination is not empty: {}. Re-run with --force to overwrite.",
        path.display()
    );
}

fn validate_adopt_destination(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Adopt destination does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Adopt destination is not a directory: {}", path.display());
    }
    Ok(())
}

fn validate_update_destination(path: &Path) -> Result<()> {
    validate_adopt_destination(path)?;
    let answers_path = path.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Update destination does not contain {}: {}",
            ANSWERS_FILE,
            path.display()
        );
    }
    Ok(())
}

fn external_program(env_key: &str, fallback: &str) -> String {
    env::var(env_key).unwrap_or_else(|_| fallback.to_string())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    if resolved.exists() {
        fs::canonicalize(&resolved)
            .with_context(|| format!("Failed to canonicalize {}", resolved.display()))
    } else {
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests;
