use clap::{Parser, ValueEnum};

use super::run::format_presets_human_summary;
use super::*;

#[test]
fn parses_presets_command() {
    let presets = Cli::try_parse_from(["jig", "presets"]).unwrap();

    assert!(matches!(presets.command, CommandKind::Presets));
}

#[test]
fn presets_summary_explains_defaults_and_ownership() {
    let output = bootstrap::scaffold_presets_report();
    assert_eq!(
        output["presets"].as_array().unwrap().len(),
        bootstrap::ScaffoldPreset::value_variants().len()
    );

    let summary = format_presets_human_summary(&output);

    assert!(summary.contains("available presets"));
    assert!(summary.contains("rust-react"));
    assert!(summary.contains("Rust crate roots default to apps and crates."));
    assert!(summary.contains("apps/<repo>-api"));
    assert!(summary.contains("admin: Vite React admin app in admin-panel/"));
    assert!(summary.contains("jig init ./my-app --preset rust-react"));
    assert!(summary.contains("project-owned after creation"));
    assert!(summary.contains("Presets are starter shapes, not long-term application frameworks."));
}
