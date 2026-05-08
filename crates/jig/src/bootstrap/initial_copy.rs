use std::path::Path;

use anyhow::Result;
#[cfg(test)]
use serde_yaml::{Mapping, Value as YamlValue};

use super::AnswerOpts;
use super::answers::RenderAnswers;
use super::renderer::stage_render;
use super::sync::{ApplyRenderOptions, apply_staged_render};
use super::template_source::PreparedTemplateSource;
#[cfg(test)]
use super::template_source::PrivateAnswerOverrides;
#[cfg(test)]
use super::{TEMPLATE_LOCAL_PATH_KEY, TEMPLATE_MODE_KEY};

pub(super) struct BootstrapCopyRequest<'a> {
    pub(super) destination: &'a Path,
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a AnswerOpts,
    pub(super) force: bool,
    pub(super) seed_repo_path: Option<&'a Path>,
}

pub(super) struct BootstrapCopyResult {
    pub(super) default_branch: Option<String>,
}

pub(super) fn render_and_copy_bootstrap_template(
    request: BootstrapCopyRequest<'_>,
) -> Result<BootstrapCopyResult> {
    let answers = RenderAnswers::from_opts(request.answers, request.destination)?;
    let staged = stage_render(request.template, &answers, request.seed_repo_path)?;

    apply_staged_render(
        &staged,
        request.destination,
        ApplyRenderOptions {
            force: request.force,
            allow_answers_overwrite: false,
            conflict_message: "Adopt would overwrite template-managed paths. Re-run with --force or clear these paths first:",
        },
    )?;

    Ok(BootstrapCopyResult {
        default_branch: Some(answers.default_branch().to_string()),
    })
}

#[cfg(test)]
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
        private_answers.template_mode_answer(),
    );
    insert_string(
        &mut mapping,
        TEMPLATE_LOCAL_PATH_KEY,
        private_answers.template_local_path_answer(),
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

#[cfg(test)]
fn insert_string(mapping: &mut Mapping, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        mapping.insert(
            YamlValue::String(key.to_string()),
            YamlValue::String(value.to_string()),
        );
    }
}

#[cfg(test)]
fn insert_bool(mapping: &mut Mapping, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        mapping.insert(YamlValue::String(key.to_string()), YamlValue::Bool(value));
    }
}
