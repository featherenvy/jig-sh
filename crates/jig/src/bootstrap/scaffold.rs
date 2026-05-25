use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::Value;

use crate::context::validate_web_package_manager;

use super::{
    AnswerOpts, DevApp, FrontendApp, ScaffoldDb, ScaffoldFrontend, ScaffoldFrontendKind,
    ScaffoldOpts, ScaffoldPreset,
};

#[derive(Clone, Debug)]
pub(super) struct InitScaffoldPlan {
    preset: ScaffoldPreset,
    /// The repo name exactly as requested or inferred from the destination path.
    requested_repo_name: String,
    /// The Rust-compatible repo name recorded in generated Jig answers.
    repo_name: String,
    /// The kebab-case Cargo package stem used for generated workspace members.
    package_name: String,
    /// The underscore Rust module stem derived from `package_name`.
    module_name: String,
    db: ScaffoldDb,
    migration_dir: String,
    package_manager: String,
    frontends: Vec<FrontendScaffold>,
}

mod embedded_templates;
mod frontend;
mod names;
mod rust_workspace;
mod templates;
mod write;

use frontend::{FrontendScaffold, scaffold_bootstrap_command};
use names::{default_repo_name, sanitize_package_name, validate_scaffold_relative_path};
use write::ScaffoldReport;

impl InitScaffoldPlan {
    pub(super) fn from_opts(
        opts: &ScaffoldOpts,
        answers: &AnswerOpts,
        destination: &Path,
    ) -> Result<Option<Self>> {
        if opts.preset.is_none()
            && opts.db.is_none()
            && opts.frontends.is_empty()
            && opts.frontend_list.is_empty()
        {
            return Ok(None);
        }
        let Some(preset) = opts.preset else {
            bail!("Scaffold options require --preset rust-react");
        };
        match preset {
            ScaffoldPreset::RustReact => Self::rust_react(opts, answers, destination).map(Some),
        }
    }

    pub(super) fn apply_answer_defaults(&self, answers: &mut AnswerOpts) {
        if answers.repo_name.as_deref() != Some(self.repo_name.as_str()) {
            answers.repo_name = Some(self.repo_name.clone());
        }
        if answers.sqlx_enabled.is_none() {
            answers.sqlx_enabled = Some(self.db != ScaffoldDb::None);
        }
        if self.db != ScaffoldDb::None {
            if answers.rust_migration_dir.is_none() {
                answers.rust_migration_dir = Some(self.migration_dir.clone());
            }
            if answers.rust_sqlx_metadata_dir.is_none() {
                answers.rust_sqlx_metadata_dir = Some(".sqlx".into());
            }
            if answers.schema_dump_enabled.is_none() {
                answers.schema_dump_enabled = Some(false);
            }
        }
        if answers.rust_crate_roots.is_empty() {
            answers.rust_crate_roots = vec!["apps".into(), "crates".into()];
        }
        let package_manager = answers
            .web_package_manager
            .clone()
            .unwrap_or_else(|| self.package_manager.clone());
        if answers.web_package_manager.is_none() {
            answers.web_package_manager = Some(self.package_manager.clone());
        }
        if answers.bootstrap_command.is_none() {
            answers.bootstrap_command = Some(scaffold_bootstrap_command(
                &package_manager,
                &self.frontends,
            ));
        }
        if answers.frontend_apps.is_empty() {
            answers.frontend_apps = self
                .frontends
                .iter()
                .map(|frontend| FrontendApp {
                    name: frontend.name.clone(),
                    dir: frontend.dir.clone(),
                    coverage_threshold: frontend.coverage_threshold,
                    kind: frontend.dev_kind.clone(),
                })
                .collect();
        }
        if answers.dev_apps.is_empty() {
            answers.dev_apps = vec![DevApp {
                name: "api".into(),
                dir: Some(".".into()),
                kind: "env-port".into(),
                command: Some(format!(
                    "BIND_ADDR=\"${{HOST}}:${{PORT}}\" cargo run -p {}-api",
                    self.package_name
                )),
                argv: Vec::new(),
                port: None,
                host: None,
                proxy: true,
            }];
        }
    }

    pub(super) fn summary(&self) -> String {
        let mut parts = vec![format!("Rust backend for {}", self.repo_name)];
        match self.db {
            ScaffoldDb::None => {}
            ScaffoldDb::Postgres => parts.push("postgres DB".to_string()),
            ScaffoldDb::Sqlite => parts.push("sqlite DB".to_string()),
        }
        if self.requested_repo_name != self.repo_name {
            parts.push(format!("repo name {}", self.repo_name));
        }
        if !self.frontends.is_empty() {
            parts.push(format!("{} frontend app(s)", self.frontends.len()));
        }
        parts.join(", ")
    }

