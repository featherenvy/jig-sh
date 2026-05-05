use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;

use super::copier::{CopySpecOptions, build_copy_spec, run_copier};
use super::{
    ALWAYS_TASK_MUTATED_PATHS, ANSWERS_FILE, SQLX_PRUNED_TASK_PATHS, read_optional_answer_bool,
    read_optional_answer_string,
};

pub(super) struct StagedRender {
    pub(super) _root: TempDir,
    pub(super) destination: PathBuf,
    pub(super) answers_path: PathBuf,
    pub(super) resolved_vcs_ref: Option<String>,
}

pub(super) fn stage_render(
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
            CopySpecOptions {
                answers_data_path,
                vcs_ref,
                overwrite: seed_repo_path.is_some(),
                use_defaults: non_interactive_defaults,
                skip_tasks: true,
                ..CopySpecOptions::default()
            },
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

pub(super) fn seed_preview_workspace(source_root: &Path, destination_root: &Path) -> Result<()> {
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
    let metadata = prepare_copy_destination_and_read_metadata(source_path, destination_path)?;
    copy_file_or_symlink_with_permissions(source_path, destination_path, &metadata)
}

fn prepare_copy_destination_and_read_metadata(
    source_path: &Path,
    destination_path: &Path,
) -> Result<fs::Metadata> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    fs::symlink_metadata(source_path)
        .with_context(|| format!("Failed to stat {}", source_path.display()))
}

fn copy_file_or_symlink_with_permissions(
    source_path: &Path,
    destination_path: &Path,
    metadata: &fs::Metadata,
) -> Result<()> {
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
pub(super) fn create_symlink(target: &Path, link_path: &Path) -> Result<()> {
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
pub(super) fn create_symlink(target: &Path, link_path: &Path) -> Result<()> {
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
