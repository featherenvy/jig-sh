use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_yaml::{Mapping, Value as YamlValue};
use tempfile::TempDir;

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
const TEMPLATE_CACHE_RELATIVE_PATH: &str = ".agent/.cache/template-source";

#[derive(Debug, Args, Clone, Default)]
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

#[derive(Debug, Args, Clone)]
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

#[derive(Debug, Args, Clone)]
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

#[derive(Debug, Args, Clone)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateMode {
    Committed,
    WorkingTree,
}

impl TemplateMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::WorkingTree => "working-tree",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopierMode {
    Copy,
    Update,
    Recopy,
}

impl CopierMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Update => "update",
            Self::Recopy => "recopy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopierCommandSpec {
    program: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct PrivateAnswerOverrides {
    template_mode: Option<TemplateMode>,
    template_local_path: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedTemplateSource {
    copier_template: String,
    vcs_ref: Option<String>,
    private_answers: PrivateAnswerOverrides,
}

#[derive(Debug, Clone)]
struct StoredTemplateState {
    src_path: String,
    default_branch: Option<String>,
    template_mode: Option<TemplateMode>,
    template_local_path: Option<String>,
}

struct StagedRender {
    _root: TempDir,
    destination: PathBuf,
    answers_path: PathBuf,
    resolved_vcs_ref: Option<String>,
}

pub fn run_init(opts: InitOpts) -> Result<Value> {
    let destination = absolute_path(&opts.path)?;
    validate_init_destination(&destination, opts.force)?;
    let interactive = !(opts.defaults || opts.no_input);
    let template = prepare_template_source(
        &opts.template,
        opts.template_mode,
        opts.vcs_ref.as_deref(),
        &destination,
        false,
    )?;

    let seed_answers = TempAnswersFile::write_seed(&opts.answers, &template.private_answers)?;
    let staged = stage_render(
        &template.copier_template,
        template.vcs_ref.as_deref(),
        seed_answers.as_ref().map(|file| file.path()),
        None,
        opts.defaults || opts.no_input,
        interactive,
    )?;
    run_copier(
        build_copy_spec(
            &template.copier_template,
            &destination,
            Some(&staged.answers_path),
            staged.resolved_vcs_ref.as_deref(),
            opts.force,
            false,
            true,
            false,
        ),
        None,
        false,
    )?;
    rewrite_private_template_answers(
        &destination.join(ANSWERS_FILE),
        &PreparedTemplateSource {
            copier_template: template.copier_template.clone(),
            vcs_ref: staged.resolved_vcs_ref.clone(),
            private_answers: template.private_answers.clone(),
        },
    )?;

    let default_branch = read_default_branch(&staged.answers_path)?;
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
    let interactive = !(opts.defaults || opts.no_input);
    let template = prepare_template_source(
        &opts.template,
        opts.template_mode,
        opts.vcs_ref.as_deref(),
        &destination,
        false,
    )?;

    let seed_answers = TempAnswersFile::write_seed(&opts.answers, &template.private_answers)?;
    let staged = stage_render(
        &template.copier_template,
        template.vcs_ref.as_deref(),
        seed_answers.as_ref().map(|file| file.path()),
        Some(&destination),
        opts.defaults || opts.no_input,
        interactive,
    )?;

    if !opts.force {
        let conflicts =
            rendered_conflicts(&staged.destination, &staged.answers_path, &destination)?;
        if !conflicts.is_empty() {
            bail!(
                "Adopt would overwrite template-managed paths. Re-run with --force or clear these paths first:\n{}",
                conflicts.join("\n")
            );
        }
    }

    run_copier(
        build_copy_spec(
            &template.copier_template,
            &destination,
            Some(&staged.answers_path),
            staged.resolved_vcs_ref.as_deref(),
            opts.force,
            false,
            true,
            false,
        ),
        None,
        false,
    )?;
    rewrite_private_template_answers(
        &destination.join(ANSWERS_FILE),
        &PreparedTemplateSource {
            copier_template: template.copier_template.clone(),
            vcs_ref: staged.resolved_vcs_ref.clone(),
            private_answers: template.private_answers.clone(),
        },
    )?;

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
    let mut update_template = None;
    let mut answers_postwrite = None;
    let source_override_requested = opts.template.is_some() || opts.template_mode.is_some();
    let committed_vcs_ref_update =
        opts.vcs_ref.is_some() && stored.template_mode != Some(TemplateMode::WorkingTree);

    if source_override_requested || committed_vcs_ref_update {
        let template_arg = opts.template.as_deref().unwrap_or_else(|| {
            if stored.template_mode == Some(TemplateMode::WorkingTree)
                && opts.template_mode == Some(TemplateMode::Committed)
            {
                stored_template_local_path(&stored)
            } else {
                &stored.src_path
            }
        });
        let template_mode = opts.template_mode.or({
            if is_remote_template_source(template_arg) {
                None
            } else {
                stored.template_mode
            }
        });
        let prepared = prepare_template_source(
            template_arg,
            template_mode,
            opts.vcs_ref.as_deref(),
            &destination,
            true,
        )?;
        if !stored.src_path.is_empty() && !template_identities_match(&stored, &prepared) {
            bail!(
                "jig update cannot switch template source paths in-place. Re-run with the existing source path, or re-adopt the repo from the new template source."
            );
        }
        if stored.template_mode == Some(TemplateMode::WorkingTree)
            && prepared.private_answers.template_mode == Some(TemplateMode::Committed)
        {
            let template_root = absolute_path(Path::new(stored_template_local_path(&stored)))?;
            let snapshot_root = absolute_path(Path::new(&stored.src_path))?;
            let committed_vcs_ref = prepared.vcs_ref.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Missing committed template ref for relink update")
            })?;
            update_template = Some(prepare_snapshot_update_source(
                &template_root,
                &snapshot_root,
                committed_vcs_ref,
            )?);
            answers_postwrite = Some(prepared);
        } else {
            update_template = Some(prepared.clone());
            answers_postwrite = Some(final_update_template_state(&stored, &prepared));
        }
    } else if opts.vcs_ref.is_some() {
        bail!(
            "--vcs-ref requires a committed template source. Re-run with --template-mode committed when updating a working-tree template checkout."
        );
    } else {
        if stored.template_mode == Some(TemplateMode::WorkingTree) {
            let local_path = stored.template_local_path.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Missing {TEMPLATE_LOCAL_PATH_KEY} in {} for working-tree template mode",
                    answers_path.display()
                )
            })?;
            let prepared = prepare_template_source(
                local_path,
                Some(TemplateMode::WorkingTree),
                None,
                &destination,
                true,
            )?;
            update_template = Some(prepared);
            answers_postwrite = update_template.clone();
        }
    }
    run_copier(
        build_update_spec(
            mode,
            &destination,
            Path::new(ANSWERS_FILE),
            update_template
                .as_ref()
                .and_then(|prepared| prepared.vcs_ref.as_deref())
                .or(opts.vcs_ref.as_deref()),
            opts.defaults || opts.no_input,
        ),
        Some(&destination),
        !(opts.defaults || opts.no_input),
    )?;
    if let Some(mut prepared) = answers_postwrite {
        refresh_postwrite_template_metadata(&answers_path, &mut prepared)?;
        rewrite_private_template_answers(&answers_path, &prepared)?;
    }

    Ok(json!({
        "ok": true,
        "command": "update",
        "copier_mode": mode.as_str(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
    }))
}

