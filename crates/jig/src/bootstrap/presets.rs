use clap::ValueEnum;
use serde::Serialize;
use serde_json::{Value, json};

use super::ScaffoldPreset;

#[derive(Debug, Serialize)]
struct ScaffoldPresetReport {
    name: &'static str,
    summary: &'static str,
    defaults: Vec<&'static str>,
    layout: Vec<&'static str>,
    frontend_shorthands: Vec<ScaffoldFrontendShorthandReport>,
    examples: Vec<&'static str>,
    ownership: &'static str,
    non_goals: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ScaffoldFrontendShorthandReport {
    name: &'static str,
    expands_to: &'static str,
}

pub fn scaffold_presets_report() -> Value {
    let presets = ScaffoldPreset::value_variants()
        .iter()
        .copied()
        .map(ScaffoldPreset::report)
        .collect::<Vec<_>>();
    json!({
        "ok": true,
        "command": crate::tool_defs::cli_command::PRESETS,
        "presets": presets
    })
}

impl ScaffoldPreset {
    fn report(self) -> ScaffoldPresetReport {
        match self {
            Self::RustReact => ScaffoldPresetReport {
                name: "rust-react",
                summary: "Rust API workspace plus optional Vite React, Astro, and admin frontend apps.",
                defaults: vec![
                    "Rust crate roots default to apps and crates.",
                    "Frontends default to web when omitted.",
                    "Database scaffolding defaults to none; pass --db postgres or --db sqlite when wanted.",
                    "Generated frontend checks default to bun unless --web-package-manager is supplied.",
                    "Schema dumps stay disabled until a command is configured.",
                ],
                layout: vec![
                    "apps/<repo>-api",
                    "crates/<repo>-core",
                    "crates/<repo>",
                    "crates/<repo>-http",
                    "crates/<repo>-test-support",
                    "crates/<repo>-db when --db postgres or --db sqlite is selected",
                ],
                frontend_shorthands: vec![
                    ScaffoldFrontendShorthandReport {
                        name: "web",
                        expands_to: "Vite React app in web/",
                    },
                    ScaffoldFrontendShorthandReport {
                        name: "landing",
                        expands_to: "Astro site in landing/",
                    },
                    ScaffoldFrontendShorthandReport {
                        name: "admin",
                        expands_to: "Vite React admin app in admin-panel/",
                    },
                ],
                examples: vec![
                    "jig init ./my-app --preset rust-react",
                    "jig init ./my-app --preset rust-react --db postgres --frontends web,landing,admin",
                    "jig init ./my-app --preset rust-react --db sqlite --frontends web",
                ],
                ownership: "Scaffolded application code is project-owned after creation; jig update keeps the Jig harness current and does not rewrite app code.",
                non_goals: vec![
                    "jig update does not migrate or overwrite scaffolded application source.",
                    "Presets are starter shapes, not long-term application frameworks.",
                ],
            },
        }
    }
}
