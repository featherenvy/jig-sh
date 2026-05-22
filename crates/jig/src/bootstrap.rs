use std::borrow::Cow;
#[cfg(test)]
use std::cell::Cell;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use toml::Table;
#[cfg(test)]
use toml::Value as TomlValue;
use ulid::Ulid;

use crate::progress::CliProgress;
use answers::{AnswerInput, RenderAnswers};
#[cfg(test)]
use file_copy::create_symlink;
use git::init_git_repo;
#[cfg(test)]
use git::{git, git_stdout};
#[cfg(test)]
use initial_copy::seed_answers_toml;
use initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
use path::{absolute_path_from, bootstrap_invocation_cwd};
#[cfg(test)]
use preview_seed::seed_preview_workspace;
use renderer::{RenderStageRequest, stage_render};
#[cfg(test)]
use sync::rendered_conflicts;
use sync::{ApplyRenderOptions, apply_staged_render};
use template_source::EMBEDDED_TEMPLATE_SOURCE;
#[cfg(test)]
use template_source::PrivateAnswerOverrides;
use template_source::{prepare_update_template_source, read_stored_template_state};

mod adopt_infer;
mod answers;
mod crate_guide;
mod embedded_templates;
mod file_copy;
mod gate_preview;
mod git;
mod initial_copy;
mod managed_paths;
mod path;
mod preview_seed;
mod renderer;
mod staged_render;
mod sync;
mod template_source;

const ANSWERS_FILE: &str = ".jig.toml";
const GIT_BIN_ENV: &str = "JIG_GIT_BIN";
const BUILD_TEMPLATE_PIN_RELEASED: &str = "released";
const BUILD_TEMPLATE_PIN_UNRELEASED: &str = "unreleased";
const OFFICIAL_TEMPLATE_SOURCE: &str = "https://github.com/bpcakes/jig-sh.git";
const REMOTE_TEMPLATE_MODE_ERROR: &str = "--template-mode only applies to local git template paths. Omit --template-mode for remote templates, or pass --template /path/to/jig-sh --template-mode committed.";
// Legacy conflict helpers keep these in sync with template task side effects.
#[cfg(test)]
const ALWAYS_TASK_MUTATED_PATHS: &[&str] = &[".jig.toml", "agent-map.md"];
const TEMPLATE_MODE_KEY: &str = "_template_mode";
const TEMPLATE_LOCAL_PATH_KEY: &str = "_template_local_path";

#[derive(Args, Clone, Debug, Default)]
pub struct AnswerOpts {
    #[arg(long, help = "Read renderer answers from a TOML file")]
    pub answers_file: Option<PathBuf>,
    #[arg(long, help = "Repository display name written into generated docs")]
    pub repo_name: Option<String>,
    #[arg(
        long,
        help = "Default branch used for generated CI and comparison commands"
    )]
    pub default_branch: Option<String>,
    #[arg(long, help = "GitHub Actions runs-on value for generated workflows")]
    pub ci_github_runner: Option<String>,
    #[arg(long, help = "Exact Jig runtime version to pin in generated repos")]
    pub jig_version: Option<String>,
    #[arg(
        long,
        help = "Portable canonical template source URL for future updates"
    )]
    pub template_source_url: Option<String>,
    #[arg(long, help = "Generate SQLx and migration contract tools")]
    pub sqlx_enabled: Option<bool>,
    #[arg(
        long = "rust-crate-root",
        help = "Directory whose direct children are Rust crates; may be repeated"
    )]
    pub rust_crate_roots: Vec<String>,
    #[arg(long, help = "SQL migration directory for SQLx-enabled repos")]
    pub rust_migration_dir: Option<String>,
    #[arg(long, help = "Committed SQLx metadata directory")]
    pub rust_sqlx_metadata_dir: Option<String>,
    #[arg(long, help = "Generate schema dump and freshness commands")]
    pub schema_dump_enabled: Option<bool>,
    #[arg(long, help = "Command used by scripts/jig schema-dump")]
    pub schema_dump_command: Option<String>,
    #[arg(long, help = "Command used by legacy schema-check manifests")]
    pub schema_check_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig check sqlx")]
    pub sqlx_check_command: Option<String>,
    #[arg(long, help = "Command used by legacy migration-add manifests")]
    pub migration_add_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig bootstrap")]
    pub bootstrap_command: Option<String>,
    #[arg(long, help = "Command used by legacy contract-check manifests")]
    pub contract_check_command: Option<String>,
    #[arg(long, help = "Deprecated; configure [dev] and [[dev.apps]] instead")]
    pub dev_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig check fmt")]
    pub rust_fmt_check_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig check clippy")]
    pub rust_clippy_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig check test")]
    pub rust_test_command: Option<String>,
    #[arg(long, help = "Command used by scripts/jig check test-locked")]
    pub rust_test_locked_command: Option<String>,
    #[arg(long, help = "Web package manager for generated web app checks")]
    pub web_package_manager: Option<String>,
    #[arg(
        long = "frontend-app",
        value_parser = parse_frontend_app,
        help = "Frontend CI app as name:dir:coverage_threshold[:kind]; kind defaults to vite; package.json must expose lint, typecheck, build:bundle, and test:coverage; may be repeated"
    )]
    pub frontend_apps: Vec<FrontendApp>,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Templates:
  Release builds default to the official jig-sh harness template:
  https://github.com/bpcakes/jig-sh.git

  Release builds pin omitted --template to this jig version's release tag.
  Unreleased or dirty local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Examples:
  jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
  jig init /path/to/new-repo --template /path/to/jig-sh --template-mode committed --repo-name new-repo --sqlx-enabled false")]
