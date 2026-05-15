use std::path::PathBuf;

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
    #[command(name = tool_defs::cli_command::BOOTSTRAP)]
    Bootstrap(ToolOpts),
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

mod run;

#[cfg(test)]
use run::{command_reports_failure_with_ok, require_json_ok, should_add_template_hint};
pub(crate) use run::{is_structured_json_failure, run};

fn parse_ip_literal_string(value: &str) -> std::result::Result<String, String> {
    value
        .parse::<std::net::IpAddr>()
        .map(|_| value.to_string())
        .map_err(|_| format!("'{value}' must be an IP literal"))
}

#[cfg(test)]
mod tests;
