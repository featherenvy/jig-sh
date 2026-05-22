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

const REMOVED_MANAGED_PATHS: &[&str] = &[
    "scripts/add-migration.sh",
    "scripts/check-agent-guides.sh",
    "scripts/check-agent-map.sh",
    "scripts/check-jig-contract.sh",
    "scripts/check-migration-immutability.sh",
    "scripts/check-no-mod-rs.sh",
    "scripts/check-rust-file-loc.sh",
    "scripts/check-schema-dump.sh",
    "scripts/check-sqlx-unchecked-non-test.sh",
    "scripts/generate-agent-map.sh",
    "scripts/generate-sqlx-unchecked-queries-todo.sh",
    "scripts/jig-toml.sh",
    "scripts/normalize-template-source.sh",
];

pub(super) fn removed_managed_paths() -> impl Iterator<Item = PathBuf> {
    REMOVED_MANAGED_PATHS.iter().map(PathBuf::from)
}

pub(super) fn should_omit_unmanaged_rendered_path(
    relative: &Path,
    _answers: &RenderAnswers,
) -> bool {
    relative == Path::new("Makefile")
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
