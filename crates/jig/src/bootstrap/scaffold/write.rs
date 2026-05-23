use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::{InitScaffoldPlan, ScaffoldDb, ScaffoldPreset};

#[derive(Clone, Debug, Default)]
pub(super) struct ScaffoldReport {
    files_created: Vec<String>,
    files_modified: Vec<String>,
    files_unchanged: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct ScaffoldFile {
    relative: String,
    contents: String,
}

#[derive(Clone, Debug)]
enum ScaffoldWrite {
    Create(ScaffoldFile),
    Modify(ScaffoldFile),
    Unchanged(String),
}

pub(super) fn scaffold_file(
    relative: impl Into<String>,
    contents: impl Into<String>,
) -> ScaffoldFile {
    ScaffoldFile {
        relative: relative.into(),
        contents: contents.into(),
    }
}

impl ScaffoldReport {
    pub(super) fn write_files(
        destination: &Path,
        files: Vec<ScaffoldFile>,
        force: bool,
    ) -> Result<Self> {
        let mut report = Self::default();
        let mut seen = HashSet::new();
        let mut conflicts = Vec::new();
        let mut writes = Vec::new();

        for file in files {
            if !seen.insert(file.relative.clone()) {
                bail!(
                    "Scaffold rendered duplicate output path {}; this is a Jig scaffold bug",
                    file.relative
                );
            }
            let path = destination.join(&file.relative);
            if !path.exists() {
                writes.push(ScaffoldWrite::Create(file));
                continue;
            }
            let existing = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            if existing != file.contents && !force {
                conflicts.push(file.relative.clone());
            } else if existing == file.contents {
                writes.push(ScaffoldWrite::Unchanged(file.relative));
            } else {
                writes.push(ScaffoldWrite::Modify(file));
            }
        }

        if !conflicts.is_empty() {
            conflicts.sort();
            bail!(
                "Scaffold paths already exist and differ; pass --force to overwrite them in place:\n  {}",
                conflicts.join("\n  ")
            );
        }

        for write in writes {
            match write {
                ScaffoldWrite::Create(file) => {
                    write_scaffold_file(destination, file, &mut report.files_created)?;
                }
                ScaffoldWrite::Modify(file) => {
                    write_scaffold_file(destination, file, &mut report.files_modified)?;
                }
                ScaffoldWrite::Unchanged(relative) => report.files_unchanged.push(relative),
            }
        }
        Ok(report)
    }

    pub(super) fn into_json(self, plan: &InitScaffoldPlan) -> Value {
        json!({
            "preset": match plan.preset {
                ScaffoldPreset::RustReact => "rust-react",
            },
            "repo_name": &plan.repo_name,
            "repo_name_sanitized_from": (plan.requested_repo_name != plan.repo_name).then_some(&plan.requested_repo_name),
            "db": match plan.db {
                ScaffoldDb::None => "none",
                ScaffoldDb::Postgres => "postgres",
                ScaffoldDb::Sqlite => "sqlite",
            },
            "frontends": plan.frontends.iter().map(|frontend| {
                json!({
                    "name": frontend.name,
                    "dir": frontend.dir,
                    "kind": frontend.kind.as_str(),
                })
            }).collect::<Vec<_>>(),
            "files_created": self.files_created,
            "files_modified": self.files_modified,
            "files_unchanged": self.files_unchanged,
        })
    }
}

fn write_scaffold_file(
    destination: &Path,
    file: ScaffoldFile,
    completed: &mut Vec<String>,
) -> Result<()> {
    let path = destination.join(&file.relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&path, file.contents)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    completed.push(file.relative);
    Ok(())
}
