use super::answers::RenderAnswers;

pub(super) fn generated_gates(answers: &RenderAnswers) -> Vec<String> {
    // Keep this list in sync with the check tools rendered into the harness.
    // Bootstrap adopt tests cross-check the rendered tools against this preview.
    let mut gates = Vec::new();
    if answers.bootstrap_command_configured() {
        gates.push("scripts/jig bootstrap".into());
    }
    gates.extend([
        "scripts/jig check contract".into(),
        "scripts/jig check fmt".into(),
        "scripts/jig check clippy".into(),
        "scripts/jig check test".into(),
    ]);
    if answers.sqlx_enabled() {
        gates.push("scripts/jig check sqlx".into());
    }
    if answers.schema_dump_enabled() {
        gates.push("scripts/jig check schema".into());
        gates.push("scripts/jig schema-dump".into());
    }
    if !answers.frontend_apps().is_empty() {
        gates.extend([
            "scripts/jig check typescript-lint".into(),
            "scripts/jig check typescript-typecheck".into(),
            "scripts/jig check typescript-build".into(),
            "scripts/jig check typescript-coverage".into(),
        ]);
    }
    gates.push("scripts/jig check agent-guides".into());
    gates
}
