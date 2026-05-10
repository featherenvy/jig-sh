use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::ANSWERS_FILE;
use super::file_copy::{
    copy_file_or_symlink_with_permissions, path_exists, prepare_copy_destination_and_read_metadata,
};
use super::renderer::ROOT_AGENTS_PATH;
use super::staged_render::StagedRender;
#[cfg(test)]
use super::{ALWAYS_TASK_MUTATED_PATHS, SQLX_PRUNED_TASK_PATHS, read_optional_answer_bool};

pub(super) struct ApplyRenderOptions<'a> {
    pub(super) force: bool,
    pub(super) allow_answers_overwrite: bool,
    pub(super) conflict_message: &'a str,
}

pub(super) fn apply_staged_render(
    staged: &StagedRender,
    destination: &Path,
    options: ApplyRenderOptions<'_>,
) -> Result<()> {
    if !options.force {
        let conflicts =
            staged_render_conflicts(staged, destination, options.allow_answers_overwrite)?;
        if !conflicts.is_empty() {
            bail!("{}\n{}", options.conflict_message, conflicts.join("\n"));
        }
    }

    for relative in &staged.managed_paths {
        let rendered_path = staged.destination.join(relative);
        let destination_path = destination.join(relative);
        if path_exists(&rendered_path) {
            copy_rendered_path(&rendered_path, &destination_path)?;
        } else if path_exists(&destination_path) {
            remove_destination_path(&destination_path)?;
        }
    }
    Ok(())
}

fn staged_render_conflicts(
    staged: &StagedRender,
    destination: &Path,
    allow_answers_overwrite: bool,
) -> Result<Vec<String>> {
    let mut conflicts = BTreeSet::new();
    for relative in &staged.managed_paths {
        if allow_answers_overwrite && relative == Path::new(ANSWERS_FILE) {
            continue;
        }

        let rendered_path = staged.destination.join(relative);
        let destination_path = destination.join(relative);
        if let Some(blocking_ancestor) = blocking_ancestor(destination, &destination_path) {
            conflicts.insert(relative_to_string(destination, &blocking_ancestor));
            continue;
        }

        if path_exists(&rendered_path) {
            if is_root_agents_path(relative) {
                if destination_is_regular_file(&destination_path)? {
                    continue;
                }
                if path_exists(&destination_path) {
                    conflicts.insert(relative.display().to_string());
                    continue;
                }
            }
            if path_exists(&destination_path) && !files_match(&rendered_path, &destination_path)? {
                conflicts.insert(relative.display().to_string());
            }
        } else if path_exists(&destination_path) {
            conflicts.insert(relative.display().to_string());
        }
    }
    Ok(conflicts.into_iter().collect())
}

fn is_root_agents_path(relative: &Path) -> bool {
    relative == Path::new(ROOT_AGENTS_PATH)
}

fn destination_is_regular_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn blocking_ancestor(root: &Path, path: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?;
    while current != root {
        if current.exists() && !current.is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
    None
}

fn relative_to_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn copy_rendered_path(rendered_path: &Path, destination_path: &Path) -> Result<()> {
    let metadata = prepare_copy_destination_and_read_metadata(rendered_path, destination_path)?;
    if path_exists(destination_path) && !metadata.is_dir() {
        remove_destination_path(destination_path)?;
    }
    copy_file_or_symlink_with_permissions(rendered_path, destination_path, &metadata)
}

fn remove_destination_path(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).with_context(|| format!("Failed to remove {}", path.display()))
    } else {
        fs::remove_file(path).with_context(|| format!("Failed to remove {}", path.display()))
    }
}

#[cfg(test)]
pub(super) fn rendered_conflicts(
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

#[cfg(test)]
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

fn files_match(rendered_path: &Path, destination_path: &Path) -> Result<bool> {
    let rendered_meta = fs::symlink_metadata(rendered_path)
        .with_context(|| format!("Failed to stat {}", rendered_path.display()))?;
    let destination_meta = fs::symlink_metadata(destination_path)
        .with_context(|| format!("Failed to stat {}", destination_path.display()))?;

    if either_is_symlink(&rendered_meta, &destination_meta) {
        return symlinks_match(
            rendered_path,
            &rendered_meta,
            destination_path,
            &destination_meta,
        );
    }

    if !both_are_files(&rendered_meta, &destination_meta) {
        return Ok(false);
    }
    if !executable_bits_match(&rendered_meta, &destination_meta) {
        return Ok(false);
    }

    file_contents_match(rendered_path, destination_path)
}

fn either_is_symlink(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    rendered_meta.file_type().is_symlink() || destination_meta.file_type().is_symlink()
}

fn symlinks_match(
    rendered_path: &Path,
    rendered_meta: &fs::Metadata,
    destination_path: &Path,
    destination_meta: &fs::Metadata,
) -> Result<bool> {
    if !(rendered_meta.file_type().is_symlink() && destination_meta.file_type().is_symlink()) {
        return Ok(false);
    }

    let rendered_target = fs::read_link(rendered_path)
        .with_context(|| format!("Failed to read symlink {}", rendered_path.display()))?;
    let destination_target = fs::read_link(destination_path)
        .with_context(|| format!("Failed to read symlink {}", destination_path.display()))?;
    Ok(rendered_target == destination_target)
}

fn both_are_files(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    rendered_meta.is_file() && destination_meta.is_file()
}

fn file_contents_match(rendered_path: &Path, destination_path: &Path) -> Result<bool> {
    let rendered = fs::read(rendered_path)
        .with_context(|| format!("Failed to read {}", rendered_path.display()))?;
    let destination = fs::read(destination_path)
        .with_context(|| format!("Failed to read {}", destination_path.display()))?;
    Ok(rendered == destination)
}

#[cfg(unix)]
fn executable_bits_match(rendered_meta: &fs::Metadata, destination_meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    (rendered_meta.permissions().mode() & 0o111) == (destination_meta.permissions().mode() & 0o111)
}

#[cfg(not(unix))]
fn executable_bits_match(_rendered_meta: &fs::Metadata, _destination_meta: &fs::Metadata) -> bool {
    true
}