pub struct InitOpts {
    #[arg(help = "Destination directory to create or populate")]
    pub path: PathBuf,
    #[arg(
        long,
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Allow init to write into a non-empty destination")]
    pub force: bool,
    #[arg(long, help = "Use default answers for omitted configuration prompts")]
    pub defaults: bool,
    #[arg(long, help = "Fail instead of prompting for missing answers")]
    pub no_input: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Templates:
  Release builds default to the official jig-sh harness template:
  https://github.com/bpcakes/jig-sh.git

  Release builds pin omitted --template to this jig version's release tag.
  Unreleased or dirty local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Adoption scans the existing repository before resolving answers. If SQLx is detected,
omitted SQLx answers resolve to migration defaults; if it is not detected, omitted SQLx
answers resolve to a tooling-only profile. Pass --sqlx-enabled true and --rust-migration-dir
<dir> to override.

Examples:
  jig adopt .
  jig adopt . --write
  jig adopt . --write --template /path/to/jig-sh --template-mode committed")]
pub struct AdoptOpts {
    #[arg(default_value = ".", help = "Existing repository directory to adopt")]
    pub path: PathBuf,
    #[arg(
        long,
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Overwrite conflicting template-managed paths")]
    pub force: bool,
    #[arg(long, help = "Write rendered managed files; omit to preview only")]
    pub write: bool,
    #[arg(
        long,
        help = "Use default answers for omitted configuration prompts and adopt write confirmation"
    )]
    pub defaults: bool,
    #[arg(
        long,
        help = "Fail instead of prompting for missing answers and skip adopt write confirmation"
    )]
    pub no_input: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Update modes:
  jig update advances to the resolved template source.
  jig update --recopy re-renders from the stored .jig.toml commit.
  Add --force only when changed template-managed files should be replaced.

Examples:
  jig update
  jig update --recopy
  jig update --template /path/to/jig-sh --template-mode committed --force")]
