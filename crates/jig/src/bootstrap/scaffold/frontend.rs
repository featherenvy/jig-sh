use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::names::{sanitize_package_name, validate_scaffold_name};
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{FrontendApp, ScaffoldFrontend, ScaffoldFrontendKind};

const VITE_REACT_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/index.html.jinja",
        output: "index.html",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/vite.config.ts.jinja",
        output: "vite.config.ts",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/eslint.config.js.jinja",
        output: "eslint.config.js",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/main.tsx.jinja",
        output: "src/main.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.tsx.jinja",
        output: "src/App.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/App.test.tsx.jinja",
        output: "src/App.test.tsx",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/vite-react/src/test-setup.ts.jinja",
        output: "src/test-setup.ts",
    },
];

const ASTRO_TEMPLATES: &[ScaffoldTemplateFile] = &[
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/package.json.jinja",
        output: "package.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/astro.config.mjs.jinja",
        output: "astro.config.mjs",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/tsconfig.json.jinja",
        output: "tsconfig.json",
    },
    ScaffoldTemplateFile {
        template: "rust-react/frontend/astro/src/pages/index.astro.jinja",
        output: "src/pages/index.astro",
    },
];

#[derive(Clone, Debug)]
pub(super) struct FrontendScaffold {
    pub(super) name: String,
    pub(super) dir: String,
    pub(super) kind: ScaffoldFrontendKind,
    pub(super) coverage_threshold: u32,
    pub(super) dev_kind: String,
    package_name: String,
}

impl FrontendScaffold {
    pub(super) fn from_spec(spec: ScaffoldFrontend) -> Result<Self> {
        validate_scaffold_name("frontend name", &spec.name)?;
        let package_name = sanitize_package_name(&spec.name)?;
        let (coverage_threshold, dev_kind) = scaffold_frontend_defaults(spec.kind);
        Ok(Self {
            dir: spec.name.clone(),
            name: spec.name,
            kind: spec.kind,
            coverage_threshold,
            dev_kind: dev_kind.into(),
            package_name,
        })
    }

    pub(super) fn from_frontend_app(app: &FrontendApp) -> Result<Self> {
        validate_scaffold_name("frontend app name", &app.name)?;
        Ok(Self {
            name: app.name.clone(),
            dir: app.dir.clone(),
            kind: infer_frontend_kind(app),
            coverage_threshold: app.coverage_threshold,
            dev_kind: app.kind.clone(),
            package_name: sanitize_package_name(&app.name)?,
        })
    }

    pub(super) fn relative_paths(&self) -> Vec<PathBuf> {
        self.template_files()
            .iter()
            .map(|file| PathBuf::from(format!("{}/{}", self.dir, file.output)))
            .collect()
    }

    pub(super) fn render_files(
        &self,
        package_manager: &str,
        repo_name: &str,
    ) -> Result<Vec<ScaffoldFile>> {
        self.render_template_files(package_manager, repo_name)
    }

    fn render_template_files(
        &self,
        package_manager: &str,
        repo_name: &str,
    ) -> Result<Vec<ScaffoldFile>> {
        let template_files = self.template_files();
        ensure_scaffold_template_paths(template_files)?;
        let title = title_case(&self.name);
        let context = json!({
            "package_name": self.package_name,
            "repo_name": repo_name,
            "title": title,
            "subtitle": if self.kind == ScaffoldFrontendKind::Admin {
                "Operational workspace"
            } else {
                "Product workspace"
            },
            "install_command": scaffold_frontend_install_command(package_manager),
        });
        template_files
            .iter()
            .map(|file| {
                Ok(scaffold_file(
                    format!("{}/{}", self.dir, file.output),
                    render_scaffold_template(file.template, &context)?,
                ))
            })
            .collect()
    }

    fn template_files(&self) -> &'static [ScaffoldTemplateFile] {
        match self.kind {
            ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin => VITE_REACT_TEMPLATES,
            ScaffoldFrontendKind::Astro => ASTRO_TEMPLATES,
        }
    }
}

fn scaffold_frontend_defaults(kind: ScaffoldFrontendKind) -> (u32, &'static str) {
    match kind {
        ScaffoldFrontendKind::Spa | ScaffoldFrontendKind::Admin => (80, "vite"),
        ScaffoldFrontendKind::Astro => (0, "env-port"),
    }
}

pub(super) fn scaffold_bootstrap_command(
    package_manager: &str,
    frontends: &[FrontendScaffold],
) -> String {
    std::iter::once(optional_cargo_command("cargo fetch", "bootstrap"))
        .chain(frontends.iter().map(|frontend| {
            format!(
                "(cd {} && {})",
                frontend.dir,
                scaffold_frontend_install_command(package_manager)
            )
        }))
        .collect::<Vec<_>>()
        .join(" && ")
}

fn scaffold_frontend_install_command(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun install",
        "npm" => "npm install",
        "pnpm" => "pnpm install",
        "yarn" => "yarn install",
        _ => unreachable!("web package manager was already validated"),
    }
}

fn optional_cargo_command(command: &str, action: &str) -> String {
    format!(
        "if [ -f Cargo.toml ]; then {command}; else printf '%s\\n' 'No Cargo.toml found; skipping cargo {action}.'; fi"
    )
}

fn infer_frontend_kind(app: &FrontendApp) -> ScaffoldFrontendKind {
    // Existing harness config stores Astro apps as env-port dev entries.
    // Explicit scaffold specs use ScaffoldFrontendKind directly.
    match (app.kind.as_str(), app.name.as_str()) {
        ("env-port", _) => ScaffoldFrontendKind::Astro,
        (_, "admin" | "admin-panel") => ScaffoldFrontendKind::Admin,
        _ => ScaffoldFrontendKind::Spa,
    }
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