fn prepare_template_source(
    template: &str,
    template_mode: Option<TemplateMode>,
    vcs_ref: Option<&str>,
    destination: &Path,
    update_existing: bool,
) -> Result<PreparedTemplateSource> {
    if is_remote_template_source(template) {
        if template_mode.is_some() {
            bail!("--template-mode only applies to local git template paths.");
        }
        return Ok(PreparedTemplateSource {
            copier_template: template.to_string(),
            vcs_ref: vcs_ref.map(str::to_string),
            private_answers: PrivateAnswerOverrides::default(),
        });
    }

    let local_template = absolute_path(Path::new(template))?;
    if !local_template.is_dir() {
        bail!(
            "Template path is not a directory: {}",
            local_template.display()
        );
    }

    if !local_template.join("copier.yml").exists() {
        bail!(
            "Template path does not contain copier.yml: {}",
            local_template.display()
        );
    }

    if !is_git_work_tree(&local_template) {
        if vcs_ref.is_some() {
            bail!(
                "--vcs-ref only applies to remote templates or local git template paths: {}",
                local_template.display()
            );
        }
        if template_mode.is_some() {
            bail!(
                "Local template mode requires a git working tree: {}",
                local_template.display()
            );
        }
        return Ok(PreparedTemplateSource {
            copier_template: local_template.display().to_string(),
            vcs_ref: vcs_ref.map(str::to_string),
            private_answers: PrivateAnswerOverrides::default(),
        });
    }

    let mode = template_mode.ok_or_else(|| {
        anyhow::anyhow!(
            "Local git template paths require --template-mode committed or --template-mode working-tree.\n\
             Example: jig adopt . --template {} --template-mode working-tree",
            local_template.display()
        )
    })?;

    match mode {
        TemplateMode::Committed => prepare_committed_template_source(&local_template, vcs_ref),
        TemplateMode::WorkingTree => {
            prepare_working_tree_template_source(&local_template, destination, update_existing)
        }
    }
}

fn prepare_committed_template_source(
    template_root: &Path,
    vcs_ref: Option<&str>,
) -> Result<PreparedTemplateSource> {
    ensure_clean_git_work_tree(template_root)?;
    let resolved_vcs_ref = match vcs_ref {
        Some(value) => git_stdout(template_root, ["rev-parse", &format!("{value}^{{commit}}")])?,
        None => git_stdout(template_root, ["rev-parse", "HEAD"])?,
    };

    Ok(PreparedTemplateSource {
        copier_template: template_root.display().to_string(),
        vcs_ref: Some(resolved_vcs_ref),
        private_answers: PrivateAnswerOverrides {
            template_mode: Some(TemplateMode::Committed),
            template_local_path: Some(template_root.display().to_string()),
        },
    })
}

fn prepare_working_tree_template_source(
    template_root: &Path,
    destination: &Path,
    update_existing: bool,
) -> Result<PreparedTemplateSource> {
    let snapshot_root = template_cache_root(destination);
    let snapshot_commit =
        refresh_template_snapshot(template_root, &snapshot_root, update_existing)?;

    Ok(PreparedTemplateSource {
        copier_template: snapshot_root.display().to_string(),
        vcs_ref: Some(snapshot_commit),
        private_answers: PrivateAnswerOverrides {
            template_mode: Some(TemplateMode::WorkingTree),
            template_local_path: Some(template_root.display().to_string()),
        },
    })
}

fn prepare_snapshot_update_source(
    template_root: &Path,
    snapshot_root: &Path,
    vcs_ref: &str,
) -> Result<PreparedTemplateSource> {
    ensure_clean_git_work_tree(template_root)?;
    let snapshot_commit =
        refresh_template_snapshot_from_ref(template_root, snapshot_root, vcs_ref, true)?;

    Ok(PreparedTemplateSource {
        copier_template: snapshot_root.display().to_string(),
        vcs_ref: Some(snapshot_commit),
        private_answers: PrivateAnswerOverrides::default(),
    })
}

fn rewrite_private_template_answers(
    answers_path: &Path,
    template: &PreparedTemplateSource,
) -> Result<()> {
    let mut answers = read_answers_yaml(answers_path)?;
    apply_private_template_answers(&mut answers, template);
    write_answers_yaml(answers_path, &answers)
}

fn apply_private_template_answers(answers: &mut Mapping, template: &PreparedTemplateSource) {
    answers.insert(
        YamlValue::String("_src_path".into()),
        YamlValue::String(template.copier_template.clone()),
    );
    answers.insert(
        YamlValue::String("_commit".into()),
        YamlValue::String(template.vcs_ref.clone().unwrap_or_default()),
    );
    set_optional_yaml_string(
        answers,
        TEMPLATE_MODE_KEY,
        template
            .private_answers
            .template_mode
            .map(TemplateMode::as_str),
    );
    set_optional_yaml_string(
        answers,
        TEMPLATE_LOCAL_PATH_KEY,
        template.private_answers.template_local_path.as_deref(),
    );
}

fn read_stored_template_state(answers_path: &Path) -> Result<StoredTemplateState> {
    Ok(StoredTemplateState {
        src_path: read_optional_answer_string(answers_path, "_src_path")?.unwrap_or_default(),
        default_branch: read_optional_answer_string(answers_path, "default_branch")?,
        template_mode: read_optional_answer_string(answers_path, TEMPLATE_MODE_KEY)?
            .as_deref()
            .map(parse_template_mode_answer)
            .transpose()?,
        template_local_path: read_optional_answer_string(answers_path, TEMPLATE_LOCAL_PATH_KEY)?,
    })
}

fn parse_template_mode_answer(value: &str) -> Result<TemplateMode> {
    match value {
        "committed" => Ok(TemplateMode::Committed),
        "working-tree" => Ok(TemplateMode::WorkingTree),
        other => bail!("Unsupported template mode '{other}' in {}", ANSWERS_FILE),
    }
}

fn stored_template_local_path(stored: &StoredTemplateState) -> &str {
    stored
        .template_local_path
        .as_deref()
        .unwrap_or(&stored.src_path)
}