pub struct UpdateOpts {
    #[arg(default_value = ".", help = "Adopted repository directory to update")]
    pub path: PathBuf,
    #[arg(long, help = "Template source to render from for this update")]
    pub template: Option<String>,
    #[arg(long, value_enum, help = "How to read a local git template checkout")]
    pub template_mode: Option<TemplateMode>,
    #[arg(
        long,
        help = "Re-render from the stored .jig.toml commit instead of advancing"
    )]
    pub recopy: bool,
    #[arg(long, help = "Overwrite changed template-managed files")]
    pub force: bool,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Use default answers for omitted configuration prompts")]
    pub defaults: bool,
    #[arg(long, help = "Fail instead of prompting for missing answers")]
    pub no_input: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
    #[serde(default = "default_frontend_app_kind")]
    pub kind: String,
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
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("init");
    progress.header_for_path("render harness into new repo", &destination);
    progress.step("validate destination", "empty directory or --force");
    progress.log_blocked_on_err(validate_init_destination(&destination, opts.force))?;
    progress.step(
        "resolve template",
        template_progress_label(opts.template.as_deref()),
    );
    let template_request = progress.log_blocked_on_err(resolve_initial_template_request(
        opts.template.as_deref(),
        &opts.vcs_ref,
    ))?;
    let template = progress.log_blocked_on_err(prepare_initial_template_source(
        &template_request,
        opts.template_mode,
        &invocation_cwd,
    ))?;

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &opts.answers,
        answer_input: None,
        use_defaults: opts.defaults,
        force: opts.force,
        dry_run: false,
        backup_root: None,
        seed_repo_path: None,
        progress,
    })?;
    let default_branch = copy_result
        .default_branch
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing default_branch in staged {}", ANSWERS_FILE))?;
    progress.step("initialize git", format!("default branch {default_branch}"));
    let git_initialized =
        progress.log_blocked_on_err(init_git_repo(&destination, default_branch))?;
    progress.done("init complete");

    Ok(json!({
        "ok": true,
        "command": "init",
        "render_mode": "copy",
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": git_initialized,
        "adoption_report": adoption_report(&copy_result),
        "next_steps": initial_next_steps(InitialCommand::Init, &destination, &copy_result),
        "notes": initial_notes(copy_result.notes),
    }))
}

pub fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("adopt");
    progress.header_for_path("render harness into existing repo", &destination);
    progress.step("validate destination", "existing repository directory");
    progress.log_blocked_on_err(validate_adopt_destination(&destination))?;
    progress.step(
        "resolve template",
        template_progress_label(opts.template.as_deref()),
    );
    let template_request = progress.log_blocked_on_err(resolve_initial_template_request(
        opts.template.as_deref(),
        &opts.vcs_ref,
    ))?;
    let template = progress.log_blocked_on_err(prepare_initial_template_source(
        &template_request,
        opts.template_mode,
        &invocation_cwd,
    ))?;
    progress.step("infer answers", "scan existing repository");
    let inference = adopt_infer::infer_adopt_answers(&destination);
    let mut answers = opts.answers.clone();
    let answer_input = progress.log_blocked_on_err(AnswerInput::from_opts(&answers))?;
    let answer_shape = answer_input.shape().clone();
    progress.info("detected", inference.summary());
    progress.info("detected stack", inference.detected_stack_label());
    for warning in inference.warnings() {
        progress.info("warning", warning);
    }
    inference.apply_to_answers(&mut answers, &answer_shape);
    let review = inference.adoption_review(&answers, &opts.answers, &answer_shape);
    for item in &review.items {
        progress.info("review", item);
    }
    if opts.write {
        confirm_adopt_write(&opts)?;
    } else {
        progress.info(
            "mode",
            "preview only; re-run with --write to apply managed files",
        );
    }
    let backup_root = opts.write.then(|| adopt_backup_root(&destination));

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &answers,
        answer_input: Some(answer_input),
        use_defaults: opts.defaults,
        force: opts.force,
        dry_run: !opts.write,
        backup_root: backup_root.clone(),
        seed_repo_path: Some(&destination),
        progress,
    })?;
    if opts.write {
        if let Err(error) =
            write_adopt_last_receipt(&destination, backup_root.as_deref(), &copy_result)
        {
            progress.info(
                "warning",
                format!("adopt write completed but undo receipt could not be recorded: {error:#}"),
            );
        }
        progress.done("adopt complete");
    } else {
        progress.done("adopt preview complete");
    }

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "render_mode": if opts.write { "copy" } else { "preview" },
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "write": opts.write,
        "detection_report": inference.report(),
        "adoption_profile": inference.adoption_profile_report(
            &copy_result.render_preview.generated_gates,
            &copy_result.render_preview.managed_files,
            &opts.answers,
            &answer_shape,
        ),
        "adoption_review": review.items,
        "adoption_report": adoption_report(&copy_result),
        "next_steps": initial_next_steps(InitialCommand::Adopt, &destination, &copy_result),
        "notes": initial_notes(copy_result.notes),
    }))
}

