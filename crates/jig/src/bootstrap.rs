use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_yaml::{Mapping, Value as YamlValue};

use copier::{CopierMode, build_update_spec, run_copier};
#[cfg(test)]
use copier::{CopySpecOptions, build_copy_spec};
use git::init_git_repo;
#[cfg(test)]
use git::{git, git_stdout};
#[cfg(test)]
use initial_copy::seed_answers_yaml;
use initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
#[cfg(test)]
use sync::rendered_conflicts;
#[cfg(test)]
use sync::{create_symlink, seed_preview_workspace};
#[cfg(test)]
use template_source::PrivateAnswerOverrides;
use template_source::{
    PreparedTemplateSource, StoredTemplateState, final_update_template_state,
    prepare_default_update_template_source, prepare_template_source, prepare_update_answers_file,
    read_stored_template_state, refresh_postwrite_template_metadata,
    resolve_update_template_source, template_identities_match, write_final_template_answers,
};

mod copier;
mod git;
mod initial_copy;
mod sync;
mod template_source;

const ANSWERS_FILE: &str = ".jig.yml";
const UVX_BIN_ENV: &str = "JIG_UVX_BIN";
const GIT_BIN_ENV: &str = "JIG_GIT_BIN";
// Keep in sync with the current template tasks in copier.yml and the normalization/generation
// scripts they invoke. The end-to-end adopt test below exercises the real template tasks.
const ALWAYS_TASK_MUTATED_PATHS: &[&str] = &[".jig.yml", "agent-map.md"];
const SQLX_PRUNED_TASK_PATHS: &[&str] = &[
    "scripts/add-migration.sh",
    "scripts/check-migration-immutability.sh",
    "scripts/check-schema-dump.sh",
    "scripts/check-sqlx-unchecked-non-test.sh",
    "scripts/generate-sqlx-unchecked-queries-todo.sh",
];
const TEMPLATE_MODE_KEY: &str = "_template_mode";
const TEMPLATE_LOCAL_PATH_KEY: &str = "_template_local_path";

#[derive(Args, Clone, Debug, Default)]
pub struct AnswerOpts {
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
    fn as_str(self) -> &'static str {
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
        defaults: opts.defaults,
        no_input: opts.no_input,
        seed_repo_path: None,
    })?;
    let default_branch = copy_result
        .default_branch
        .ok_or_else(|| anyhow::anyhow!("Missing default_branch in staged {}", ANSWERS_FILE))?;
    let git_initialized = init_git_repo(&destination, &default_branch)?;

    Ok(json!({
        "ok": true,
        "command": "init",
        "copier_mode": "copy",
        "template": template.copier_template,
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
        defaults: opts.defaults,
        no_input: opts.no_input,
        seed_repo_path: Some(&destination),
    })?;

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "copier_mode": "copy",
        "template": template.copier_template,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
    }))
}

pub fn run_update(opts: UpdateOpts) -> Result<Value> {
    let destination = absolute_path(&opts.path)?;
    validate_update_destination(&destination)?;
    let mode = if opts.recopy {
        CopierMode::Recopy
    } else {
        CopierMode::Update
    };
    let answers_path = destination.join(ANSWERS_FILE);
    let stored = read_stored_template_state(&answers_path)?;
    let update_template = prepare_update_template_source(&opts, &stored)?;
    let answers_postwrite = update_template
        .as_ref()
        .map(|template| final_update_template_state(&stored, template));
    let update_answers =
        prepare_update_answers_file(&destination, &answers_path, update_template.as_ref())?;
    let update_result = (|| -> Result<()> {
        run_copier(
            build_update_spec(
                mode,
                &destination,
                &update_answers.copier_arg,
                update_template
                    .as_ref()
                    .and_then(|prepared| prepared.vcs_ref.as_deref())
                    .or(opts.vcs_ref.as_deref()),
                opts.defaults || opts.no_input,
                update_answers.exclude_destination_answers,
            ),
            Some(&destination),
            !(opts.defaults || opts.no_input),
        )?;
        if let Some(mut prepared) = answers_postwrite {
            refresh_postwrite_template_metadata(&update_answers.path, &mut prepared)?;
            write_final_template_answers(&update_answers.path, &answers_path, &prepared)?;
        }
        Ok(())
    })();
    if update_result.is_err()
        && let Some(answers) = update_answers.restore_on_error.as_ref()
    {
        let _ = write_answers_yaml(&answers_path, answers);
    }
    update_result?;

    Ok(json!({
        "ok": true,
        "command": "update",
        "copier_mode": mode.as_str(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
    }))
}

fn prepare_update_template_source(
    opts: &UpdateOpts,
    stored: &StoredTemplateState,
) -> Result<Option<PreparedTemplateSource>> {
    let source_override_requested = opts.template.is_some() || opts.template_mode.is_some();
    if !source_override_requested && opts.vcs_ref.is_none() {
        return prepare_default_update_template_source(stored);
    }

    let resolved_source = resolve_update_template_source(opts, stored)?;
    let prepared = prepare_template_source(
        resolved_source.template,
        resolved_source.template_mode,
        opts.vcs_ref.as_deref(),
    )?;
    ensure_update_template_identity(stored, &prepared)?;
    Ok(Some(prepared))
}

fn ensure_update_template_identity(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> Result<()> {
    if stored.src_path.is_empty() || template_identities_match(stored, prepared) {
        return Ok(());
    }

    bail!(
        "jig update cannot switch template source paths in-place. Re-run with the existing source path, or re-adopt the repo from the new template source."
    )
}

fn read_optional_answer_bool(answers_path: &Path, key: &str) -> Result<Option<bool>> {
    let answers = read_answers_yaml(answers_path)?;
    Ok(answers.get(key).and_then(YamlValue::as_bool))
}

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

fn write_answers_yaml(path: &Path, mapping: &Mapping) -> Result<()> {
    let yaml = serde_yaml::to_string(&YamlValue::Mapping(mapping.clone()))
        .with_context(|| format!("Failed to serialize {}", path.display()))?;
    fs::write(path, yaml).with_context(|| format!("Failed to write {}", path.display()))
}

fn set_optional_yaml_string(mapping: &mut Mapping, key: &str, value: Option<&str>) {
    let key = YamlValue::String(key.to_string());
    match value {
        Some(value) => {
            mapping.insert(key, YamlValue::String(value.to_string()));
        }
        None => {
            mapping.remove(&key);
        }
    }
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

    let mut entries = fs::read_dir(path)?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("Failed to enumerate {}", path.display()))?;
    if entries.is_empty() || force {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.path());
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
