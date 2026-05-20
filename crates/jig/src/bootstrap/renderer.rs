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
use super::crate_guide::crate_guide_skip_reason;
use super::managed_paths;
use super::preview_seed::seed_preview_workspace;
use super::staged_render::StagedRender;
use super::template_source::PreparedTemplateSource;
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
    if let Some(seed_repo_path) = request.seed_repo_path {
        request.progress.step(
            "scaffold crate guides",
            "starter AGENTS.md for missing crates",
        );
        managed_paths.extend(request.progress.log_blocked_on_err(
            scaffold_missing_crate_guides(
                seed_repo_path,
                &destination,
                request.answers,
                request.progress,
            ),
        )?);
    }
    request
        .progress
        .step("generate agent map", "native renderer");
    request
        .progress
        .log_blocked_on_err(run_post_render_tasks(&destination))?;
    merge_existing_root_agents(request.seed_repo_path, &destination, request.progress)?;
    managed_paths.extend(managed_paths::removed_managed_paths());

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
        if managed_paths::should_omit_unmanaged_rendered_path(&relative, answers) {
            continue;
        }
        managed_paths.insert(relative.clone());

        let source = fs::read_to_string(&template_path)
            .with_context(|| format!("Failed to read {}", template_path.display()))?;
        let rendered = environment
            .render_str(&source, &context)
            .with_context(|| format!("Failed to render {}", template_path.display()))?;
        write_rendered_file(destination, &relative, rendered.as_bytes())?;
    }

    Ok(managed_paths)
}

fn scaffold_missing_crate_guides(
    seed_repo_path: &Path,
    destination: &Path,
    answers: &RenderAnswers,
    progress: CliProgress,
) -> Result<BTreeSet<PathBuf>> {
    let mut scaffolded = BTreeSet::new();
    for root in answers.rust_crate_roots() {
        let crate_root = seed_repo_path.join(root);
        if !crate_root.is_dir() {
            continue;
        }
        // Crate roots are expected to contain direct child crates. Configure
        // additional roots for nested crate groups that need starter guides.
        for entry in fs::read_dir(&crate_root)
            .with_context(|| format!("Failed to read {}", crate_root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let crate_dir = entry.path();
            if !crate_dir.join("Cargo.toml").is_file() {
                continue;
            }
            let relative_crate_dir = crate_dir.strip_prefix(seed_repo_path).with_context(|| {
                format!(
                    "{} is not under {}",
                    crate_dir.display(),
                    seed_repo_path.display()
                )
            })?;
            let relative_guide = relative_crate_dir.join("AGENTS.md");
            let destination_guide = destination.join(&relative_guide);
            // Existing crate guides were copied into the staging directory by
            // seed_preview_workspace before this runs; only scaffold gaps.
            if destination_guide.exists() {
                continue;
            }
            let crate_name = crate_package_name(&crate_dir)?;
            if let Some(reason) = crate_guide_skip_reason(relative_crate_dir, Some(&crate_name)) {
                progress.info(
                    "skipped crate guide",
                    format!("{}: {reason}", relative_crate_dir.display()),
                );
                continue;
            }
            write_rendered_file(
                destination,
                &relative_guide,
                starter_crate_guide(&crate_name).as_bytes(),
            )?;
            scaffolded.insert(relative_guide);
        }
    }
    Ok(scaffolded)
}

fn crate_package_name(crate_dir: &Path) -> Result<String> {
    let fallback = crate_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("crate")
        .to_string();
    let cargo_toml_path = crate_dir.join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("Failed to read {}", cargo_toml_path.display()))?;
    let parsed = toml::from_str::<toml::Value>(&cargo_toml)
        .with_context(|| format!("Failed to parse {}", cargo_toml_path.display()))?;
    Ok(parsed
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or(fallback))
}

fn starter_crate_guide(crate_name: &str) -> String {
    format!(
        "# {crate_name} crate guide\n\n\
## Purpose\n\n\
Document what this crate owns before adding substantial behavior.\n\n\
## Key entrypoints\n\n\
- `src/lib.rs`: library entrypoint when present.\n\
- `src/main.rs`: binary entrypoint when present.\n\n\
## Edit here for X\n\n\
- Update this section with the crate's main responsibilities.\n\n\
## Invariants\n\n\
- Keep crate-specific rules here instead of in the root `AGENTS.md`.\n\n\
## Common commands\n\n\
- `cargo test -p {crate_name}`\n\
\n\
Adjust the package name in commands if this starter guide could not infer it from `Cargo.toml`.\n"
    )
}

fn merge_existing_root_agents(
    seed_repo_path: Option<&Path>,
    destination: &Path,
    progress: CliProgress,
) -> Result<()> {
    let Some(seed_repo_path) = seed_repo_path else {
        return Ok(());
    };

    let existing_path = seed_repo_path.join(managed_paths::ROOT_AGENTS_PATH);
    if !existing_path.exists() {
        return Ok(());
    }

    let rendered_path = destination.join(managed_paths::ROOT_AGENTS_PATH);
    if !rendered_path.exists() {
        return Ok(());
    }

    progress.step("merge root guide", "preserve repo-owned AGENTS.md content");
    let existing = progress.log_blocked_on_err(
        fs::read_to_string(&existing_path)
            .with_context(|| format!("Failed to read {}", existing_path.display())),
    )?;
    let rendered = progress.log_blocked_on_err(
        fs::read_to_string(&rendered_path)
            .with_context(|| format!("Failed to read {}", rendered_path.display())),
    )?;
    let block = progress.log_blocked_on_err(extract_jig_block(&rendered, &rendered_path))?;
    let merged = progress.log_blocked_on_err(merge_jig_block(&existing, block, &existing_path))?;
    progress.log_blocked_on_err(
        fs::write(&rendered_path, merged)
            .with_context(|| format!("Failed to write {}", rendered_path.display())),
    )
}

fn extract_jig_block<'a>(contents: &'a str, path: &Path) -> Result<&'a str> {
    let Some((start, end)) = jig_block_bounds(contents, path)? else {
        bail!(
            "Rendered {} does not contain a Jig managed block.",
            path.display()
        );
    };
    Ok(&contents[start..end])
}

fn merge_jig_block(existing: &str, block: &str, path: &Path) -> Result<String> {
    if let Some((start, end)) = jig_block_bounds(existing, path)? {
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

fn jig_block_bounds(contents: &str, path: &Path) -> Result<Option<(usize, usize)>> {
    let begins = contents
        .match_indices(managed_paths::ROOT_AGENTS_BLOCK_BEGIN)
        .collect::<Vec<_>>();
    let ends = contents
        .match_indices(managed_paths::ROOT_AGENTS_BLOCK_END)
        .collect::<Vec<_>>();

    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(begin, _)], [(end, _)]) if begin < end => Ok(Some((
            *begin,
            end + managed_paths::ROOT_AGENTS_BLOCK_END.len(),
        ))),
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
