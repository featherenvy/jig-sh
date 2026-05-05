use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value as YamlValue};

use super::copier::{CopySpecOptions, build_copy_spec, run_copier};
use super::sync::{StagedRender, rendered_conflicts, stage_render};
use super::template_source::{
    PreparedTemplateSource, PrivateAnswerOverrides, rewrite_private_template_answers,
};
use super::{
    ANSWERS_FILE, AnswerOpts, TEMPLATE_LOCAL_PATH_KEY, TEMPLATE_MODE_KEY, TemplateMode,
    read_optional_answer_string,
};

static UNIQUE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) struct BootstrapCopyRequest<'a> {
    pub(super) destination: &'a Path,
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a AnswerOpts,
    pub(super) force: bool,
    pub(super) defaults: bool,
    pub(super) no_input: bool,
    pub(super) seed_repo_path: Option<&'a Path>,
}

pub(super) struct BootstrapCopyResult {
    pub(super) default_branch: Option<String>,
}

pub(super) fn render_and_copy_bootstrap_template(
    request: BootstrapCopyRequest<'_>,
) -> Result<BootstrapCopyResult> {
    let non_interactive = request.defaults || request.no_input;
    let interactive = !non_interactive;
    let seed_answers = SeedAnswersFile::write(request.answers, &request.template.private_answers)?;
    let staged = stage_render(
        &request.template.copier_template,
        request.template.vcs_ref.as_deref(),
        seed_answers.as_ref().map(|file| file.path()),
        request.seed_repo_path,
        non_interactive,
        interactive,
    )?;

    check_adopt_conflicts(&request, &staged)?;

    run_copier(
        build_copy_spec(
            &request.template.copier_template,
            request.destination,
            CopySpecOptions {
                answers_data_path: Some(&staged.answers_path),
                vcs_ref: staged.resolved_vcs_ref.as_deref(),
                force: request.force,
                use_defaults: true,
                ..CopySpecOptions::default()
            },
        ),
        None,
        false,
    )?;
    let rendered_template = request
        .template
        .with_vcs_ref(staged.resolved_vcs_ref.clone());
    rewrite_private_template_answers(&request.destination.join(ANSWERS_FILE), &rendered_template)?;

    Ok(BootstrapCopyResult {
        default_branch: read_optional_answer_string(&staged.answers_path, "default_branch")?,
    })
}

fn check_adopt_conflicts(request: &BootstrapCopyRequest<'_>, staged: &StagedRender) -> Result<()> {
    if request.seed_repo_path.is_none() || request.force {
        return Ok(());
    }

    let conflicts = rendered_conflicts(
        &staged.destination,
        &staged.answers_path,
        request.destination,
    )?;
    if conflicts.is_empty() {
        return Ok(());
    }

    bail!(
        "Adopt would overwrite template-managed paths. Re-run with --force or clear these paths first:\n{}",
        conflicts.join("\n")
    );
}

struct SeedAnswersFile {
    path: PathBuf,
}

impl SeedAnswersFile {
    fn write(opts: &AnswerOpts, private_answers: &PrivateAnswerOverrides) -> Result<Option<Self>> {
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

impl Drop for SeedAnswersFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn seed_answers_yaml(
    opts: &AnswerOpts,
    private_answers: &PrivateAnswerOverrides,
) -> YamlValue {
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
    let sequence = UNIQUE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("jig-{prefix}-{nanos}-{sequence}")
}
