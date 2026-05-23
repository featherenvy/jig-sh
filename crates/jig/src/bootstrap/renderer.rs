use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use minijinja::{Environment, UndefinedBehavior, syntax::SyntaxConfig};
use serde_json::{Value as JsonValue, json};
use tempfile::TempDir;

use super::ANSWERS_FILE;
use super::answers::RenderAnswers;
use super::embedded_templates::EMBEDDED_TEMPLATE_FILES;
use super::managed_paths;
use super::preview_seed::seed_preview_workspace;
use super::staged_render::StagedRender;
use super::template_source::{PreparedTemplateSource, TemplateRenderSource};
use crate::progress::CliProgress;

const TEMPLATE_SUBDIRECTORY: &str = "templates/project";
const TEMPLATE_SUFFIX: &str = ".jinja";

pub(super) struct RenderStageRequest<'a> {
    pub(super) template: &'a PreparedTemplateSource,
    pub(super) answers: &'a RenderAnswers,
    pub(super) seed_repo_path: Option<&'a Path>,
    pub(super) progress: CliProgress,
}

pub(super) fn stage_render(request: RenderStageRequest<'_>) -> Result<StagedRender> {
    let root = request
        .progress
        .log_blocked_on_err(TempDir::new().context("Failed to create staging directory"))?;
    let destination = root.path().join("render");
    if let Some(seed_repo_path) = request.seed_repo_path {
        request
            .progress
            .step("seed agent guides", "scan existing repo for AGENTS.md");
        request
            .progress
            .log_blocked_on_err(seed_preview_workspace(seed_repo_path, &destination))?;
    }

    request
        .progress
        .step("render templates", "managed files, scripts, and workflows");
    let mut managed_paths = request.progress.log_blocked_on_err(render_template_files(
        request.template,
        request.answers,
        &destination,
    ))?;
    request
        .progress
        .step("generate agent map", "native renderer");
    request
        .progress
        .log_blocked_on_err(run_post_render_tasks(&destination))?;
    merge_existing_managed_blocks(request.seed_repo_path, &destination, request.progress)?;
    managed_paths.extend(managed_paths::retired_managed_paths(request.answers));

    let answers_path = destination.join(ANSWERS_FILE);
    if !answers_path.exists() {
        request
            .progress
            .blocked(format!("staging render did not produce {}", ANSWERS_FILE));
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

    let mut render = TemplateRender {
        environment: &mut environment,
        context: &context,
        answers,
        destination,
        managed_paths: BTreeSet::new(),
    };
    match template.render_source() {
        TemplateRenderSource::Filesystem(render_root) => {
            let template_root = render_root.join(TEMPLATE_SUBDIRECTORY);
            if !template_root.is_dir() {
                bail!(
                    "Template source does not contain {}: {}",
                    TEMPLATE_SUBDIRECTORY,
                    render_root.display()
                );
            }
            for template_path in collect_template_paths(&template_root)? {
                let relative_template =
                    template_path
                        .strip_prefix(&template_root)
                        .with_context(|| {
                            format!(
                                "{} is not under {}",
                                template_path.display(),
                                template_root.display()
                            )
                        })?;
                let source = fs::read_to_string(&template_path)
                    .with_context(|| format!("Failed to read {}", template_path.display()))?;
                render.entry(
                    relative_template,
                    &template_path.display().to_string(),
                    &source,
                )?;
            }
        }
        TemplateRenderSource::Embedded => {
            for template_file in EMBEDDED_TEMPLATE_FILES {
                render.entry(
                    Path::new(template_file.relative_path),
                    template_file.relative_path,
                    template_file.contents,
                )?;
            }
        }
    }

    Ok(render.managed_paths)
}

struct TemplateRender<'a, 'env> {
    environment: &'a mut Environment<'env>,
    context: &'a JsonValue,
    answers: &'a RenderAnswers,
    destination: &'a Path,
    managed_paths: BTreeSet<PathBuf>,
}

impl TemplateRender<'_, '_> {
    fn entry(&mut self, relative_template: &Path, source_label: &str, source: &str) -> Result<()> {
        let relative = output_relative_path(relative_template)?;
        if managed_paths::should_omit_unmanaged_rendered_path(&relative, self.answers) {
            return Ok(());
        }
        self.managed_paths.insert(relative.clone());

        let rendered = self
            .environment
            .render_str(source, self.context)
            .with_context(|| format!("Failed to render {source_label}"))?;
        write_rendered_file(self.destination, &relative, rendered.as_bytes())
    }
}

fn merge_existing_managed_blocks(
    seed_repo_path: Option<&Path>,
    destination: &Path,
    progress: CliProgress,
) -> Result<()> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(());
    };

    for relative in [
        Path::new(managed_paths::ROOT_AGENTS_PATH),
        Path::new(managed_paths::ROOT_GITATTRIBUTES_PATH),
        Path::new(managed_paths::ROOT_GITIGNORE_PATH),
    ] {
        merge_existing_managed_block(seed_repo_path, destination, relative, progress)?;
    }
    Ok(())
}

