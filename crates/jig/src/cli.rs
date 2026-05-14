use std::io::Write;
use std::path::PathBuf;
use std::process;

use anyhow::{Result, bail};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Args, Parser, Subcommand};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{
    bootstrap, context::RepoContext, mcp, runtime, state::DEFAULT_RECEIPTS_LIMIT, tool_defs,
};

#[derive(Debug, Parser)]
#[command(
    name = "jig",
    version,
    about = "Repo-local agent runtime and bootstrapper for jig.sh"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

const TEMPLATE_ERROR_HINT: &str = "\
Templates:
  Omit --template to use the official jig-sh harness template:
  https://github.com/bpcakes/jig-sh.git

If you passed --template without a value, either omit it to use the default
or provide a path/URL.

Use one of:
  jig adopt . --repo-name my-repo --sqlx-enabled false
  jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
  jig adopt . --template /path/to/jig-sh --repo-name my-repo --sqlx-enabled false

Pass --template only for a local checkout, fork, or private template.";

#[derive(Debug, Subcommand)]
pub(crate) enum CommandKind {
    #[command(name = tool_defs::cli_command::INIT)]
    Init(bootstrap::InitOpts),
    #[command(name = tool_defs::cli_command::ADOPT)]
    Adopt(bootstrap::AdoptOpts),
    #[command(name = tool_defs::cli_command::UPDATE)]
    Update(bootstrap::UpdateOpts),
    #[command(name = tool_defs::cli_command::FMT_CHECK)]
    FmtCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::CLIPPY)]
    Clippy(ToolOpts),
    #[command(name = tool_defs::cli_command::TEST)]
    Test(ToolOpts),
    #[command(name = tool_defs::cli_command::TEST_LOCKED)]
    TestLocked(ToolOpts),
    #[command(name = tool_defs::cli_command::SQLX_CHECK)]
    SqlxCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::SCHEMA_CHECK)]
    SchemaCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::SCHEMA_DUMP)]
    SchemaDump(ToolOpts),
    #[command(name = tool_defs::cli_command::MIGRATION_ADD)]
    MigrationAdd(MigrationAddOpts),
    #[command(name = tool_defs::cli_command::CONTRACT_CHECK)]
    ContractCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::DEV)]
    Dev(DevOpts),
    #[command(name = tool_defs::cli_command::RUN_TARGET)]
    RunTarget(RunTargetOpts),
    #[command(name = tool_defs::cli_command::PROXY, subcommand)]
    Proxy(ProxyCommand),
    #[command(name = tool_defs::cli_command::AGENT, subcommand)]
    Agent(AgentCommand),
    #[command(name = tool_defs::cli_command::WORK, subcommand)]
    Work(WorkCommand),
    #[command(name = tool_defs::cli_command::MCP)]
    Mcp,
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkCommand {
    #[command(name = tool_defs::cli_command::WORK_GOAL)]
    Goal(WorkGoalOpts),
    #[command(name = tool_defs::cli_command::WORK_START)]
    Start(WorkStartOpts),
    #[command(name = tool_defs::cli_command::WORK_APPEND)]
    Append(WorkAppendOpts),
    #[command(name = tool_defs::cli_command::WORK_CHECK)]
    Check(WorkCheckOpts),
    #[command(name = tool_defs::cli_command::WORK_GATES)]
    Gates(WorkGatesOpts),
    #[command(name = tool_defs::cli_command::WORK_DECIDE)]
    Decide(WorkDecisionAddOpts),
    #[command(name = tool_defs::cli_command::WORK_RECEIPTS)]
    Receipts(WorkReceiptsOpts),
    #[command(name = tool_defs::cli_command::WORK_STATUS)]
    Status,
    #[command(name = tool_defs::cli_command::WORK_FINISH)]
    Finish(WorkFinishOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    #[command(name = tool_defs::cli_command::AGENT_DOCTOR)]
    Doctor,
    #[command(name = tool_defs::cli_command::AGENT_BOOTSTRAP)]
    Bootstrap(AgentBootstrapOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyCommand {
    #[command(name = tool_defs::cli_command::PROXY_START)]
    Start(ProxyStartOpts),
    #[command(name = tool_defs::cli_command::PROXY_STOP)]
    Stop(ProxyStopOpts),
    #[command(name = tool_defs::cli_command::PROXY_LIST)]
    List(ProxyListOpts),
    #[command(name = tool_defs::cli_command::PROXY_PRUNE)]
    Prune(ProxyPruneOpts),
    #[command(name = tool_defs::cli_command::PROXY_RUN)]
    Run(ProxyRunOpts),
    #[command(name = tool_defs::cli_command::PROXY_ALIAS)]
    Alias(ProxyAliasOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT, subcommand)]
    Cert(ProxyCertCommand),
    #[command(name = tool_defs::cli_command::PROXY_SERVICE, subcommand)]
    Service(ProxyServiceCommand),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyCertCommand {
    #[command(name = tool_defs::cli_command::PROXY_CERT_GENERATE)]
    Generate(ProxyCertGenerateOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_STATUS)]
    Status(ProxyCertRuntimeOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_TRUST)]
    Trust(ProxyCertTrustOpts),
    #[command(name = tool_defs::cli_command::PROXY_CERT_UNTRUST)]
    Untrust(ProxyCertUntrustOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyServiceCommand {
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_INSTALL)]
    Install(ProxyServiceInstallOpts),
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_UNINSTALL)]
    Uninstall(ProxyServiceRuntimeOpts),
    #[command(name = tool_defs::cli_command::PROXY_SERVICE_STATUS)]
    Status(ProxyServiceRuntimeOpts),
}