#[derive(Debug)]
struct InitialTemplateRequest<'a> {
    template: &'a str,
    vcs_ref: Option<Cow<'a, str>>,
    used_default: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildTemplatePinPolicy {
    Released,
    Unreleased,
    Unknown,
}

#[cfg(test)]
thread_local! {
    static TEST_BUILD_TEMPLATE_PIN_POLICY: Cell<Option<BuildTemplatePinPolicy>> = const { Cell::new(None) };
}

fn resolve_initial_template_request<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
) -> Result<InitialTemplateRequest<'a>> {
    resolve_initial_template_request_with_policy(
        template,
        vcs_ref,
        current_build_template_pin_policy(),
    )
}

fn resolve_initial_template_request_with_policy<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'a>> {
    match template {
        Some(template) if is_official_template_source(template) => {
            official_initial_template_request(vcs_ref, pin_policy)
        }
        Some(template) => Ok(InitialTemplateRequest {
            template,
            vcs_ref: vcs_ref.as_deref().map(Cow::Borrowed),
            used_default: false,
        }),
        None => default_initial_template_request(vcs_ref, pin_policy),
    }
}

fn default_initial_template_request<'a>(
    vcs_ref: &'a Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'a>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        // Omitted template on local builds is offline-friendly; explicitly naming
        // the official URL still means "use remote official template code".
        return Ok(InitialTemplateRequest {
            template: EMBEDDED_TEMPLATE_SOURCE,
            vcs_ref: None,
            used_default: true,
        });
    }

    official_initial_template_request(vcs_ref, pin_policy)
}

fn official_initial_template_request<'a>(
    vcs_ref: &'a Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'a>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        bail!(
            "This jig binary was built from unreleased or dirty local source version {}.\nThe default official template pin {} may not match this binary.\nTo render from your checkout, pass --template /path/to/jig-sh --template-mode committed.\nTo use official remote template code, pass --vcs-ref <ref>.",
            env!("CARGO_PKG_VERSION"),
            official_template_ref(),
        );
    }

    Ok(InitialTemplateRequest {
        template: OFFICIAL_TEMPLATE_SOURCE,
        // The release workflow tags the whole workspace as vVERSION. Keep the
        // default template pinned to the installed jig binary's workspace version.
        vcs_ref: Some(
            vcs_ref
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Owned(official_template_ref())),
        ),
        used_default: true,
    })
}

fn current_build_template_pin_policy() -> BuildTemplatePinPolicy {
    #[cfg(test)]
    {
        TEST_BUILD_TEMPLATE_PIN_POLICY
            .with(Cell::get)
            .unwrap_or(BuildTemplatePinPolicy::Released)
    }

    #[cfg(not(test))]
    {
        build_template_pin_policy_from_env(option_env!("JIG_BUILD_OFFICIAL_TEMPLATE_PIN"))
    }
}

fn build_template_pin_policy_from_env(value: Option<&str>) -> BuildTemplatePinPolicy {
    match value {
        Some(BUILD_TEMPLATE_PIN_RELEASED) => BuildTemplatePinPolicy::Released,
        Some(BUILD_TEMPLATE_PIN_UNRELEASED) => BuildTemplatePinPolicy::Unreleased,
        // Published crates do not carry .git metadata, so build.rs emits
        // unknown. Missing or unrecognized values keep the same release-pin
        // behavior rather than failing crates.io and packaged installs.
        _ => BuildTemplatePinPolicy::Unknown,
    }
}

fn is_official_template_source(template: &str) -> bool {
    canonical_template_source(template) == canonical_template_source(OFFICIAL_TEMPLATE_SOURCE)
}

fn canonical_template_source(template: &str) -> &str {
    template.strip_suffix(".git").unwrap_or(template)
}

fn official_template_ref() -> String {
    // The published binary and the template tag share the workspace version.
    official_template_ref_for_version(env!("CARGO_PKG_VERSION"))
}

fn official_template_ref_for_version(version: &str) -> String {
    format!("v{version}")
}

