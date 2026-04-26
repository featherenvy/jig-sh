use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value as YamlValue};
use tempfile::{NamedTempFile, TempDir};

use super::git::{
    ensure_clean_git_work_tree, ensure_git_repo, git, git_command, git_stdout, is_git_work_tree,
};
use super::sync::copy_working_tree_snapshot;
use super::{
    ANSWERS_FILE, TEMPLATE_CACHE_RELATIVE_PATH, TEMPLATE_LOCAL_PATH_KEY, TEMPLATE_MODE_KEY,
    TemplateMode, UpdateOpts, absolute_path, read_answers_yaml, read_optional_answer_string,
    set_optional_yaml_string, write_answers_yaml,
};

#[derive(Debug, Clone, Default)]
pub(super) struct PrivateAnswerOverrides {
    pub(super) template_mode: Option<TemplateMode>,
    pub(super) template_local_path: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedTemplateSource {
    pub(super) copier_template: String,
    pub(super) vcs_ref: Option<String>,
    pub(super) private_answers: PrivateAnswerOverrides,
}

impl PreparedTemplateSource {
    pub(super) fn with_vcs_ref(&self, vcs_ref: Option<String>) -> Self {
        Self {
            copier_template: self.copier_template.clone(),
            vcs_ref,
            private_answers: self.private_answers.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct StoredTemplateState {
    pub(super) src_path: String,
    pub(super) template_mode: Option<TemplateMode>,
    pub(super) template_local_path: Option<String>,
}

pub(super) struct ResolvedUpdateTemplateSource<'a> {
    pub(super) template: &'a str,
    pub(super) template_mode: Option<TemplateMode>,
}

impl<'a> ResolvedUpdateTemplateSource<'a> {
    fn new(template: &'a str, template_mode: Option<TemplateMode>) -> Self {
        Self {
            template,
            template_mode,
        }
    }
}

pub(super) struct UpdateAnswersFile {
    pub(super) copier_arg: PathBuf,
    pub(super) path: PathBuf,
    pub(super) exclude_destination_answers: bool,
    pub(super) restore_on_error: Option<Mapping>,
    pub(super) _temporary_file: Option<NamedTempFile>,
}

impl UpdateAnswersFile {
    fn existing(answers_path: &Path) -> Self {
        Self {
            copier_arg: PathBuf::from(ANSWERS_FILE),
            path: answers_path.to_path_buf(),
            exclude_destination_answers: false,
            restore_on_error: None,
            _temporary_file: None,
        }
    }

    fn in_place(answers_path: &Path, restore_on_error: Mapping) -> Self {
        Self {
            copier_arg: PathBuf::from(ANSWERS_FILE),
            path: answers_path.to_path_buf(),
            exclude_destination_answers: false,
            restore_on_error: Some(restore_on_error),
            _temporary_file: None,
        }
    }
}

pub(super) fn prepare_template_source(
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
            if vcs_ref.is_some() {
                bail!(
                    "--vcs-ref is not supported with --template-mode working-tree: {}",
                    local_template.display()
                );
            }
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

pub(super) fn prepare_snapshot_update_source(
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

pub(super) fn rewrite_private_template_answers(
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

pub(super) fn read_stored_template_state(answers_path: &Path) -> Result<StoredTemplateState> {
    Ok(StoredTemplateState {
        src_path: read_optional_answer_string(answers_path, "_src_path")?.unwrap_or_default(),
        template_mode: read_optional_answer_string(answers_path, TEMPLATE_MODE_KEY)?
            .as_deref()
            .map(parse_template_mode_answer)
            .transpose()?,
        template_local_path: read_optional_answer_string(answers_path, TEMPLATE_LOCAL_PATH_KEY)?,
    })
}

pub(super) fn prepare_update_answers_file(
    destination: &Path,
    answers_path: &Path,
    update_template: Option<&PreparedTemplateSource>,
) -> Result<UpdateAnswersFile> {
    let Some(template) = update_template else {
        return Ok(UpdateAnswersFile::existing(answers_path));
    };

    let original_answers = read_answers_yaml(answers_path)?;
    if copier_update_source_matches(&original_answers, template) {
        return Ok(UpdateAnswersFile::existing(answers_path));
    }

    let mut update_answers = original_answers.clone();
    apply_copier_update_source_answer(&mut update_answers, template);

    if let Some(git_dir) = destination_git_dir(destination)? {
        let temporary_file = NamedTempFile::new_in(&git_dir)
            .with_context(|| format!("Failed to create temporary file in {}", git_dir.display()))?;
        write_answers_yaml(temporary_file.path(), &update_answers)?;
        return Ok(UpdateAnswersFile {
            copier_arg: copier_answers_file_argument(destination, temporary_file.path()),
            path: temporary_file.path().to_path_buf(),
            exclude_destination_answers: true,
            restore_on_error: Some(original_answers),
            _temporary_file: Some(temporary_file),
        });
    }

    write_answers_yaml(answers_path, &update_answers)?;
    Ok(UpdateAnswersFile::in_place(answers_path, original_answers))
}

fn destination_git_dir(destination: &Path) -> Result<Option<PathBuf>> {
    if !is_git_work_tree(destination) {
        return Ok(None);
    }

    let git_dir = PathBuf::from(git_stdout(destination, ["rev-parse", "--git-dir"])?);
    let git_dir = if git_dir.is_absolute() {
        git_dir
    } else {
        destination.join(git_dir)
    };
    if git_dir.is_dir() {
        Ok(Some(git_dir))
    } else {
        Ok(None)
    }
}

fn copier_answers_file_argument(destination: &Path, answers_path: &Path) -> PathBuf {
    answers_path
        .strip_prefix(destination)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| answers_path.to_path_buf())
}

fn apply_copier_update_source_answer(answers: &mut Mapping, template: &PreparedTemplateSource) {
    answers.insert(
        YamlValue::String("_src_path".into()),
        YamlValue::String(template.copier_template.clone()),
    );
}

fn copier_update_source_matches(answers: &Mapping, template: &PreparedTemplateSource) -> bool {
    answers
        .get(YamlValue::String("_src_path".into()))
        .and_then(YamlValue::as_str)
        == Some(template.copier_template.as_str())
}

pub(super) fn write_final_template_answers(
    source_answers_path: &Path,
    destination_answers_path: &Path,
    template: &PreparedTemplateSource,
) -> Result<()> {
    let mut answers = read_answers_yaml(source_answers_path)?;
    apply_private_template_answers(&mut answers, template);
    write_answers_yaml(destination_answers_path, &answers)
}

pub(super) fn required_prepared_vcs_ref(prepared: &PreparedTemplateSource) -> Result<&str> {
    prepared
        .vcs_ref
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Missing committed template ref for relink update"))
}

pub(super) fn resolve_update_template_source<'a>(
    opts: &'a UpdateOpts,
    stored: &'a StoredTemplateState,
    answers_path: &Path,
) -> Result<ResolvedUpdateTemplateSource<'a>> {
    if let Some(template) = opts.template.as_deref() {
        return Ok(ResolvedUpdateTemplateSource::new(
            template,
            inherited_update_template_mode(template, opts.template_mode, stored),
        ));
    }

    if opts.template_mode == Some(TemplateMode::Committed) {
        if stored.template_mode == Some(TemplateMode::WorkingTree) {
            return Ok(ResolvedUpdateTemplateSource::new(
                required_stored_template_local_path(stored, answers_path)?,
                Some(TemplateMode::Committed),
            ));
        }

        if let Some(template) = stored.template_local_path.as_deref() {
            return Ok(ResolvedUpdateTemplateSource::new(
                template,
                Some(TemplateMode::Committed),
            ));
        }

        if is_remote_template_source(&stored.src_path) {
            return Ok(ResolvedUpdateTemplateSource::new(&stored.src_path, None));
        }
    }

    Ok(ResolvedUpdateTemplateSource::new(
        &stored.src_path,
        inherited_update_template_mode(&stored.src_path, opts.template_mode, stored),
    ))
}

fn inherited_update_template_mode(
    template: &str,
    requested_mode: Option<TemplateMode>,
    stored: &StoredTemplateState,
) -> Option<TemplateMode> {
    if requested_mode.is_some() || is_remote_template_source(template) {
        requested_mode
    } else {
        stored.template_mode
    }
}

fn parse_template_mode_answer(value: &str) -> Result<TemplateMode> {
    match value {
        "committed" => Ok(TemplateMode::Committed),
        "working-tree" => Ok(TemplateMode::WorkingTree),
        other => bail!("Unsupported template mode '{other}' in {}", ANSWERS_FILE),
    }
}

pub(super) fn stored_template_local_path(stored: &StoredTemplateState) -> &str {
    stored
        .template_local_path
        .as_deref()
        .unwrap_or(&stored.src_path)
}

pub(super) fn required_stored_template_local_path<'a>(
    stored: &'a StoredTemplateState,
    answers_path: &Path,
) -> Result<&'a str> {
    stored.template_local_path.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Missing {TEMPLATE_LOCAL_PATH_KEY} in {} for working-tree template mode",
            answers_path.display()
        )
    })
}

pub(super) fn template_identities_match(
    stored: &StoredTemplateState,
    prepared: &PreparedTemplateSource,
) -> bool {
    let uses_working_tree_identity = stored.template_mode == Some(TemplateMode::WorkingTree)
        || prepared.private_answers.template_mode == Some(TemplateMode::WorkingTree);
    if uses_working_tree_identity
        && let (Some(stored_local_path), Some(prepared_local_path)) = (
            stored.template_local_path.as_deref(),
            prepared.private_answers.template_local_path.as_deref(),
        )
    {
        return stored_local_path == prepared_local_path;
    }

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

pub(super) fn final_update_template_state(
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

    final_template
}

pub(super) fn refresh_postwrite_template_metadata(
    answers_path: &Path,
    template: &mut PreparedTemplateSource,
) -> Result<()> {
    if is_remote_template_source(&template.copier_template)
        && !template.vcs_ref.as_deref().is_some_and(is_full_git_commit)
    {
        template.vcs_ref = read_optional_answer_string(answers_path, "_commit")?;
    }
    Ok(())
}

fn is_full_git_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
        git(
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

fn is_remote_template_source(template: &str) -> bool {
    template.contains("://") || template.starts_with("git@") && template.contains(':')
}