fn merge_existing_managed_block(
    seed_repo_path: &Path,
    destination: &Path,
    relative: &Path,
    progress: CliProgress,
) -> Result<()> {
    let Some(spec) = managed_paths::managed_block_spec(relative) else {
        return Ok(());
    };

    let existing_path = seed_repo_path.join(relative);
    if !existing_path.exists() {
        return Ok(());
    }

    let rendered_path = destination.join(relative);
    if !rendered_path.exists() {
        return Ok(());
    }

    let step_label = format!("merge {}", spec.progress_label);
    progress.step(
        &step_label,
        format!("preserve repo-owned {} content", spec.path),
    );
    let existing = progress.log_blocked_on_err(
        fs::read_to_string(&existing_path)
            .with_context(|| format!("Failed to read {}", existing_path.display())),
    )?;
    let rendered = progress.log_blocked_on_err(
        fs::read_to_string(&rendered_path)
            .with_context(|| format!("Failed to read {}", rendered_path.display())),
    )?;
    let block = progress.log_blocked_on_err(extract_jig_block(&rendered, &rendered_path, spec))?;
    let merged =
        progress.log_blocked_on_err(merge_jig_block(&existing, block, &existing_path, spec))?;
    progress.log_blocked_on_err(
        fs::write(&rendered_path, merged)
            .with_context(|| format!("Failed to write {}", rendered_path.display())),
    )
}

fn extract_jig_block<'a>(
    contents: &'a str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<&'a str> {
    let Some((start, end)) = jig_block_bounds(contents, path, spec)? else {
        bail!(
            "Rendered {} does not contain a Jig managed block.",
            path.display()
        );
    };
    Ok(&contents[start..end])
}

fn merge_jig_block(
    existing: &str,
    block: &str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<String> {
    if let Some((start, end)) = jig_block_bounds(existing, path, spec)? {
        return Ok(format!(
            "{}{}{}",
            &existing[..start],
            block,
            &existing[end..]
        ));
    }

    let mut merged = existing.trim_end_matches('\n').to_string();
    if !merged.is_empty() {
        merged.push_str("\n\n");
    }
    merged.push_str(block.trim_end_matches('\n'));
    merged.push('\n');
    Ok(merged)
}

fn jig_block_bounds(
    contents: &str,
    path: &Path,
    spec: managed_paths::ManagedBlockSpec,
) -> Result<Option<(usize, usize)>> {
    let begins = contents.match_indices(spec.begin).collect::<Vec<_>>();
    let ends = contents.match_indices(spec.end).collect::<Vec<_>>();

    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(begin, _)], [(end, _)]) if begin < end => Ok(Some((*begin, end + spec.end.len()))),
        _ => bail!(
            "Malformed Jig managed block in {}. Expected exactly one begin marker before exactly one end marker.",
            path.display()
        ),
    }
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
    remove_existing_symlink(&path)?;
    fs::write(&path, contents).with_context(|| format!("Failed to write {}", path.display()))?;
    set_rendered_permissions(&path, relative)
}

fn remove_existing_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(path)
            .with_context(|| format!("Failed to remove symlink {}", path.display())),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Failed to stat {}", path.display())),
    }
}

fn run_post_render_tasks(destination: &Path) -> Result<()> {
    set_scripts_executable(destination)?;
    crate::policy::write_agent_map(destination, Path::new("agent-map.md"))
}

#[cfg(unix)]
fn set_rendered_permissions(path: &Path, relative: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if managed_paths::is_executable_script(relative) {
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
        if managed_paths::is_executable_script(&relative) {
            paths.push(relative);
        }
    }
    Ok(paths)
}