#[derive(Args, Debug)]
pub(crate) struct WorkGoalOpts {
    #[arg(long)]
    pub(crate) objective: String,
    #[arg(long)]
    pub(crate) success: String,
    #[arg(long = "validation", required = true)]
    pub(crate) validations: Vec<String>,
    #[arg(long = "constraint")]
    pub(crate) constraints: Vec<String>,
    #[arg(long = "checkpoint")]
    pub(crate) checkpoints: Vec<String>,
    #[arg(long)]
    pub(crate) title: Option<String>,
    #[arg(long)]
    pub(crate) notes: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct AgentBootstrapOpts {
    #[arg(long)]
    pub(crate) marketplace: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ProxyRuntimeOpts {
    #[arg(
        long,
        help = "Proxy state directory; defaults to JIG_PROXY_STATE_DIR or ~/.jig/proxy"
    )]
    pub(crate) state_dir: Option<PathBuf>,
    #[arg(
        long,
        help = "HTTP listener port for the local proxy",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub(crate) http_port: Option<u16>,
    #[arg(
        long,
        help = "HTTPS listener port for the local proxy",
        value_parser = clap::value_parser!(u16).range(1..)
    )]
    pub(crate) https_port: Option<u16>,
    #[arg(
        long,
        conflicts_with = "no_https",
        help = "Start or require the HTTPS listener"
    )]
    pub(crate) https: bool,
    #[arg(
        long,
        conflicts_with = "https",
        help = "Disable HTTPS even when [dev].https is true"
    )]
    pub(crate) no_https: bool,
    // Expert diagnostic toggle kept for service parity while HTTP/2 support is
    // still settling; normal users should rely on the [dev] config default.
    #[arg(
        long,
        hide = true,
        conflicts_with = "no_http2",
        help = "Enable HTTP/2 ALPN on the HTTPS listener"
    )]
    pub(crate) http2: bool,
    // Expert diagnostic toggle kept for service parity while HTTP/2 support is
    // still settling; normal users should rely on the [dev] config default.
    #[arg(
        long,
        hide = true,
        conflicts_with = "http2",
        help = "Disable HTTP/2 ALPN on the HTTPS listener"
    )]
    pub(crate) no_http2: bool,
    #[arg(
        long,
        conflicts_with = "no_lan",
        help = "Bind the proxy on 0.0.0.0; LAN clients can reach Jig-supervised loopback apps"
    )]
    pub(crate) lan: bool,
    #[arg(
        long,
        conflicts_with = "lan",
        help = "Disable LAN binding even when [dev].lan is true"
    )]
    pub(crate) no_lan: bool,
    #[arg(long, help = "Private/local TLD for generated route hostnames")]
    pub(crate) tld: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct DevOpts {
    #[arg(long = "app")]
    pub(crate) apps: Vec<String>,
    #[arg(long)]
    pub(crate) discover_workspace: bool,
    #[arg(long)]
    pub(crate) no_proxy: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct ProxyStartOpts {
    #[arg(long)]
    pub(crate) foreground: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyStopOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyListOpts {
    #[arg(long)]
    pub(crate) raw: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyPruneOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug)]
#[command(
    after_help = "Pass the app command after --, for example: jig proxy run web -- vite --open. Ad-hoc proxy runs bind the app to 127.0.0.1; use [[dev.apps]].host for configured loopback IP targets."
)]
pub(crate) struct ProxyRunOpts {
    pub(crate) name: String,
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long)]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) port: Option<u16>,
    #[arg(long)]
    pub(crate) no_proxy: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
    #[arg(
        last = true,
        allow_hyphen_values = true,
        required = true,
        help = "Command to run after --, for example: vite --open"
    )]
    pub(crate) command: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ProxyAliasOpts {
    pub(crate) name: String,
    #[arg(long, help = "Backend TCP port to route to", value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) port: u16,
    #[arg(
        long,
        default_value = "127.0.0.1",
        value_parser = parse_ip_literal_string,
        help = "Backend host as an IP literal"
    )]
    pub(crate) host: String,
    #[arg(
        long,
        help = "Acknowledge that this local alias can proxy browser requests to a non-loopback target IP"
    )]
    pub(crate) accept_non_loopback_target: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertGenerateOpts {
    #[arg(long)]
    pub(crate) force: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertRuntimeOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertTrustOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that the Jig local CA is a non-name-constrained root that can sign certificates for any hostname on this machine, and that Linux trust helpers are resolved from fixed system tool directories"
    )]
    pub(crate) accept_trust_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyCertUntrustOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that Jig will mutate the platform trust store to remove matching Jig local CA certificates"
    )]
    pub(crate) accept_trust_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyServiceInstallOpts {
    #[arg(
        long,
        required = true,
        help = "Acknowledge that Jig will write and load a per-user launchd/systemd service for the local development proxy"
    )]
    pub(crate) accept_service_scope: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug, Default)]