fn prepare_initial_template_source(
    request: &InitialTemplateRequest<'_>,
    template_mode: Option<TemplateMode>,
    path_base: &Path,
) -> Result<template_source::PreparedTemplateSource> {
    if request.used_default && template_mode.is_some() {
        // Keep local-only mode errors direct; wrapping them as default-source
        // resolution failures would incorrectly suggest a network or tag issue.
        bail!(REMOTE_TEMPLATE_MODE_ERROR);
    }

    let result = template_source::prepare_template_source_from_base(
        request.template,
        template_mode,
        request.vcs_ref.as_deref(),
        path_base,
    );
    if request.used_default {
        result.with_context(|| default_template_failure_context(request))
    } else {
        result
    }
}

fn default_template_failure_context(request: &InitialTemplateRequest<'_>) -> String {
    let Some(vcs_ref) = request.vcs_ref.as_deref() else {
        return format!(
            "Failed to resolve the official Jig template {}. For offline use, pass --template <local-path>. To use a specific official ref such as main, pass --vcs-ref <ref>.",
            request.template
        );
    };
    let ref_requirement = if vcs_ref == official_template_ref() {
        "network access and a matching release tag. If this Jig binary was built from a prerelease or development version, that tag may not exist yet"
    } else {
        "network access and the selected ref must exist"
    };
    format!(
        "Failed to resolve the official Jig template {} at {}. The official template requires {}. For offline use, pass --template <local-path>. To use a different official ref such as main, pass --vcs-ref <ref>.",
        request.template, vcs_ref, ref_requirement
    )
}

pub fn run_update(opts: UpdateOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("update");
    let mode = if opts.recopy { "recopy" } else { "update" };
    progress.header_for_path(format!("refresh harness ({mode})"), &destination);
    progress.step("validate destination", "adopted repository directory");
    progress.log_blocked_on_err(validate_update_destination(&destination))?;
    let answers_path = destination.join(ANSWERS_FILE);
    progress.step("read answers", answers_path.display());
    let stored = progress.log_blocked_on_err(read_stored_template_state(&answers_path))?;
    progress.step("resolve template", "stored source metadata");
    let update_template = progress.log_blocked_on_err(prepare_update_template_source(
        &opts,
        &stored,
        &invocation_cwd,
    ))?;
    let Some(update_template) = update_template else {
        progress.blocked("stored template source metadata is missing");
        bail!(
            "Missing template source metadata in {ANSWERS_FILE}. Re-adopt the repo before running jig update."
        );
    };
    let answers = progress.log_blocked_on_err(RenderAnswers::from_answers_file(&answers_path))?;
    let staged = stage_render(RenderStageRequest {
        template: &update_template,
        answers: &answers,
        seed_repo_path: Some(&destination),
        progress,
    })?;
    let render_report = apply_staged_render(
        &staged,
        &destination,
        ApplyRenderOptions {
            force: opts.force,
            dry_run: false,
            allow_answers_overwrite: true,
            backup_root: None,
            conflict_message: "Update would overwrite or remove template-managed paths. No files were changed. Re-run with --force to accept the rendered output:",
            progress,
        },
    )?;
    progress.done("update complete");

    Ok(json!({
        "ok": true,
        "command": "update",
        "render_mode": mode,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "render_report": render_report,
    }))
}

fn template_progress_label(template: Option<&str>) -> String {
    template.unwrap_or("default jig-sh template").to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitialCommand {
    Init,
    Adopt,
}

fn initial_next_steps(
    command: InitialCommand,
    destination: &Path,
    result: &initial_copy::BootstrapCopyResult,
) -> Vec<String> {
    let destination_for_cd = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());
    let mut steps = vec![
        format!(
            "cd {}",
            crate::shell::quote(&destination_for_cd.display().to_string())
        ),
        "Review .jig.toml, AGENTS.md, agent-map.md, and generated checks.".into(),
    ];
    if command == InitialCommand::Adopt && result.apply_report.dry_run {
        steps.push("Re-run jig adopt --write after reviewing the preview.".into());
        steps.push("No files were changed by this preview.".into());
        return steps;
    }
    if result.bootstrap_command_configured {
        steps.push("scripts/jig bootstrap".into());
    }
    steps.push("scripts/jig doctor --summary".into());
    steps.push("scripts/jig check contract".into());
    if result.frontend_apps_configured {
        steps.push("scripts/jig check typescript-lint".into());
        steps.push("scripts/jig check typescript-typecheck".into());
        steps.push("scripts/jig check typescript-build".into());
        steps.push("scripts/jig check typescript-coverage".into());
        steps.push("scripts/jig dev".into());
    }
    if result.codex_skills_configured {
        steps.push("scripts/jig agent bootstrap".into());
    }
    if result.sqlx_enabled {
        steps.push(
            "Install cargo-sqlx and configure database access, then run scripts/jig check sqlx."
                .into(),
        );
    }
    if result.schema_dump_enabled {
        steps.push(
            "Implement scripts/dump-schema.sh for this repo, then run scripts/jig schema-dump."
                .into(),
        );
    }
    steps.push("scripts/jig check agent-guides".into());
    steps.push("scripts/jig check test".into());
    if command == InitialCommand::Adopt {
        if result.apply_report.dry_run {
            steps.push("No files were changed by this preview.".into());
        } else {
            steps.push("Commit the adoption diff after the generated checks pass.".into());
        }
    }
    steps
}