    pub(super) fn sanitized_repo_name_note(&self) -> Option<String> {
        (self.requested_repo_name != self.repo_name).then(|| {
            format!(
                "requested repo name '{}' was normalized to '{}' for Rust crate compatibility",
                self.requested_repo_name, self.repo_name
            )
        })
    }

    pub(super) fn write(&self, destination: &Path, force: bool) -> Result<Value> {
        let mut files = self.render_rust_workspace_files()?;
        for frontend in &self.frontends {
            files.extend(frontend.render_files(&self.package_manager, &self.repo_name)?);
        }
        let report = ScaffoldReport::write_files(destination, files, force)?;
        Ok(report.into_json(self))
    }

    fn rust_react(opts: &ScaffoldOpts, answers: &AnswerOpts, destination: &Path) -> Result<Self> {
        let requested_repo_name = answers
            .repo_name
            .clone()
            .unwrap_or_else(|| default_repo_name(destination));
        let package_name = sanitize_package_name(&requested_repo_name)?;
        let repo_name = package_name.clone();
        // sanitize_package_name validates the underscore form before this replacement.
        let module_name = package_name.replace('-', "_");
        let db = opts.db.unwrap_or(ScaffoldDb::None);
        let package_manager = answers
            .web_package_manager
            .clone()
            .unwrap_or_else(|| "bun".into());
        validate_web_package_manager(&package_manager)?;
        if db != ScaffoldDb::None && answers.sqlx_enabled == Some(false) {
            bail!("Scaffold --db requires SQLx; remove --sqlx-enabled false or use --db none");
        }
        let migration_dir = answers
            .rust_migration_dir
            .clone()
            .unwrap_or_else(|| "migrations".into());
        if db != ScaffoldDb::None || answers.rust_migration_dir.is_some() {
            validate_scaffold_relative_path("migration dir", &migration_dir)?;
        }
        let frontend_specs = collect_frontend_specs(opts);
        if !frontend_specs.is_empty() && !answers.frontend_apps.is_empty() {
            bail!(
                "Scaffold frontends cannot be combined with --frontend-app answers; use --frontend/--frontends for scaffold output or --frontend-app for existing app configuration"
            );
        }
        let frontends = if frontend_specs.is_empty() && answers.frontend_apps.is_empty() {
            vec![FrontendScaffold::from_spec(ScaffoldFrontend {
                name: "web".into(),
                kind: ScaffoldFrontendKind::Spa,
            })?]
        } else if frontend_specs.is_empty() {
            answers
                .frontend_apps
                .iter()
                .map(FrontendScaffold::from_frontend_app)
                .collect::<Result<Vec<_>>>()?
        } else {
            frontend_specs
                .into_iter()
                .map(FrontendScaffold::from_spec)
                .collect::<Result<Vec<_>>>()?
        };
        validate_unique_frontends(&frontends)?;
        Ok(Self {
            preset: ScaffoldPreset::RustReact,
            requested_repo_name,
            repo_name,
            package_name,
            module_name,
            db,
            migration_dir,
            package_manager,
            frontends,
        })
    }

    pub(super) fn output_paths(&self) -> Vec<PathBuf> {
        let mut paths = self.rust_workspace_relative_paths();
        paths.extend(
            self.frontends
                .iter()
                .flat_map(FrontendScaffold::relative_paths),
        );
        paths
    }
}

impl ScaffoldFrontendKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Spa => "spa",
            Self::Admin => "admin",
            Self::Astro => "astro",
        }
    }
}

fn collect_frontend_specs(opts: &ScaffoldOpts) -> Vec<ScaffoldFrontend> {
    opts.frontends
        .iter()
        .chain(opts.frontend_list.iter())
        .cloned()
        .collect()
}

fn validate_unique_frontends(frontends: &[FrontendScaffold]) -> Result<()> {
    let mut names = HashSet::new();
    let mut dirs = HashSet::new();
    for frontend in frontends {
        validate_scaffold_relative_path("frontend dir", &frontend.dir)?;
        if !names.insert(frontend.name.as_str()) {
            bail!("Duplicate scaffold frontend '{}'", frontend.name);
        }
        if !dirs.insert(frontend.dir.as_str()) {
            bail!("Duplicate scaffold frontend dir '{}'", frontend.dir);
        }
        let root_dir = frontend.dir.split('/').next().unwrap_or_default();
        if matches!(root_dir, "apps" | "crates") {
            bail!(
                "Scaffold frontend '{}' uses reserved directory '{}'",
                frontend.name,
                frontend.dir
            );
        }
    }
    Ok(())
}