pub(crate) struct ProxyServiceRuntimeOpts {
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ToolOpts {
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MigrationAddOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct RunTargetOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct WorkStartOpts {
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkAppendOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkCheckOpts {
    #[arg(long)]
    pub(crate) plan_id: String,

    #[arg(long = "tool")]
    pub(crate) tools: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkGatesOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
}

#[derive(Args, Debug)]
pub(crate) struct WorkFinishOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) resolution: Option<String>,
    #[arg(long)]
    pub(crate) outcome: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkReceiptsOpts {
    #[arg(long)]
    pub(crate) session_id: Option<String>,
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
    #[arg(long)]
    pub(crate) tool_name: Option<String>,
    #[arg(long)]
    pub(crate) failed_only: bool,
    #[arg(long, default_value_t = DEFAULT_RECEIPTS_LIMIT)]
    pub(crate) limit: usize,
}

impl Default for WorkReceiptsOpts {
    fn default() -> Self {
        Self {
            session_id: None,
            plan_id: None,
            tool_name: None,
            failed_only: false,
            limit: DEFAULT_RECEIPTS_LIMIT,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct WorkDecisionAddOpts {
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) selected_option: String,
    #[arg(long)]
    pub(crate) rationale: String,
    #[arg(long)]
    pub(crate) alternatives: Vec<String>,
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
}

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
                bail!(
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

fn command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    matches!(
        command,
        CommandKind::Dev(_) | CommandKind::Proxy(_) | CommandKind::Agent(AgentCommand::Doctor)
    )
}

fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
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

fn parse_ip_literal_string(value: &str) -> std::result::Result<String, String> {
    value
        .parse::<std::net::IpAddr>()
        .map(|_| value.to_string())
        .map_err(|_| format!("'{value}' must be an IP literal"))
}

fn exit_with_cli_error(error: clap::Error) -> ! {
    if should_add_template_hint(&error) {
        let message = error.to_string();
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    error.exit();
}

fn should_add_template_hint(error: &clap::Error) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_errors_get_hint() {
        let missing_template_value =
            Cli::try_parse_from(["jig", "adopt", ".", "--template"]).unwrap_err();
        assert_eq!(
            missing_template_value.kind(),
            clap::error::ErrorKind::InvalidValue
        );
        assert!(should_add_template_hint(&missing_template_value));

        let unrelated = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();
        assert!(!should_add_template_hint(&unrelated));
    }

    #[test]
    fn adopt_and_init_default_to_official_template() {
        let adopt = Cli::try_parse_from(["jig", "adopt", ".", "--repo-name", "demo"]).unwrap();
        match adopt.command {
            CommandKind::Adopt(bootstrap::AdoptOpts { template, .. }) => {
                assert_eq!(template, None);
            }
            other => panic!("expected adopt command, got {other:?}"),
        }

        let init =
            Cli::try_parse_from(["jig", "init", "/tmp/demo", "--repo-name", "demo"]).unwrap();
        match init.command {
            CommandKind::Init(bootstrap::InitOpts { template, .. }) => {
                assert_eq!(template, None);
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn parses_init_command_with_repeatable_flags() {
        let cli = Cli::try_parse_from([
            "jig",
            "init",
            "/tmp/demo",
            "--template",
            "/tmp/template",
            "--template-mode",
            "committed",
            "--repo-name",
            "demo",
            "--rust-migration-dir",
            "migrations",
            "--rust-crate-root",
            "crates",
            "--rust-crate-root",
            "libs",
            "--frontend-app",
            "frontend:web:40",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Init(bootstrap::InitOpts {
                template_mode,
                answers,
                ..
            }) => {
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
                assert_eq!(answers.rust_crate_roots, vec!["crates", "libs"]);
                assert_eq!(answers.frontend_apps.len(), 1);
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn rejects_working_tree_template_mode() {
        let error = Cli::try_parse_from([
            "jig",
            "init",
            "/tmp/demo",
            "--template",
            "/tmp/template",
            "--template-mode",
            "working-tree",
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("invalid value 'working-tree'"));
        assert!(error.contains("committed"));
    }

    #[test]
    fn parses_update_recopy_flag() {
        let cli = Cli::try_parse_from([
            "jig",
            "update",
            "--recopy",
            "--force",
            "--template",
            "/tmp/template",
            "--template-mode",
            "committed",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Update(bootstrap::UpdateOpts {
                recopy,
                force,
                template,
                template_mode,
                ..
            }) => {
                assert!(recopy);
                assert!(force);
                assert_eq!(template.as_deref(), Some("/tmp/template"));
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
            }
            other => panic!("expected update command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_receipts_filters() {
        let cli = Cli::try_parse_from([
            "jig",
            "work",
            "receipts",
            "--session-id",
            "session_1",
            "--plan-id",
            "plan_1",
            "--tool-name",
            tool::TEST,
            "--failed-only",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Receipts(opts)) => {
                assert_eq!(opts.session_id.as_deref(), Some("session_1"));
                assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
                assert_eq!(opts.tool_name.as_deref(), Some(tool::TEST));
                assert!(opts.failed_only);
                assert_eq!(opts.limit, 5);
            }
            other => panic!("expected work receipts command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_goal() {
        let cli = Cli::try_parse_from([
            "jig",
            "work",
            "goal",
            "--objective",
            "Migrate the API",
            "--success",
            "all handlers use the new type",
            "--validation",
            "make test",
            "--validation",
            "make clippy",
            "--constraint",
            "do not change public routes",
            "--checkpoint",
            "baseline current tests",
            "--title",
            "API migration",
            "--notes",
            "Keep changes small.",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Goal(opts)) => {
                assert_eq!(opts.objective, "Migrate the API");
                assert_eq!(opts.success, "all handlers use the new type");
                assert_eq!(opts.validations, vec!["make test", "make clippy"]);
                assert_eq!(opts.constraints, vec!["do not change public routes"]);
                assert_eq!(opts.checkpoints, vec!["baseline current tests"]);
                assert_eq!(opts.title.as_deref(), Some("API migration"));
                assert_eq!(opts.notes.as_deref(), Some("Keep changes small."));
            }
            other => panic!("expected work goal command, got {other:?}"),
        }
    }

    #[test]
    fn parses_agent_doctor_command() {
        let cli = Cli::try_parse_from(["jig", "agent", "doctor"]).unwrap();

        match cli.command {
            CommandKind::Agent(AgentCommand::Doctor) => {}
            other => panic!("expected agent doctor command, got {other:?}"),
        }
    }

    #[test]
    fn parses_agent_bootstrap_marketplace() {
        let cli = Cli::try_parse_from([
            "jig",
            "agent",
            "bootstrap",
            "--marketplace",
            "../jig-skills",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Agent(AgentCommand::Bootstrap(opts)) => {
                assert_eq!(opts.marketplace.as_deref(), Some("../jig-skills"));
            }
            other => panic!("expected agent bootstrap command, got {other:?}"),
        }
    }

    #[test]
    fn parses_proxy_run_command() {
        let cli = Cli::try_parse_from([
            "jig",
            "proxy",
            "run",
            "web",
            "--kind",
            "vite",
            "--http-port",
            "1555",
            "--",
            "vite",
            "--open",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Proxy(ProxyCommand::Run(opts)) => {
                assert_eq!(opts.name, "web");
                assert_eq!(opts.kind.as_deref(), Some("vite"));
                assert_eq!(opts.proxy.http_port, Some(1555));
                assert!(!opts.no_proxy);
                assert_eq!(opts.command, vec!["vite", "--open"]);
            }
            other => panic!("expected proxy run command, got {other:?}"),
        }
    }

    #[test]
    fn proxy_run_requires_separator_before_command() {
        let error = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn parses_proxy_run_no_proxy() {
        let cli = Cli::try_parse_from([
            "jig",
            "proxy",
            "run",
            "web",
            "--no-proxy",
            "--",
            "cargo",
            "run",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Proxy(ProxyCommand::Run(opts)) => {
                assert!(opts.no_proxy);
                assert_eq!(opts.command, vec!["cargo", "run"]);
            }
            other => panic!("expected proxy run command, got {other:?}"),
        }
    }

    #[test]
    fn parses_proxy_state_dir() {
        let cli =
            Cli::try_parse_from(["jig", "proxy", "list", "--state-dir", "/tmp/jig-proxy-test"])
                .unwrap();

        match cli.command {
            CommandKind::Proxy(ProxyCommand::List(opts)) => {
                assert_eq!(
                    opts.proxy.state_dir,
                    Some(PathBuf::from("/tmp/jig-proxy-test"))
                );
            }
            other => panic!("expected proxy list command, got {other:?}"),
        }
    }

    #[test]
    fn parses_proxy_alias_port_flag() {
        let cli = Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "8080"]).unwrap();

        match cli.command {
            CommandKind::Proxy(ProxyCommand::Alias(opts)) => {
                assert_eq!(opts.name, "api");
                assert_eq!(opts.port, 8080);
            }
            other => panic!("expected proxy alias command, got {other:?}"),
        }
    }

    #[test]
    fn proxy_alias_host_rejects_non_ip_literals_at_parse_time() {
        let error = Cli::try_parse_from([
            "jig",
            "proxy",
            "alias",
            "api",
            "--port",
            "8080",
            "--host",
            "localhost",
        ])
        .unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn proxy_ports_reject_zero_at_parse_time() {
        let alias_error =
            Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "0"]).unwrap_err();
        assert_eq!(alias_error.kind(), clap::error::ErrorKind::ValueValidation);

        let run_error =
            Cli::try_parse_from(["jig", "proxy", "run", "web", "--port", "0", "--", "vite"])
                .unwrap_err();
        assert_eq!(run_error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn proxy_cert_trust_requires_scope_acknowledgement_at_parse_time() {
        for command in ["trust", "untrust"] {
            let error = Cli::try_parse_from(["jig", "proxy", "cert", command]).unwrap_err();

            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::MissingRequiredArgument
            );
        }
    }

    #[test]
    fn proxy_service_install_requires_scope_acknowledgement_at_parse_time() {
        let error = Cli::try_parse_from(["jig", "proxy", "service", "install"]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn proxy_json_ok_false_is_cli_failure() {
        let error = require_json_ok(true, &serde_json::json!({ "ok": false }))
            .unwrap_err()
            .to_string();

        assert!(error.contains("ok=false"));
        require_json_ok(false, &serde_json::json!({ "ok": false })).unwrap();
        assert!(command_reports_failure_with_ok(&CommandKind::Dev(
            DevOpts {
                apps: Vec::new(),
                discover_workspace: false,
                no_proxy: false,
                proxy: ProxyRuntimeOpts::default(),
            }
        )));
        assert!(command_reports_failure_with_ok(&CommandKind::Agent(
            AgentCommand::Doctor
        )));
    }

    #[test]
    fn parses_proxy_runtime_flags_on_prune_cert_and_service_commands() {
        let prune =
            Cli::try_parse_from(["jig", "proxy", "prune", "--state-dir", "/tmp/proxy"]).unwrap();
        match prune.command {
            CommandKind::Proxy(ProxyCommand::Prune(opts)) => {
                assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
            }
            other => panic!("expected proxy prune command, got {other:?}"),
        }

        let cert =
            Cli::try_parse_from(["jig", "proxy", "cert", "status", "--tld", "test"]).unwrap();
        match cert.command {
            CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Status(opts))) => {
                assert_eq!(opts.proxy.tld.as_deref(), Some("test"));
            }
            other => panic!("expected proxy cert status command, got {other:?}"),
        }

        let cert_trust = Cli::try_parse_from([
            "jig",
            "proxy",
            "cert",
            "trust",
            "--accept-trust-scope",
            "--state-dir",
            "/tmp/proxy",
        ])
        .unwrap();
        match cert_trust.command {
            CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Trust(opts))) => {
                assert!(opts.accept_trust_scope);
                assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
            }
            other => panic!("expected proxy cert trust command, got {other:?}"),
        }

        let cert_untrust = Cli::try_parse_from([
            "jig",
            "proxy",
            "cert",
            "untrust",
            "--accept-trust-scope",
            "--state-dir",
            "/tmp/proxy",
        ])
        .unwrap();
        match cert_untrust.command {
            CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Untrust(opts))) => {
                assert!(opts.accept_trust_scope);
                assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
            }
            other => panic!("expected proxy cert untrust command, got {other:?}"),
        }

        let service = Cli::try_parse_from([
            "jig",
            "proxy",
            "service",
            "status",
            "--state-dir",
            "/tmp/proxy",
        ])
        .unwrap();
        match service.command {
            CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Status(opts))) => {
                assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
            }
            other => panic!("expected proxy service status command, got {other:?}"),
        }

        let service_install = Cli::try_parse_from([
            "jig",
            "proxy",
            "service",
            "install",
            "--accept-service-scope",
            "--state-dir",
            "/tmp/proxy",
        ])
        .unwrap();
        match service_install.command {
            CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Install(opts))) => {
                assert!(opts.accept_service_scope);
                assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
            }
            other => panic!("expected proxy service install command, got {other:?}"),
        }
    }

    #[test]
    fn parses_dev_command_with_selected_apps() {
        let cli = Cli::try_parse_from([
            "jig", "dev", "--app", "web", "--app", "api", "--https", "--lan",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Dev(opts) => {
                assert_eq!(opts.apps, vec!["web", "api"]);
                assert!(opts.proxy.https);
                assert!(opts.proxy.lan);
            }
            other => panic!("expected dev command, got {other:?}"),
        }
    }

    #[test]
    fn parses_hidden_proxy_no_http2_runtime_flag() {
        let cli =
            Cli::try_parse_from(["jig", "proxy", "start", "--foreground", "--no-http2"]).unwrap();

        match cli.command {
            CommandKind::Proxy(ProxyCommand::Start(opts)) => {
                assert!(opts.foreground);
                assert!(opts.proxy.no_http2);
            }
            other => panic!("expected proxy start command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_status_command() {
        let cli = Cli::try_parse_from(["jig", "work", "status"]).unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Status) => {}
            other => panic!("expected work status command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_check_tools() {
        let cli = Cli::try_parse_from([
            "jig",
            "work",
            "check",
            "--plan-id",
            "plan_1",
            "--tool",
            tool::CONTRACT_CHECK,
            "--tool",
            tool::TEST,
        ])
        .unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Check(opts)) => {
                assert_eq!(opts.plan_id, "plan_1");
                assert_eq!(opts.tools, vec![tool::CONTRACT_CHECK, tool::TEST]);
            }
            other => panic!("expected work check command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_gates_command() {
        let cli = Cli::try_parse_from(["jig", "work", "gates", "--plan-id", "plan_1"]).unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Gates(opts)) => {
                assert_eq!(opts.plan_id, "plan_1");
            }
            other => panic!("expected work gates command, got {other:?}"),
        }
    }
}