fn initial_notes(extra_notes: Vec<String>) -> Vec<String> {
    let mut notes = vec![
        "The first scripts/jig command may install or compile the pinned Jig runtime into this repo's local cache.".into(),
        "Adoption validates configured frontend app package scripts and lockfiles immediately.".into(),
        "Init records configured frontend apps without reading package.json; add lint, typecheck, build:bundle, test:coverage, and a package-manager lockfile before web CI runs.".into(),
    ];
    notes.extend(extra_notes);
    notes
}

fn adopt_backup_root(destination: &Path) -> PathBuf {
    destination
        .join(".agent/.cache/adopt/backups")
        .join(Ulid::new().to_string())
}

fn confirm_adopt_write(opts: &AdoptOpts) -> Result<()> {
    if opts.defaults || opts.no_input {
        return Ok(());
    }
    let stdin = io::stdin();
    let mut stderr = io::stderr();
    if !stdin.is_terminal() || !stderr.is_terminal() {
        bail!(
            "Adopt write needs confirmation but stdin or stderr is not a terminal. Re-run interactively, or pass --defaults or --no-input for noninteractive execution."
        );
    }

    write!(stderr, "Proceed with adopt --write? [y/N] ")
        .context("Failed to write adopt confirmation prompt")?;
    stderr
        .flush()
        .context("Failed to flush adopt confirmation prompt")?;
    let mut answer = String::new();
    stdin
        .read_line(&mut answer)
        .context("Failed to read adopt confirmation")?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        return Ok(());
    }
    bail!("Adopt write cancelled; re-run with --defaults or --no-input to skip confirmation.");
}

fn write_adopt_last_receipt(
    destination: &Path,
    backup_root: Option<&Path>,
    result: &initial_copy::BootstrapCopyResult,
) -> Result<()> {
    let adopt_cache_dir = destination.join(".agent/.cache/adopt");
    fs::create_dir_all(&adopt_cache_dir)
        .with_context(|| format!("Failed to create {}", adopt_cache_dir.display()))?;
    let receipt_path = adopt_cache_dir.join("adopt-last.json");
    let receipt = json!({
        "command": "adopt",
        "created_at_unix": OffsetDateTime::now_utc().unix_timestamp(),
        "destination": destination.display().to_string(),
        "backup_root": backup_root.map(|path| path.display().to_string()),
        "canonical_receipt_path": ".agent/.cache/adopt/adopt-last.json",
        "legacy_receipt_path": ".agent/state/adopt-last.json",
        "legacy_receipt_deprecated": true,
        "apply_report": &result.apply_report,
        "undo_hint": "Use apply_report.backups to restore modified or removed files, then delete paths listed in apply_report.files_created if you want to undo this adopt write. Delete backup_root when those backups are no longer needed.",
    });
    let text =
        serde_json::to_string_pretty(&receipt).context("Failed to serialize adopt receipt")?;
    fs::write(&receipt_path, format!("{text}\n"))
        .with_context(|| format!("Failed to write {}", receipt_path.display()))?;
    // TODO(jig-0.4): remove the legacy receipt copy after adopted repos have
    // had a release window to migrate readers to the canonical cache path.
    let legacy_receipt_path = destination.join(".agent/state/adopt-last.json");
    if let Some(parent) = legacy_receipt_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&legacy_receipt_path, format!("{text}\n"))
        .with_context(|| format!("Failed to write {}", legacy_receipt_path.display()))?;
    Ok(())
}

