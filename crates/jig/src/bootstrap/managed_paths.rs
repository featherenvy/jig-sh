use std::path::{Path, PathBuf};

use super::answers::RenderAnswers;

pub(super) const ROOT_AGENTS_PATH: &str = "AGENTS.md";
pub(super) const ROOT_AGENTS_BLOCK_BEGIN: &str = "<!-- BEGIN JIG MANAGED BLOCK -->";
pub(super) const ROOT_AGENTS_BLOCK_END: &str = "<!-- END JIG MANAGED BLOCK -->";
pub(super) const ROOT_GITATTRIBUTES_PATH: &str = ".gitattributes";
pub(super) const ROOT_GITATTRIBUTES_BLOCK_BEGIN: &str = "# BEGIN JIG MANAGED BLOCK";
pub(super) const ROOT_GITATTRIBUTES_BLOCK_END: &str = "# END JIG MANAGED BLOCK";

#[derive(Clone, Copy, Debug)]
pub(super) struct ManagedBlockSpec {
    pub(super) path: &'static str,
    pub(super) begin: &'static str,
    pub(super) end: &'static str,
    pub(super) progress_label: &'static str,
}

const RETIRED_MANAGED_PATHS: &[&str] = &[
    "scripts/add-migration.sh",
    "scripts/check-agent-guides.sh",
    "scripts/check-agent-map.sh",
    "scripts/check-jig-contract.sh",
    "scripts/check-migration-immutability.sh",
    "scripts/check-no-mod-rs.sh",
    "scripts/check-rust-file-loc.sh",
    "scripts/check-schema-dump.sh",
    "scripts/check-sqlx-unchecked-non-test.sh",
    "scripts/enforce-coverage.js",
    "scripts/generate-agent-map.sh",
    "scripts/generate-sqlx-unchecked-queries-todo.sh",
    "scripts/jig-toml.sh",
    "scripts/normalize-template-source.sh",
];

const WEB_MANAGED_PATHS: &[&str] = &[
    ".github/workflows/webapp-checks.yml",
    "scripts/check-webapp-scripts.mjs",
    "scripts/check-webapps.sh",
    "scripts/enforce-coverage.cjs",
];

pub(super) fn retired_managed_paths(answers: &RenderAnswers) -> impl Iterator<Item = PathBuf> + '_ {
    RETIRED_MANAGED_PATHS
        .iter()
        .copied()
        .chain(web_managed_paths_retired_for_answers(answers))
        .map(PathBuf::from)
}

pub(super) fn is_retired_managed_path(relative: &Path, answers: &RenderAnswers) -> bool {
    retired_managed_paths(answers).any(|retired| relative == retired)
}

pub(super) fn should_omit_unmanaged_rendered_path(
    relative: &Path,
    answers: &RenderAnswers,
) -> bool {
    relative == Path::new("Makefile")
        || (answers.frontend_apps().is_empty() && is_web_managed_path(relative))
}

fn web_managed_paths_retired_for_answers(answers: &RenderAnswers) -> impl Iterator<Item = &str> {
    let paths: &[&str] = if answers.frontend_apps().is_empty() {
        WEB_MANAGED_PATHS
    } else {
        &[]
    };
    paths.iter().copied()
}

fn is_web_managed_path(relative: &Path) -> bool {
    WEB_MANAGED_PATHS
        .iter()
        .any(|web_path| relative == Path::new(web_path))
}

pub(super) fn managed_block_spec(relative: &Path) -> Option<ManagedBlockSpec> {
    if relative == Path::new(ROOT_AGENTS_PATH) {
        return Some(ManagedBlockSpec {
            path: ROOT_AGENTS_PATH,
            begin: ROOT_AGENTS_BLOCK_BEGIN,
            end: ROOT_AGENTS_BLOCK_END,
            progress_label: "root guide",
        });
    }
    if relative == Path::new(ROOT_GITATTRIBUTES_PATH) {
        return Some(ManagedBlockSpec {
            path: ROOT_GITATTRIBUTES_PATH,
            begin: ROOT_GITATTRIBUTES_BLOCK_BEGIN,
            end: ROOT_GITATTRIBUTES_BLOCK_END,
            progress_label: "git attributes",
        });
    }
    None
}

pub(super) fn is_executable_script(relative: &Path) -> bool {
    relative.starts_with("scripts")
        && (relative.extension().and_then(|ext| ext.to_str()) == Some("sh")
            || relative.file_name().and_then(|name| name.to_str()) == Some("jig"))
}