fn template_identities_match(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> bool {
    let stored_identities = [
        Some(stored.src_path.as_str()),
        stored.template_local_path.as_deref(),
    ];
    let prepared_identities = [
        Some(prepared.copier_template.as_str()),
        prepared.private_answers.template_local_path.as_deref(),
    ];

    stored_identities
        .into_iter()
        .flatten()
        .any(|stored_identity| {
            prepared_identities
                .into_iter()
                .flatten()
                .any(|prepared_identity| stored_identity == prepared_identity)
        })
}

fn final_update_template_state(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> PreparedTemplateSource {
    let mut final_template = prepared.clone();
    let same_committed_template = stored.template_mode == Some(TemplateMode::Committed)
        && template_identities_match(stored, prepared);

    if same_committed_template {
        if final_template.private_answers.template_mode.is_none() {
            final_template.private_answers.template_mode = stored.template_mode;
        }
        if final_template.private_answers.template_local_path.is_none() {
            final_template.private_answers.template_local_path = stored.template_local_path.clone();
        }
    }

    if same_committed_template
        && final_template.private_answers.template_mode == Some(TemplateMode::Committed)
        && is_remote_template_source(&stored.src_path)
        && prepared.vcs_ref.is_some()
        && stored.default_branch.is_some()
    {
        let (commit, branch) = prepared
            .vcs_ref
            .as_deref()
            .zip(stored.default_branch.as_deref())
            .expect("checked above");
        if remote_template_commit_reachability(&stored.src_path, commit, branch)
            != RemoteCommitReachability::Unreachable
        {
            final_template.copier_template = stored.src_path.clone();
        }
    }
    final_template
}

fn refresh_postwrite_template_metadata(
    answers_path: &Path,
    template: &mut PreparedTemplateSource,
) -> Result<()> {
    if is_remote_template_source(&template.copier_template) {
        template.vcs_ref = read_optional_answer_string(answers_path, "_commit")?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteCommitReachability {
    Reachable,
    Unreachable,
    Unknown,
}

fn remote_template_commit_reachability(
    source: &str,
    commit: &str,
    branch: &str,
) -> RemoteCommitReachability {
    let probe = match tempfile::tempdir() {
        Ok(probe) => probe,
        Err(_) => return RemoteCommitReachability::Unknown,
    };
    if git(probe.path(), ["init", "--bare"]).is_err() {
        return RemoteCommitReachability::Unknown;
    }
    let fetch_ref = format!("refs/heads/{branch}:refs/remotes/origin/{branch}");
    if git(probe.path(), ["fetch", "--quiet", source, &fetch_ref]).is_err() {
        return RemoteCommitReachability::Unknown;
    }

    if git_command(
        probe.path(),
        [
            "merge-base",
            "--is-ancestor",
            commit,
            &format!("origin/{branch}"),
        ],
    )
    .output()
    .is_ok_and(|output| output.status.success())
    {
        RemoteCommitReachability::Reachable
    } else {
        RemoteCommitReachability::Unreachable
    }
}

fn template_cache_root(destination: &Path) -> PathBuf {
    destination.join(TEMPLATE_CACHE_RELATIVE_PATH)
}

fn refresh_template_snapshot(
    template_root: &Path,
    snapshot_root: &Path,
    update_existing: bool,
) -> Result<String> {
    fs::create_dir_all(snapshot_root)
        .with_context(|| format!("Failed to create {}", snapshot_root.display()))?;
    ensure_git_repo(snapshot_root)?;
    clear_snapshot_worktree(snapshot_root)?;
    copy_working_tree_snapshot(template_root, snapshot_root)?;
    git(snapshot_root, ["add", "-A"])?;

    let has_changes = !git_stdout(snapshot_root, ["status", "--porcelain"])?.is_empty();
    let has_head = git_command(snapshot_root, ["rev-parse", "--verify", "HEAD"])
        .output()
        .is_ok_and(|output| output.status.success());

    if has_changes || !has_head {
        let message = if update_existing {
            format!("Refresh template snapshot from {}", template_root.display())
        } else {
            format!("Create template snapshot from {}", template_root.display())
        };
        git_with_config(
            snapshot_root,
            [
                "-c",
                "user.name=jig",
                "-c",
                "user.email=jig@local.invalid",
                "commit",
                "-m",
                &message,
            ],
        )?;
    }

    git_stdout(snapshot_root, ["rev-parse", "HEAD"])
}

fn refresh_template_snapshot_from_ref(
    template_root: &Path,
    snapshot_root: &Path,
    vcs_ref: &str,
    update_existing: bool,
) -> Result<String> {
    let worktree_root = TempDir::new().context("Failed to create temporary worktree root")?;
    let worktree_path = worktree_root.path().join("template");
    git(
        template_root,
        [
            "worktree",
            "add",
            "--detach",
            &worktree_path.display().to_string(),
            vcs_ref,
        ],
    )?;

    let result = refresh_template_snapshot(&worktree_path, snapshot_root, update_existing);
    let _ = git(
        template_root,
        [
            "worktree",
            "remove",
            "--force",
            &worktree_path.display().to_string(),
        ],
    );
    result
}

fn ensure_git_repo(path: &Path) -> Result<()> {
    if path.join(".git").exists() {
        return Ok(());
    }

    let git_program = external_program(GIT_BIN_ENV, "git");
    let output = Command::new(&git_program)
        .current_dir(path)
        .args(["init", "-b", "main"])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    if output.status.success() {
        return Ok(());
    }
    if !git_init_branch_flag_unsupported(&output) {
        bail!(
            "git init -b main failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    git(path, ["init"])?;
    set_git_head_branch(path, &git_program, "main")
}

fn clear_snapshot_worktree(snapshot_root: &Path) -> Result<()> {
    for entry in fs::read_dir(snapshot_root)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let entry_path = entry.path();
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(&entry_path)
                .with_context(|| format!("Failed to remove {}", entry_path.display()))?;
        } else {
            fs::remove_file(&entry_path)
                .with_context(|| format!("Failed to remove {}", entry_path.display()))?;
        }
    }
    Ok(())
}

fn copy_working_tree_snapshot(template_root: &Path, snapshot_root: &Path) -> Result<()> {
    let output = git_command(
        template_root,
        [
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .output()
    .with_context(|| format!("Failed to start git in {}", template_root.display()))?;
    if !output.status.success() {
        bail!(
            "git ls-files failed for {}\nstdout:\n{}\nstderr:\n{}",
            template_root.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for relative in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let relative = std::str::from_utf8(relative)
            .with_context(|| format!("Non-UTF-8 git path under {}", template_root.display()))?;
        let source_path = template_root.join(relative);
        let destination_path = snapshot_root.join(relative);
        copy_path_entry(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_path_entry(source_path: &Path, destination_path: &Path) -> Result<()> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let metadata = fs::symlink_metadata(source_path)
        .with_context(|| format!("Failed to stat {}", source_path.display()))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(source_path)
            .with_context(|| format!("Failed to read symlink {}", source_path.display()))?;
        create_symlink(&target, destination_path)?;
        return Ok(());
    }

    if file_type.is_dir() {
        fs::create_dir_all(destination_path)
            .with_context(|| format!("Failed to create {}", destination_path.display()))?;
        return Ok(());
    }

    fs::copy(source_path, destination_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source_path.display(),
            destination_path.display()
        )
    })?;
    fs::set_permissions(destination_path, metadata.permissions()).with_context(|| {
        format!(
            "Failed to set permissions on {}",
            destination_path.display()
        )
    })?;
    Ok(())
}

fn is_remote_template_source(template: &str) -> bool {
    template.contains("://") || template.starts_with("git@") && template.contains(':')
}

fn is_git_work_tree(path: &Path) -> bool {
    git_command(path, ["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn ensure_clean_git_work_tree(path: &Path) -> Result<()> {
    let status = git_stdout(path, ["status", "--short"])?;
    if !status.is_empty() {
        bail!(
            "Local committed template mode requires a clean git working tree: {}\n\
             Commit or stash template changes, or re-run with --template-mode working-tree.",
            path.display()
        );
    }
    Ok(())
}

fn git(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<()> {
    let output = git_command(path, args).output()?;
    if !output.status.success() {
        bail!(
            "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn git_stdout(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<String> {
    let output = git_command(path, args).output()?;
    if !output.status.success() {
        bail!(
            "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_with_config(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<()> {
    let output = git_command(path, args).output()?;
    if !output.status.success() {
        bail!(
            "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn git_command(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Command {
    let git_program = external_program(GIT_BIN_ENV, "git");
    let mut command = Command::new(git_program);
    command.current_dir(path);
    for arg in args {
        command.arg(arg.as_ref());
    }
    command
}

fn stage_render(
    template: &str,
    vcs_ref: Option<&str>,
    answers_data_path: Option<&Path>,
    seed_repo_path: Option<&Path>,
    non_interactive_defaults: bool,
    interactive: bool,
) -> Result<StagedRender> {
    let root = TempDir::new().context("Failed to create staging directory")?;
    let destination = root.path().join("render");
    if let Some(seed_repo_path) = seed_repo_path {
        seed_preview_workspace(seed_repo_path, &destination)?;
    }
    run_copier(
        build_copy_spec(
            template,
            &destination,
            answers_data_path,
            vcs_ref,
            false,
            seed_repo_path.is_some(),
            non_interactive_defaults,
            true,
        ),
        None,
        interactive,
    )?;

    let answers_path = destination.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Staging render did not produce {} in {}",
            ANSWERS_FILE,
            destination.display()
        );
    }
    let resolved_vcs_ref = read_optional_answer_string(&answers_path, "_commit")?;
    Ok(StagedRender {
        _root: root,
        destination,
        answers_path,
        resolved_vcs_ref,
    })
}

fn read_default_branch(answers_path: &Path) -> Result<String> {
    let value = read_optional_answer_string(answers_path, "default_branch")?
        .ok_or_else(|| anyhow::anyhow!("Missing default_branch in {}", answers_path.display()))?;
    Ok(value.to_string())
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

fn rendered_conflicts(
    rendered_root: &Path,
    answers_path: &Path,
    destination: &Path,
) -> Result<Vec<String>> {
    let mut conflicts = BTreeSet::new();
    collect_sync_conflicts(rendered_root, destination, rendered_root, &mut conflicts)?;
    for relative in ALWAYS_TASK_MUTATED_PATHS {
        let path = destination.join(relative);
        if path.exists() {
            conflicts.insert((*relative).to_string());
        }
    }
    if read_optional_answer_bool(answers_path, "sqlx_enabled")? == Some(false) {
        for relative in SQLX_PRUNED_TASK_PATHS {
            let path = destination.join(relative);
            if path.exists() {
                conflicts.insert((*relative).to_string());
            }
        }
    }
    Ok(conflicts.into_iter().collect())
}

fn collect_sync_conflicts(
    rendered_root: &Path,
    destination_root: &Path,
    current_rendered: &Path,
    conflicts: &mut BTreeSet<String>,
) -> Result<()> {
    for entry in fs::read_dir(current_rendered)? {
        let entry = entry?;
        let rendered_path = entry.path();
        let relative = rendered_path.strip_prefix(rendered_root).with_context(|| {
            format!(
                "{} is not under {}",
                rendered_path.display(),
                rendered_root.display()
            )
        })?;
        let destination_path = destination_root.join(relative);
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if destination_path.exists() && !destination_path.is_dir() {
                conflicts.insert(relative.display().to_string());
                continue;
            }
            collect_sync_conflicts(rendered_root, destination_root, &rendered_path, conflicts)?;
            continue;
        }

        if destination_path.exists() && !files_match(&rendered_path, &destination_path)? {
            conflicts.insert(relative.display().to_string());
        }
    }
    Ok(())
}

fn build_copy_spec(
    template: &str,
    destination: &Path,
    answers_data_path: Option<&Path>,
    vcs_ref: Option<&str>,
    force: bool,
    overwrite: bool,
    use_defaults: bool,
    skip_tasks: bool,
) -> CopierCommandSpec {
    let mut args = vec![
        "--from".into(),
        "copier".into(),
        "copier".into(),
        CopierMode::Copy.as_str().into(),
        "--trust".into(),
        "--answers-file".into(),
        ANSWERS_FILE.into(),
    ];
    if skip_tasks {
        args.push("--skip-tasks".into());
    }
    if let Some(answers_data_path) = answers_data_path {
        args.push("--data-file".into());
        args.push(answers_data_path.display().to_string());
    }
    if force {
        args.push("--force".into());
    } else {
        if overwrite {
            args.push("--overwrite".into());
        }
        if use_defaults {
            args.push("--defaults".into());
        }
    }
    if let Some(vcs_ref) = vcs_ref {
        args.push("--vcs-ref".into());
        args.push(vcs_ref.to_string());
    }
    args.push(template.to_string());
    args.push(destination.display().to_string());

    CopierCommandSpec {
        program: external_program(UVX_BIN_ENV, "uvx"),
        args,
    }
}

fn build_update_spec(
    mode: CopierMode,
    destination: &Path,
    answers_file: &Path,
    vcs_ref: Option<&str>,
    defaults: bool,
) -> CopierCommandSpec {
    let mut args = vec![
        "--from".into(),
        "copier".into(),
        "copier".into(),
        mode.as_str().into(),
        "--trust".into(),
        "--answers-file".into(),
        answers_file.display().to_string(),
    ];
    if defaults || mode == CopierMode::Recopy {
        args.push("--defaults".into());
    }
    if mode == CopierMode::Recopy {
        args.push("--overwrite".into());
    }
    if let Some(vcs_ref) = vcs_ref {
        args.push("--vcs-ref".into());
        args.push(vcs_ref.to_string());
    }
    args.push(destination.display().to_string());

    CopierCommandSpec {
        program: external_program(UVX_BIN_ENV, "uvx"),
        args,
    }
}

fn run_copier(
    spec: CopierCommandSpec,
    current_dir: Option<&Path>,
    interactive: bool,
) -> Result<()> {
    let mut command = Command::new(&spec.program);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    command.args(&spec.args);

    if interactive {
        let status = command
            .status()
            .with_context(|| format!("Failed to start {}", spec.program))?;
        if !status.success() {
            bail!(
                "Copier command failed with status {}",
                status.code().unwrap_or(1)
            );
        }
    } else {
        let output = command
            .output()
            .with_context(|| format!("Failed to start {}", spec.program))?;
        if !output.status.success() {
            bail!(
                "Copier command failed with status {}\nstdout:\n{}\nstderr:\n{}",
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}

fn files_match(rendered_path: &Path, destination_path: &Path) -> Result<bool> {
    let rendered_meta = fs::metadata(rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let destination_meta = fs::metadata(destination_path)
        .with_context(|| format!("Failed to read {}", destination_path.display()))?;
    if rendered_meta.is_file() != destination_meta.is_file() {
        return Ok(false);
    }
    if !rendered_meta.is_file() {
        return Ok(false);
    }

    let rendered = fs::read(rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let destination = fs::read(destination_path)
        .with_context(|| format!("Failed to read {}", destination_path.display()))?;
    Ok(rendered == destination)
}

fn seed_preview_workspace(source_root: &Path, destination_root: &Path) -> Result<()> {
    fs::create_dir_all(destination_root)
        .with_context(|| format!("Failed to create {}", destination_root.display()))?;
    copy_agent_guides_recursive(source_root, destination_root, source_root)
}

fn copy_agent_guides_recursive(
    source_root: &Path,
    destination_root: &Path,
    current_source: &Path,
) -> Result<()> {
    for entry in fs::read_dir(current_source)? {
        let entry = entry?;
        let source_path = entry.path();
        let relative = source_path.strip_prefix(source_root).with_context(|| {
            format!(
                "{} is not under {}",
                source_path.display(),
                source_root.display()
            )
        })?;
        if relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == ".git")
        {
            continue;
        }
        let destination_path = destination_root.join(relative);
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("Failed to create {}", destination_path.display()))?;
            copy_agent_guides_recursive(source_root, destination_root, &source_path)?;
            continue;
        }

        let file_name = source_path.file_name().and_then(|name| name.to_str());
        if file_name != Some("AGENTS.md") {
            continue;
        }

        copy_preview_guide(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_preview_guide(source_path: &Path, destination_path: &Path) -> Result<()> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let metadata = fs::symlink_metadata(source_path)
        .with_context(|| format!("Failed to stat {}", source_path.display()))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(source_path)
            .with_context(|| format!("Failed to read symlink {}", source_path.display()))?;
        create_symlink(&target, destination_path)?;
        return Ok(());
    }

    fs::copy(source_path, destination_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source_path.display(),
            destination_path.display()
        )
    })?;
    fs::set_permissions(destination_path, metadata.permissions()).with_context(|| {
        format!(
            "Failed to set permissions on {}",
            destination_path.display()
        )
    })?;
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link_path: &Path) -> Result<()> {
    use std::os::unix::fs as unix_fs;

    if link_path.exists() {
        fs::remove_file(link_path)
            .with_context(|| format!("Failed to remove {}", link_path.display()))?;
    }
    unix_fs::symlink(target, link_path).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link_path.display(),
            target.display()
        )
    })?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink(target: &Path, link_path: &Path) -> Result<()> {
    use std::os::windows::fs as windows_fs;

    if link_path.exists() {
        fs::remove_file(link_path)
            .with_context(|| format!("Failed to remove {}", link_path.display()))?;
    }
    windows_fs::symlink_file(target, link_path).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link_path.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn init_git_repo(destination: &Path, default_branch: &str) -> Result<bool> {
    if destination.join(".git").exists() {
        return Ok(false);
    }

    let git_program = external_program(GIT_BIN_ENV, "git");
    let with_branch = Command::new(&git_program)
        .current_dir(destination)
        .args(["init", "-b", default_branch])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;

    if with_branch.status.success() {
        return Ok(true);
    }
    if !git_init_branch_flag_unsupported(&with_branch) {
        bail!(
            "git init -b {default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&with_branch.stdout),
            String::from_utf8_lossy(&with_branch.stderr)
        );
    }

    let fallback = Command::new(&git_program)
        .current_dir(destination)
        .arg("init")
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    if !fallback.status.success() {
        bail!(
            "git init failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&fallback.stdout),
            String::from_utf8_lossy(&fallback.stderr)
        );
    }
    set_git_head_branch(destination, &git_program, default_branch)?;
    Ok(true)
}

fn git_init_branch_flag_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("unknown switch `b")
        || stderr.contains("unknown option `b")
        || stderr.contains("unknown option `initial-branch")
        || stderr.contains("unknown option `initial branch")
}

fn set_git_head_branch(destination: &Path, git_program: &str, default_branch: &str) -> Result<()> {
    let output = Command::new(git_program)
        .current_dir(destination)
        .args([
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    if !output.status.success() {
        bail!(
            "git symbolic-ref HEAD refs/heads/{default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
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

struct TempAnswersFile {
    path: PathBuf,
}

impl TempAnswersFile {
    fn write_seed(
        opts: &AnswerOpts,
        private_answers: &PrivateAnswerOverrides,
    ) -> Result<Option<Self>> {
        let value = seed_answers_yaml(opts, private_answers);
        if value.as_mapping().is_some_and(Mapping::is_empty) {
            return Ok(None);
        }

        let path = env::temp_dir().join(format!("{}.yaml", unique_id("answers")));
        let yaml = serde_yaml::to_string(&value)?;
        fs::write(&path, yaml).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(Some(Self { path }))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempAnswersFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn seed_answers_yaml(opts: &AnswerOpts, private_answers: &PrivateAnswerOverrides) -> YamlValue {
    let mut mapping = Mapping::new();
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
        "migration_add_command",
        opts.migration_add_command.as_deref(),
    );
    insert_string(
        &mut mapping,
        "bootstrap_command",
        opts.bootstrap_command.as_deref(),
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
        private_answers.template_mode.map(TemplateMode::as_str),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_LOCAL_PATH_KEY,
        private_answers.template_local_path.as_deref(),
    );

    if !opts.rust_crate_roots.is_empty() {
        mapping.insert(
            YamlValue::String("rust_crate_roots".into()),
            YamlValue::Sequence(
                opts.rust_crate_roots
                    .iter()
                    .cloned()
                    .map(YamlValue::String)
                    .collect(),
            ),
        );
    }
    if !opts.frontend_apps.is_empty() {
        mapping.insert(
            YamlValue::String("frontend_apps".into()),
            YamlValue::Sequence(
                opts.frontend_apps
                    .iter()
                    .map(|app| serde_yaml::to_value(app).unwrap_or(YamlValue::Null))
                    .collect(),
            ),
        );
    }

    YamlValue::Mapping(mapping)
}

fn insert_string(mapping: &mut Mapping, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(
            YamlValue::String(key.to_string()),
            YamlValue::String(value.to_string()),
        );
    }
}

fn insert_bool(mapping: &mut Mapping, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        mapping.insert(YamlValue::String(key.to_string()), YamlValue::Bool(value));
    }
}

fn unique_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("jig-{prefix}-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn template_repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    }

    fn copy_dir_recursive(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let file_type = entry.file_type().unwrap();

            if entry.file_name() == ".git" {
                continue;
            }

            if file_type.is_dir() {
                copy_dir_recursive(&source_path, &destination_path);
                continue;
            }

            if file_type.is_symlink() {
                if let Some(parent) = destination_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                let target = fs::read_link(&source_path).unwrap();
                create_symlink(&target, &destination_path).unwrap();
                continue;
            }

            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::copy(&source_path, &destination_path).unwrap();
        }
    }

    fn materialize_template_worktree() -> TempDir {
        let temp = tempdir().unwrap();
        copy_dir_recursive(&template_repo_root(), temp.path());
        temp
    }

    fn materialize_template_git_worktree() -> TempDir {
        let temp = materialize_template_worktree();
        init_git_repo_for_test(temp.path());
        git(temp.path(), ["add", "."]).unwrap();
        git(temp.path(), ["commit", "-m", "template"]).unwrap();
        temp
    }

    #[test]
    fn parses_frontend_app_flag() {
        let app = parse_frontend_app("frontend:web:40").unwrap();
        assert_eq!(
            app,
            FrontendApp {
                name: "frontend".into(),
                dir: "web".into(),
                coverage_threshold: 40,
            }
        );
    }

    #[test]
    fn seed_answers_only_serializes_provided_values() {
        let yaml = seed_answers_yaml(
            &AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                rust_crate_roots: vec!["crates".into()],
                frontend_apps: vec![FrontendApp {
                    name: "frontend".into(),
                    dir: "web".into(),
                    coverage_threshold: 40,
                }],
                ..AnswerOpts::default()
            },
            &PrivateAnswerOverrides::default(),
        );

        let mapping = yaml.as_mapping().unwrap();
        assert_eq!(
            mapping.get(YamlValue::String("repo_name".into())).unwrap(),
            &YamlValue::String("demo".into())
        );
        assert_eq!(
            mapping
                .get(YamlValue::String("sqlx_enabled".into()))
                .unwrap(),
            &YamlValue::Bool(false)
        );
        assert!(mapping.contains_key(YamlValue::String("rust_crate_roots".into())));
        assert!(!mapping.contains_key(YamlValue::String("default_branch".into())));
    }

    #[test]
    fn build_copy_spec_uses_force_for_overwrite_mode() {
        let spec = build_copy_spec(
            "/tmp/template",
            Path::new("/tmp/dest"),
            Some(Path::new("/tmp/answers.yml")),
            Some("HEAD"),
            true,
            false,
            true,
            false,
        );

        assert_eq!(spec.program, "uvx");
        assert!(spec.args.contains(&"--force".to_string()));
        assert!(spec.args.contains(&"--vcs-ref".to_string()));
        assert!(!spec.args.contains(&"--defaults".to_string()));
        assert!(!spec.args.contains(&"--skip-tasks".to_string()));
    }

    #[test]
    fn build_update_spec_switches_to_recopy_and_overwrite() {
        let spec = build_update_spec(
            CopierMode::Recopy,
            Path::new("/tmp/dest"),
            Path::new("/tmp/dest/.jig.yml"),
            None,
            false,
        );
        assert_eq!(spec.args[3], "recopy");
        assert!(spec.args.contains(&"--defaults".to_string()));
        assert!(spec.args.contains(&"--overwrite".to_string()));
    }

    fn write_answers_fixture(dir: &Path, sqlx_enabled: Option<bool>) {
        let mut body = String::from("default_branch: main\n");
        if let Some(sqlx_enabled) = sqlx_enabled {
            body.push_str(&format!(
                "sqlx_enabled: {}\n",
                if sqlx_enabled { "true" } else { "false" }
            ));
        }
        fs::write(dir.join(".jig.yml"), body).unwrap();
    }

    #[test]
    fn rendered_conflicts_detects_generated_paths() {
        let rendered = tempdir().unwrap();
        let destination = tempdir().unwrap();
        fs::create_dir_all(rendered.path().join("scripts")).unwrap();
        fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
        write_answers_fixture(rendered.path(), Some(true));
        fs::create_dir_all(destination.path().join("scripts")).unwrap();
        fs::write(destination.path().join("scripts/jig"), "existing").unwrap();

        let conflicts = rendered_conflicts(
            rendered.path(),
            &rendered.path().join(".jig.yml"),
            destination.path(),
        )
        .unwrap();
        assert_eq!(conflicts, vec!["scripts/jig"]);
    }

    #[test]
    fn rendered_conflicts_marks_task_mutated_outputs() {
        let rendered = tempdir().unwrap();
        let destination = tempdir().unwrap();
        write_answers_fixture(rendered.path(), Some(true));
        fs::write(rendered.path().join("agent-map.md"), "placeholder").unwrap();
        fs::write(destination.path().join("agent-map.md"), "existing").unwrap();

        let conflicts = rendered_conflicts(
            rendered.path(),
            &rendered.path().join(".jig.yml"),
            destination.path(),
        )
        .unwrap();
        assert_eq!(conflicts, vec!["agent-map.md"]);
    }

    #[test]
    fn rendered_conflicts_marks_sqlx_pruned_task_outputs() {
        let rendered = tempdir().unwrap();
        let destination = tempdir().unwrap();
        write_answers_fixture(rendered.path(), Some(false));
        fs::create_dir_all(rendered.path().join("scripts")).unwrap();
        fs::create_dir_all(destination.path().join("scripts")).unwrap();
        fs::write(
            rendered.path().join("scripts/add-migration.sh"),
            "templated",
        )
        .unwrap();
        fs::write(
            destination.path().join("scripts/add-migration.sh"),
            "existing",
        )
        .unwrap();

        let conflicts = rendered_conflicts(
            rendered.path(),
            &rendered.path().join(".jig.yml"),
            destination.path(),
        )
        .unwrap();
        assert_eq!(conflicts, vec!["scripts/add-migration.sh"]);
    }

    #[test]
    fn rendered_conflicts_ignores_identical_files() {
        let rendered = tempdir().unwrap();
        let destination = tempdir().unwrap();
        write_answers_fixture(rendered.path(), Some(true));
        fs::create_dir_all(rendered.path().join("scripts")).unwrap();
        fs::create_dir_all(destination.path().join("scripts")).unwrap();
        fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
        fs::write(destination.path().join("scripts/jig"), "same").unwrap();

        let conflicts = rendered_conflicts(
            rendered.path(),
            &rendered.path().join(".jig.yml"),
            destination.path(),
        )
        .unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn rendered_conflicts_detects_blocking_ancestor_file() {
        let rendered = tempdir().unwrap();
        let destination = tempdir().unwrap();
        write_answers_fixture(rendered.path(), Some(true));
        fs::create_dir_all(rendered.path().join("scripts")).unwrap();
        fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
        fs::write(destination.path().join("scripts"), "blocking file").unwrap();

        let conflicts = rendered_conflicts(
            rendered.path(),
            &rendered.path().join(".jig.yml"),
            destination.path(),
        )
        .unwrap();
        assert_eq!(conflicts, vec!["scripts"]);
    }

    #[test]
    fn preview_workspace_only_copies_agent_guides() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        fs::create_dir_all(source.path().join("crates/api")).unwrap();
        fs::create_dir_all(source.path().join("target/debug")).unwrap();
        fs::write(source.path().join("AGENTS.md"), "root").unwrap();
        fs::write(source.path().join("crates/api/AGENTS.md"), "nested").unwrap();
        fs::write(source.path().join("target/debug/build.log"), "noise").unwrap();

        seed_preview_workspace(source.path(), destination.path()).unwrap();

        assert!(destination.path().join("AGENTS.md").exists());
        assert!(destination.path().join("crates/api/AGENTS.md").exists());
        assert!(!destination.path().join("target/debug/build.log").exists());
    }

    #[test]
    fn build_copy_spec_can_skip_tasks_for_staging() {
        let spec = build_copy_spec(
            "/tmp/template",
            Path::new("/tmp/dest"),
            None,
            None,
            false,
            true,
            true,
            true,
        );

        assert!(spec.args.contains(&"--skip-tasks".to_string()));
        assert!(spec.args.contains(&"--defaults".to_string()));
    }

    #[test]
    fn run_init_uses_stubbed_uvx_and_git() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let log_path = temp.path().join("commands.log");
        let uvx_path = bin_dir.join("uvx-stub.sh");
        fs::write(
            &uvx_path,
            format!(
                "#!/bin/sh\nprintf 'uvx %s\\n' \"$*\" >> \"{}\"\ncmd=\"$4\"\nfor arg in \"$@\"; do dest=\"$arg\"; done\nmkdir -p \"$dest\"\ncat > \"$dest/.jig.yml\" <<'EOF'\ndefault_branch: main\nEOF\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        let git_path = bin_dir.join("git-stub.sh");
        fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&uvx_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        unsafe {
            env::set_var(UVX_BIN_ENV, &uvx_path);
            env::set_var(GIT_BIN_ENV, &git_path);
        }

        let destination = temp.path().join("repo");
        let output = run_init(InitOpts {
            path: destination.clone(),
            template: "git@github.com:demo/template.git".into(),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                rust_migration_dir: Some("migrations".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        unsafe {
            env::remove_var(UVX_BIN_ENV);
            env::remove_var(GIT_BIN_ENV);
        }

        assert_eq!(output["git_initialized"], true);
        let log = fs::read_to_string(&log_path).unwrap();
        let copy_count = log.matches("uvx --from copier copier copy --trust").count();
        assert_eq!(copy_count, 2);
        assert!(log.contains("git init -b main"));
        assert!(destination.exists());
    }

    #[test]
    fn run_init_falls_back_only_for_unsupported_git_branch_flag() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let log_path = temp.path().join("commands.log");
        let uvx_path = bin_dir.join("uvx-stub.sh");
        fs::write(
            &uvx_path,
            format!(
                "#!/bin/sh\nprintf 'uvx %s\\n' \"$*\" >> \"{}\"\nfor arg in \"$@\"; do dest=\"$arg\"; done\nmkdir -p \"$dest\"\ncat > \"$dest/.jig.yml\" <<'EOF'\ndefault_branch: trunk\nEOF\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        let git_path = bin_dir.join("git-stub.sh");
        fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'error: unknown switch `b`\\n' >&2\n  exit 129\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&uvx_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        unsafe {
            env::set_var(UVX_BIN_ENV, &uvx_path);
            env::set_var(GIT_BIN_ENV, &git_path);
        }

        let destination = temp.path().join("repo");
        let output = run_init(InitOpts {
            path: destination,
            template: "git@github.com:demo/template.git".into(),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        unsafe {
            env::remove_var(UVX_BIN_ENV);
            env::remove_var(GIT_BIN_ENV);
        }

        assert_eq!(output["git_initialized"], true);
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("git init -b trunk"));
        assert!(log.contains("git init"));
        assert!(log.contains("git symbolic-ref HEAD refs/heads/trunk"));
    }

    #[test]
    fn run_init_surfaces_git_branch_init_failures() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let log_path = temp.path().join("commands.log");
        let uvx_path = bin_dir.join("uvx-stub.sh");
        fs::write(
            &uvx_path,
            format!(
                "#!/bin/sh\nprintf 'uvx %s\\n' \"$*\" >> \"{}\"\nfor arg in \"$@\"; do dest=\"$arg\"; done\nmkdir -p \"$dest\"\ncat > \"$dest/.jig.yml\" <<'EOF'\ndefault_branch: main\nEOF\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        let git_path = bin_dir.join("git-stub.sh");
        fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'fatal: repository storage is broken\\n' >&2\n  exit 1\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&uvx_path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        unsafe {
            env::set_var(UVX_BIN_ENV, &uvx_path);
            env::set_var(GIT_BIN_ENV, &git_path);
        }

        let error = run_init(InitOpts {
            path: temp.path().join("repo"),
            template: "git@github.com:demo/template.git".into(),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: false,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();

        unsafe {
            env::remove_var(UVX_BIN_ENV);
            env::remove_var(GIT_BIN_ENV);
        }

        assert!(error.contains("git init -b main failed"));
        assert!(error.contains("repository storage is broken"));
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("git init -b main"));
        assert!(!log.contains("git symbolic-ref HEAD refs/heads/main"));
    }

    #[test]
    fn adopt_with_real_template_runs_destination_tasks() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                rust_migration_dir: Some("migrations".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
        assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
        assert!(!repo.join("scripts/add-migration.sh").exists());
        assert!(
            !repo
                .join("scripts/check-migration-immutability.sh")
                .exists()
        );
        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("sqlx_enabled: false"));
    }

    #[test]
    fn adopt_with_real_template_keeps_sqlx_files_when_enabled() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(true),
                rust_migration_dir: Some("migrations".into()),
                rust_sqlx_metadata_dir: Some(".sqlx".into()),
                migration_add_command: Some("scripts/add-migration.sh".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
        assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
        assert!(repo.join("scripts/add-migration.sh").exists());
        assert!(
            repo.join("scripts/check-migration-immutability.sh")
                .exists()
        );
        assert!(
            repo.join("scripts/check-sqlx-unchecked-non-test.sh")
                .exists()
        );
        assert!(
            repo.join("scripts/generate-sqlx-unchecked-queries-todo.sh")
                .exists()
        );
        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("sqlx_enabled: true"));
    }

    #[test]
    fn adopt_with_sqlx_and_schema_dumps_disabled_hides_schema_dump_target() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(true),
                schema_dump_enabled: Some(false),
                rust_migration_dir: Some("migrations".into()),
                rust_sqlx_metadata_dir: Some(".sqlx".into()),
                migration_add_command: Some("scripts/add-migration.sh".into()),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let makefile = fs::read_to_string(repo.join("Makefile")).unwrap();
        assert!(!makefile.contains("schema-dump: ##"));
        assert!(!makefile.contains(" schema-dump "));

        let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
        assert!(!contract.contains("\"schema-dump\""));
        assert!(!contract.contains("jig.schema_dump"));
    }

    fn init_git_repo_for_test(path: &Path) {
        git(path, ["init", "-b", "main"]).unwrap();
        git(path, ["config", "user.name", "Fixture"]).unwrap();
        git(path, ["config", "user.email", "fixture@example.com"]).unwrap();
    }

    #[test]
    fn adopt_requires_template_mode_for_local_git_templates() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        let error = run_adopt(AdoptOpts {
            path: repo,
            template: template.path().display().to_string(),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("require --template-mode"));
    }

    #[test]
    fn adopt_committed_mode_rejects_dirty_local_template() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        let error = run_adopt(AdoptOpts {
            path: repo,
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("clean git working tree"));
    }

    #[test]
    fn adopt_working_tree_mode_renders_uncommitted_template_changes() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Working Tree Marker\n",
        )
        .unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::WorkingTree),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Working Tree Marker"));
        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("_template_mode: working-tree"));
        assert!(answers.contains("_template_local_path:"));
        assert!(repo.join(".agent/.cache/template-source/.git").exists());
    }

    #[test]
    fn update_working_tree_mode_refreshes_template_snapshot() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::WorkingTree),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Updated Working Tree Marker\n",
        )
        .unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy: false,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Updated Working Tree Marker"));
        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("_template_mode: working-tree"));
    }

    #[test]
    fn update_can_relink_working_tree_repo_to_committed_template_mode() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Working Tree Marker\n",
        )
        .unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::WorkingTree),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();
        fs::write(repo.join("AGENTS.md"), "# Repo Marker\n").unwrap();
        git(&repo, ["add", "AGENTS.md"]).unwrap();
        git(&repo, ["commit", "-m", "repo change"]).unwrap();

        git(
            template.path(),
            ["checkout", "--", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: Some(template.path().display().to_string()),
            template_mode: Some(TemplateMode::Committed),
            recopy: false,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(!root_guide.contains("Working Tree Marker"));
        assert!(root_guide.contains("Repo Marker"));

        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("_template_mode: committed"));
        assert!(answers.contains(&template.path().display().to_string()));
        assert!(!answers.contains(TEMPLATE_CACHE_RELATIVE_PATH));
    }

    #[test]
    fn update_relink_to_committed_mode_honors_requested_vcs_ref() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Older Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "older template"]).unwrap();
        let old_ref = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Newer Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "newer template"]).unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::WorkingTree),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: Some(template.path().display().to_string()),
            template_mode: Some(TemplateMode::Committed),
            recopy: false,
            vcs_ref: Some(old_ref.clone()),
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Older Marker"));
        assert!(!root_guide.contains("Newer Marker"));

        let answers_path = repo.join(".jig.yml");
        assert_eq!(
            read_optional_answer_string(&answers_path, "_commit")
                .unwrap()
                .as_deref(),
            Some(old_ref.as_str())
        );
    }

    #[test]
    fn update_can_relink_working_tree_repo_to_committed_mode_without_template_override() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Working Tree Marker\n",
        )
        .unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::WorkingTree),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();
        fs::write(repo.join("AGENTS.md"), "# Repo Marker\n").unwrap();
        git(&repo, ["add", "AGENTS.md"]).unwrap();
        git(&repo, ["commit", "-m", "repo change"]).unwrap();

        git(
            template.path(),
            ["checkout", "--", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: Some(TemplateMode::Committed),
            recopy: false,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(!root_guide.contains("Working Tree Marker"));
        assert!(root_guide.contains("Repo Marker"));

        let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
        assert!(answers.contains("_template_mode: committed"));
        assert!(
            answers.contains(
                &absolute_path(template.path())
                    .unwrap()
                    .display()
                    .to_string()
            )
        );
        assert!(!answers.contains(TEMPLATE_CACHE_RELATIVE_PATH));
    }

    #[test]
    fn update_committed_mode_with_vcs_ref_only_updates_metadata() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let template = materialize_template_git_worktree();
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Older Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "older template"]).unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Newer Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "newer template"]).unwrap();
        let new_ref = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy: false,
            vcs_ref: Some(new_ref.clone()),
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Newer Marker"));
        assert!(!root_guide.contains("Older Marker"));

        let answers_path = repo.join(".jig.yml");
        assert_eq!(
            read_optional_answer_string(&answers_path, "_commit")
                .unwrap()
                .as_deref(),
            Some(new_ref.as_str())
        );
    }

    #[test]
    fn init_rejects_vcs_ref_for_non_git_local_template() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("repo");
        let template = materialize_template_worktree();

        fs::remove_dir_all(template.path().join(".git")).ok();

        let error = run_init(InitOpts {
            path: destination,
            template: template.path().display().to_string(),
            template_mode: None,
            vcs_ref: Some("main".into()),
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string();

        assert!(
            error
                .contains("--vcs-ref only applies to remote templates or local git template paths")
        );
    }

    #[test]
    fn update_committed_mode_keeps_normalized_remote_source_when_commit_is_reachable() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let remote = temp.path().join("template-remote.git");
        let template = materialize_template_git_worktree();
        let remote_url = format!("file://{}", remote.display());
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
        git(
            template.path(),
            [
                "clone",
                "--bare",
                &template.path().display().to_string(),
                &remote.display().to_string(),
            ],
        )
        .unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        let answers_path = repo.join(".jig.yml");
        let mut answers = read_answers_yaml(&answers_path).unwrap();
        answers.insert(
            YamlValue::String("_src_path".into()),
            YamlValue::String(remote_url.clone()),
        );
        write_answers_yaml(&answers_path, &answers).unwrap();
        git(&repo, ["add", ".jig.yml"]).unwrap();
        git(&repo, ["commit", "-m", "normalize source"]).unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Remote Source Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "template update"]).unwrap();
        git(
            template.path(),
            ["push", &remote_url, "HEAD:refs/heads/main"],
        )
        .unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: Some(template.path().display().to_string()),
            template_mode: Some(TemplateMode::Committed),
            recopy: false,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Remote Source Marker"));

        let answers = fs::read_to_string(&answers_path).unwrap();
        assert!(answers.contains(&remote_url));
        assert!(!answers.contains(&format!("_src_path: '{}'", template.path().display())));
    }

    #[test]
    fn update_committed_mode_accepts_explicit_normalized_remote_template_source() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let remote = temp.path().join("template-remote.git");
        let template = materialize_template_git_worktree();
        let remote_url = format!("file://{}", remote.display());
        fs::create_dir_all(repo.join("crates/api")).unwrap();
        fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
        git(
            template.path(),
            [
                "clone",
                "--bare",
                &template.path().display().to_string(),
                &remote.display().to_string(),
            ],
        )
        .unwrap();

        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: template.path().display().to_string(),
            template_mode: Some(TemplateMode::Committed),
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        init_git_repo_for_test(&repo);
        git(&repo, ["add", "."]).unwrap();
        git(&repo, ["commit", "-m", "adopt"]).unwrap();

        let answers_path = repo.join(".jig.yml");
        let mut answers = read_answers_yaml(&answers_path).unwrap();
        answers.insert(
            YamlValue::String("_src_path".into()),
            YamlValue::String(remote_url.clone()),
        );
        write_answers_yaml(&answers_path, &answers).unwrap();
        git(&repo, ["add", ".jig.yml"]).unwrap();
        git(&repo, ["commit", "-m", "normalize source"]).unwrap();

        fs::write(
            template.path().join("templates/project/AGENTS.md.jinja"),
            "# Explicit Remote Marker\n",
        )
        .unwrap();
        git(
            template.path(),
            ["add", "templates/project/AGENTS.md.jinja"],
        )
        .unwrap();
        git(template.path(), ["commit", "-m", "template update"]).unwrap();
        let new_commit = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();
        git(
            template.path(),
            ["push", &remote_url, "HEAD:refs/heads/main"],
        )
        .unwrap();

        run_update(UpdateOpts {
            path: repo.clone(),
            template: Some(remote_url.clone()),
            template_mode: None,
            recopy: false,
            vcs_ref: Some("main".into()),
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
        assert!(root_guide.contains("Explicit Remote Marker"));
        let expected_local_path = absolute_path(template.path())
            .unwrap()
            .display()
            .to_string();
        let answers = read_answers_yaml(&answers_path).unwrap();
        assert_eq!(
            answers
                .get(YamlValue::String("_src_path".into()))
                .and_then(YamlValue::as_str),
            Some(remote_url.as_str())
        );
        assert_eq!(
            answers
                .get(YamlValue::String("_commit".into()))
                .and_then(YamlValue::as_str),
            Some(new_commit.as_str())
        );
        assert_eq!(
            answers
                .get(YamlValue::String(TEMPLATE_MODE_KEY.into()))
                .and_then(YamlValue::as_str),
            Some(TemplateMode::Committed.as_str())
        );
        assert_eq!(
            answers
                .get(YamlValue::String(TEMPLATE_LOCAL_PATH_KEY.into()))
                .and_then(YamlValue::as_str),
            Some(expected_local_path.as_str())
        );
    }

    #[test]
    fn final_update_template_state_preserves_remote_source_when_probe_is_unknown() {
        let _guard = lock_env();
        let temp = tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        let git_path = bin_dir.join("git-stub.sh");
        fs::write(
            &git_path,
            "#!/bin/sh\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"--bare\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"fetch\" ]; then\n  printf 'transient fetch failure\\n' >&2\n  exit 1\nfi\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        unsafe {
            env::set_var(GIT_BIN_ENV, &git_path);
        }

        let stored = StoredTemplateState {
            src_path: "https://example.com/template.git".into(),
            default_branch: Some("main".into()),
            template_mode: Some(TemplateMode::Committed),
            template_local_path: Some("/tmp/template".into()),
        };
        let prepared = PreparedTemplateSource {
            copier_template: "/tmp/template".into(),
            vcs_ref: Some("deadbeef".into()),
            private_answers: PrivateAnswerOverrides {
                template_mode: Some(TemplateMode::Committed),
                template_local_path: Some("/tmp/template".into()),
            },
        };

        let final_template = final_update_template_state(&stored, &prepared);

        unsafe {
            env::remove_var(GIT_BIN_ENV);
        }

        assert_eq!(final_template.copier_template, stored.src_path);
        assert_eq!(final_template.vcs_ref, prepared.vcs_ref);
    }
}
