use std::io::Write;
use std::process;

use anyhow::Result;
use clap::Parser;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use super::*;

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
    match cli.command {
        CommandKind::Init(opts) => print_json(&bootstrap::run_init(opts)?),
        CommandKind::Adopt(opts) => print_json(&bootstrap::run_adopt(opts)?),
        CommandKind::Update(opts) => print_json(&bootstrap::run_update(opts)?),
        CommandKind::Mcp => {
            let ctx = RepoContext::load()?;
            mcp::serve(&ctx)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Dev(opts) => {
            let output = crate::dev_proxy::commands::dev_without_context(opts)?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Dev(opts) => {
            let Some(ctx) = RepoContext::load_optional()? else {
                anyhow::bail!(
                    "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml` dev app configuration. Run it from a Jig repo, or use `scripts/jig proxy run <name> -- <command>` for an ad-hoc command."
                );
            };
            let output = runtime::dispatch(&ctx, CommandKind::Dev(opts))?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Proxy(command) => {
            let output = crate::dev_proxy::commands::proxy_without_context(command)?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Proxy(command)
            if crate::dev_proxy::commands::can_run_without_context(&command) =>
        {
            let output = if let Some(ctx) = RepoContext::load_optional()? {
                runtime::dispatch(&ctx, CommandKind::Proxy(command))?
            } else {
                crate::dev_proxy::commands::proxy_without_context(command)?
            };
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        other => {
            let require_ok = command_reports_failure_with_ok(&other);
            let ctx = RepoContext::load()?;
            let output = runtime::dispatch(&ctx, other)?;
            print_json(&output)?;
            require_json_ok(require_ok, &output)
        }
    }
}

pub(super) fn command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    matches!(
        command,
        CommandKind::Dev(_)
            | CommandKind::Proxy(_)
            | CommandKind::Agent(AgentCommand::Doctor)
            | CommandKind::AgentMap(AgentMapCommand::Check(_))
            | CommandKind::CheckAgentGuides
            | CommandKind::CheckRustFileLoc(_)
            | CommandKind::CheckNoModRs
            | CommandKind::CheckMigrationImmutability(_)
            | CommandKind::CheckSqlxUncheckedNonTest
    )
}

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(JsonOkFalse.into());
    }
    Ok(())
}

#[derive(Debug)]
struct JsonOkFalse;

impl std::fmt::Display for JsonOkFalse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command reported ok=false")
    }
}

impl std::error::Error for JsonOkFalse {}

pub(crate) fn is_structured_json_failure(error: &anyhow::Error) -> bool {
    error.is::<JsonOkFalse>()
}

fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => exit_with_cli_error(error),
    }
}

fn exit_with_cli_error(error: clap::Error) -> ! {
    if should_add_template_hint(&error) {
        let message = error.to_string();
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    error.exit();
}

pub(super) fn should_add_template_hint(error: &clap::Error) -> bool {
    if !matches!(
        error.kind(),
        ErrorKind::InvalidValue | ErrorKind::TooFewValues
    ) {
        return false;
    }
    error
        .context()
        .any(|(kind, value)| kind == ContextKind::InvalidArg && context_mentions_template(value))
}

fn context_mentions_template(value: &ContextValue) -> bool {
    match value {
        ContextValue::String(value) => is_template_arg(value),
        ContextValue::Strings(values) => values.iter().any(|value| is_template_arg(value)),
        ContextValue::StyledStr(value) => is_template_arg(&value.to_string()),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| is_template_arg(&value.to_string())),
        _ => false,
    }
}

fn is_template_arg(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .is_some_and(|arg| arg == "--template")
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    handle.write_all(b"\n")?;
    Ok(())
}
