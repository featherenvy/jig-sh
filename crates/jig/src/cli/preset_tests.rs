use clap::Parser;

use super::*;

#[test]
fn parses_presets_command() {
    let presets = Cli::try_parse_from(["jig", "presets"]).unwrap();

    assert!(matches!(presets.command, CommandKind::Presets));
}
