use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use minijinja::{Environment, UndefinedBehavior, syntax::SyntaxConfig};
use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;

use super::answers::RenderAnswers;
use super::preview_seed::seed_preview_workspace;
use super::staged_render::StagedRender;
use super::template_source::PreparedTemplateSource;
use super::{ANSWERS_FILE, SQLX_PRUNED_TASK_PATHS};

const TEMPLATE_SUBDIRECTORY: &str = "templates/project";
const TEMPLATE_SUFFIX: &str = ".jinja";
const REMOVED_MANAGED_PATHS: &[&str] = &["scripts/normalize-template-source.sh"];

pub(super) fn stage_render(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    seed_repo_path: Option<&Path>,
) -> Result<StagedRender> {
    let root = TempDir::new().context("Failed to create staging directory")?;
    let destination = root.path().join("render");
    if let Some(seed_repo_path) = seed_repo_path {
        seed_preview_workspace(seed_repo_path, &destination)?;
    }

    let mut managed_paths = render_template_files(template, answers, &destination)?;
    run_post_render_tasks(&destination)?;
    for relative in REMOVED_MANAGED_PATHS {
        managed_paths.insert(PathBuf::from(relative));
    }

    let answers_path = destination.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Staging render did not produce {} in {}",
            ANSWERS_FILE,
            destination.display()
        );
    }

    Ok(StagedRender {
        _root: root,
        destination,
        managed_paths,
    })
}

fn render_template_files(
    template: &PreparedTemplateSource,
    answers: &RenderAnswers,
    destination: &Path,
) -> Result<BTreeSet<PathBuf>> {
    let template_root = template.render_root().join(TEMPLATE_SUBDIRECTORY);
    if !template_root.is_dir() {
        bail!(
            "Template source does not contain {}: {}",
            TEMPLATE_SUBDIRECTORY,
            template.render_root().display()
        );
    }

    let context = render_context(template, answers)?;
    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()?,
    );
    environment.set_undefined_behavior(UndefinedBehavior::Strict);

    let mut managed_paths = BTreeSet::new();
    for template_path in collect_template_paths(&template_root)? {
        let relative_template = template_path
            .strip_prefix(&template_root)
            .with_context(|| {
                format!(
                    "{} is not under {}",
                    template_path.display(),
                    template_root.display()
                )
            })?;
        let relative = output_relative_path(relative_template)?;
        managed_paths.insert(relative.clone());
        if !answers.sqlx_enabled()
            && SQLX_PRUNED_TASK_PATHS
                .iter()
                .any(|path| relative == Path::new(path))
        {
            continue;
        }

        let source = fs::read_to_string(&template_path)
            .with_context(|| format!("Failed to read {}", template_path.display()))?;
        let rendered = environment
            .render_str(&source, &context)
            .with_context(|| format!("Failed to render {}", template_path.display()))?;
        write_rendered_file(destination, &relative, rendered.as_bytes())?;
    }

    for relative in SQLX_PRUNED_TASK_PATHS {
        managed_paths.insert(PathBuf::from(relative));
    }

    Ok(managed_paths)
}

fn render_context(template: &PreparedTemplateSource, answers: &RenderAnswers) -> Result<JsonValue> {
    let mut context = serde_json::to_value(answers)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    context.insert(
        "_jig".into(),
        json!({
            "commit": template.vcs_ref().unwrap_or_default(),
            "src_path": if answers.template_source_url().is_empty() {
                template.source().to_string()
            } else {
                answers.template_source_url().to_string()
            },
            "template_mode": template.template_mode_answer().unwrap_or(""),
            "template_local_path": template.template_local_path_answer().unwrap_or(""),
        }),
    );
    Ok(JsonValue::Object(context))
}

fn collect_template_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_template_paths_recursive(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_template_paths_recursive(current: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("Failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_template_paths_recursive(&path, paths)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(TEMPLATE_SUFFIX))
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn output_relative_path(relative_template: &Path) -> Result<PathBuf> {
    let file_name = relative_template
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid template path: {}", relative_template.display()))?;
    let output_name = file_name.strip_suffix(TEMPLATE_SUFFIX).ok_or_else(|| {
        anyhow::anyhow!(
            "Template path must end with {TEMPLATE_SUFFIX}: {}",
            relative_template.display()
        )
    })?;
    Ok(relative_template.with_file_name(output_name))
}

fn write_rendered_file(destination: &Path, relative: &Path, contents: &[u8]) -> Result<()> {
    let path = destination.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&path, contents).with_context(|| format!("Failed to write {}", path.display()))?;
    set_rendered_permissions(&path, relative)
}

fn run_post_render_tasks(destination: &Path) -> Result<()> {
    set_scripts_executable(destination)?;
    let output = Command::new("bash")
        .arg("scripts/generate-agent-map.sh")
        .current_dir(destination)
        .output()
        .context("Failed to start bash scripts/generate-agent-map.sh")?;
    if !output.status.success() {
        bail!(
            "scripts/generate-agent-map.sh failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(unix)]
fn set_rendered_permissions(path: &Path, relative: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if is_executable_script(relative) {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_rendered_permissions(_path: &Path, _relative: &Path) -> Result<()> {
    Ok(())
}

fn set_scripts_executable(destination: &Path) -> Result<()> {
    for relative in executable_script_paths(destination)? {
        set_rendered_permissions(&destination.join(&relative), &relative)?;
    }
    Ok(())
}

fn executable_script_paths(destination: &Path) -> Result<Vec<PathBuf>> {
    let scripts_dir = destination.join("scripts");
    if !scripts_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&scripts_dir)
        .with_context(|| format!("Failed to read {}", scripts_dir.display()))?
    {
        let entry = entry?;
        let relative = PathBuf::from("scripts").join(entry.file_name());
        if is_executable_script(&relative) {
            paths.push(relative);
        }
    }
    Ok(paths)
}

fn is_executable_script(relative: &Path) -> bool {
    relative.starts_with("scripts")
        && (relative.extension().and_then(|ext| ext.to_str()) == Some("sh")
            || relative.file_name().and_then(|name| name.to_str()) == Some("jig"))
}
