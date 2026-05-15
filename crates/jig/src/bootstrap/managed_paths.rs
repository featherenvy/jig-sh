use std::path::{Path, PathBuf};

use super::answers::RenderAnswers;

pub(super) const ROOT_AGENTS_PATH: &str = "AGENTS.md";

const REMOVED_MANAGED_PATHS: &[&str] = &["scripts/normalize-template-source.sh"];

const SQLX_PRUNED_TASK_PATHS: &[&str] = &[
    "scripts/add-migration.sh",
    "scripts/check-migration-immutability.sh",
    "scripts/check-schema-dump.sh",
    "scripts/check-sqlx-unchecked-non-test.sh",
    "scripts/generate-sqlx-unchecked-queries-todo.sh",
];

pub(super) fn removed_managed_paths() -> impl Iterator<Item = PathBuf> {
    REMOVED_MANAGED_PATHS.iter().map(PathBuf::from)
}

pub(super) fn sqlx_pruned_task_paths() -> impl Iterator<Item = PathBuf> {
    SQLX_PRUNED_TASK_PATHS.iter().map(PathBuf::from)
}

pub(super) fn should_prune_rendered_path(relative: &Path, answers: &RenderAnswers) -> bool {
    !answers.sqlx_enabled() && is_sqlx_pruned_task_path(relative)
}

pub(super) fn should_omit_unmanaged_rendered_path(
    relative: &Path,
    answers: &RenderAnswers,
) -> bool {
    !answers.makefile_enabled() && is_makefile_path(relative)
}

pub(super) fn is_sqlx_pruned_task_path(relative: &Path) -> bool {
    SQLX_PRUNED_TASK_PATHS
        .iter()
        .any(|path| relative == Path::new(path))
}

fn is_makefile_path(relative: &Path) -> bool {
    relative == Path::new("Makefile")
}

pub(super) fn is_root_agents_path(relative: &Path) -> bool {
    relative == Path::new(ROOT_AGENTS_PATH)
}

pub(super) fn is_executable_script(relative: &Path) -> bool {
    relative.starts_with("scripts")
        && (relative.extension().and_then(|ext| ext.to_str()) == Some("sh")
            || relative.file_name().and_then(|name| name.to_str()) == Some("jig"))
}