fn adoption_report(result: &initial_copy::BootstrapCopyResult) -> Value {
    json!({
        "dry_run": result.apply_report.dry_run,
        "files_created": &result.apply_report.files_created,
        "files_modified": &result.apply_report.files_modified,
        "files_removed": &result.apply_report.files_removed,
        "files_unchanged": &result.apply_report.files_unchanged,
        "managed_blocks_inserted": &result.apply_report.managed_blocks_inserted,
        "managed_blocks_rendered": &result.apply_report.managed_blocks_rendered,
        "backups": &result.apply_report.backups,
        "conflicts": &result.apply_report.conflicts,
        "commands_detected_or_skipped": initial_command_report(result),
        "todos": initial_todos(result),
        "suggested_jig_toml_edits": initial_suggested_jig_toml_edits(result),
    })
}

fn initial_command_report(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut commands = Vec::new();
    if result.bootstrap_command_configured {
        commands
            .push("bootstrap_command configured; run scripts/jig bootstrap before checks".into());
    } else {
        commands.push("bootstrap_command not configured; skip scripts/jig bootstrap".into());
    }
    commands.push("contract check available through scripts/jig check contract".into());
    if result.frontend_apps_configured {
        commands
            .push("[[dev.apps]] rendered from frontend app answers; run scripts/jig dev".into());
        commands
            .push("frontend app checks available through scripts/jig check typescript-*".into());
    } else {
        commands.push("no [[dev.apps]] configured; scripts/jig dev has no app to launch".into());
    }
    commands
}

fn initial_todos(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut todos = vec![
        "Review generated command strings in .jig.toml against this repo's actual setup.".into(),
        "Add or update crate-level AGENTS.md files for repo-owned business rules.".into(),
    ];
    if result.sqlx_enabled {
        todos.push("Confirm SQLx database access and committed metadata workflow.".into());
    }
    if result.schema_dump_enabled {
        todos.push("Provide the project-owned scripts/dump-schema.sh implementation.".into());
    }
    if result.frontend_apps_configured {
        todos.push(
            "Confirm each frontend app has package scripts and starts on the injected PORT/HOST."
                .into(),
        );
    }
    todos
}

fn initial_suggested_jig_toml_edits(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut edits = vec![
        "Replace generated fallback Cargo commands if this repo uses nested workspaces or non-Cargo checks.".into(),
    ];
    if result.frontend_apps_configured {
        edits.push("Tune [dev] ports, tld, HTTPS, LAN, and each [[dev.apps]] kind/argv if defaults do not match local development.".into());
    }
    if result.sqlx_enabled {
        edits.push("Set rust_migration_dir, rust_sqlx_metadata_dir, and sqlx_check_command to the repo-owned SQLx layout.".into());
    }
    edits
}

#[cfg(test)]
fn read_optional_answer_string(answers_path: &Path, key: &str) -> Result<Option<String>> {
    let answers = read_answers_toml(answers_path)?;
    Ok(answers
        .get(key)
        .and_then(TomlValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty()))
}

fn read_answers_toml(path: &Path) -> Result<Table> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
fn write_answers_toml(path: &Path, mapping: &Table) -> Result<()> {
    let toml = toml::to_string(mapping)
        .with_context(|| format!("Failed to serialize {}", path.display()))?;
    fs::write(path, toml).with_context(|| format!("Failed to write {}", path.display()))
}

fn parse_frontend_app(value: &str) -> Result<FrontendApp, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(parts.len() == 3 || parts.len() == 4) {
        return Err("expected <name>:<dir>:<coverage_threshold>[:kind]".into());
    }

    let coverage_threshold = parts[2]
        .parse::<u32>()
        .map_err(|error| format!("coverage_threshold must be a non-negative integer: {error}"))?;

    Ok(FrontendApp {
        name: parts[0].to_string(),
        dir: parts[1].to_string(),
        coverage_threshold,
        kind: parts.get(3).copied().unwrap_or("vite").to_string(),
    })
}

fn default_frontend_app_kind() -> String {
    "vite".into()
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

#[cfg(test)]
mod tests;
