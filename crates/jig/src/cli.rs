use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{
    bootstrap,
    context::RepoContext,
    mcp, runtime,
    tool_defs::{self, DEFAULT_RECEIPTS_LIMIT},
};

mod vault;

pub(crate) use vault::{
    VaultAuditCommand, VaultAuditVerifyOpts, VaultCommand, VaultInitOpts, VaultRunOpts,
    VaultRuntimeOpts, VaultSecretCommand, VaultSecretListOpts, VaultSecretRemoveOpts,
    VaultSecretSetOpts, VaultStatusOpts,
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

const AGENT_AFTER_HELP: &str = "\
Examples:
  jig agent doctor
  jig agent bootstrap";

const AGENT_BOOTSTRAP_AFTER_HELP: &str = "\
Use --marketplace for a GitHub owner/repo skill marketplace or another configured marketplace source.

Examples:
  jig agent bootstrap
  jig agent bootstrap --marketplace owner/skills-repo";

const PROXY_RUN_AFTER_HELP: &str = "\
The app command must come after --. Ad-hoc proxy runs bind the app to 127.0.0.1; use [[dev.apps]].host for configured loopback IP targets.

Examples:
  jig proxy run web -- npm run dev
  jig proxy run web -- vite --open
  jig proxy run api --port 3000 -- cargo run
  jig proxy run web --no-proxy -- npm run dev";

const MIGRATION_ADD_AFTER_HELP: &str = "\
Use --plan-id to associate the migration with an open structured work plan.

Examples:
  jig migration-add create_users
  jig migration-add add_login_tokens --plan-id plan_abc123";

const WORK_START_AFTER_HELP: &str = "\
Use --body for short notes or --body-file for a prepared markdown plan.
Use --print-plan-id when shell scripts only need the new plan id.

Examples:
  jig work start --title \"Add auth\" --body \"Implement login flow and validation.\"
  plan_id=\"$(jig work start --title \"Fix signup\" --body-file .agent/notes/signup-plan.md --print-plan-id)\"";

const WORK_CHECK_AFTER_HELP: &str = "\
Run all required gates for a plan, or use --tool to run one configured gate.
Use --summary for terminal scanning; JSON remains the default for automation.

Examples:
  jig work check --plan-id plan_abc123
  jig work check --plan-id plan_abc123 --summary
  jig work check --plan-id plan_abc123 --tool jig.test";

const WORK_GATES_AFTER_HELP: &str = "\
Use --summary for terminal scanning; JSON remains the default for automation.

Examples:
  jig work gates --plan-id plan_abc123
  jig work gates --plan-id plan_abc123 --summary";

const WORK_FINISH_AFTER_HELP: &str = "\
Close a plan after required gates pass; use --outcome for a machine-readable result.

Examples:
  jig work finish --plan-id plan_abc123 --resolution \"Auth flow complete\" --outcome success";

const WORK_RECEIPTS_AFTER_HELP: &str = "\
JSON is the stable default. Use --summary for terminal scanning.

Examples:
  jig work receipts --failed-only --summary --limit 5
  jig work receipts --plan-id plan_abc123 --summary";

const CHECK_AFTER_HELP: &str = "\
Run configured project checks or Jig-owned repository policy checks.

Examples:
  jig check fmt
  jig check contract
  jig check rust-file-loc --changed-against origin/main";

const VAULT_AFTER_HELP: &str = "\
Jig Vault stores local secrets outside the repository. Terminal use prompts for
the vault passphrase; scripts can set JIG_VAULT_PASSPHRASE. Command-line
passphrases are not accepted.

Quick start:
  jig vault init
  jig vault secret set api_token --value-prompt
  jig vault run --env TOKEN=api_token -- sh -c 'printf \"%s\" \"$TOKEN\"'";

#[derive(Debug, Subcommand)]
pub(crate) enum CommandKind {
    /// Create a new repository and render Jig harness files into it.
    #[command(name = tool_defs::cli_command::INIT)]
    Init(bootstrap::InitOpts),
    /// Adopt Jig harness files into an existing repository.
    #[command(name = tool_defs::cli_command::ADOPT)]
    Adopt(bootstrap::AdoptOpts),
    /// Refresh managed Jig harness files from the configured template source.
    #[command(name = tool_defs::cli_command::UPDATE)]
    Update(bootstrap::UpdateOpts),
    /// Run the configured project bootstrap command.
    #[command(name = tool_defs::cli_command::BOOTSTRAP)]
    Bootstrap(ToolOpts),
    /// Run configured project checks and Jig-owned repository policy checks.
    #[command(
        name = tool_defs::cli_command::CHECK,
        subcommand,
        after_help = CHECK_AFTER_HELP
    )]
    Check(CheckCommand),
    /// Regenerate schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::SCHEMA_DUMP)]
    SchemaDump(ToolOpts),
    /// Add a forward-only SQLx migration file when SQLx is enabled.
    #[command(name = tool_defs::cli_command::MIGRATION_ADD)]
    MigrationAdd(MigrationAddOpts),
    /// Generate the repository agent guide map.
    #[command(name = tool_defs::cli_command::AGENT_MAP, subcommand)]
    AgentMap(AgentMapCommand),
    /// Generate a TODO report for unchecked SQLx queries.
    #[command(
        name = tool_defs::cli_command::GENERATE_SQLX_UNCHECKED_QUERIES_TODO,
        hide = true
    )]
    GenerateSqlxUncheckedQueriesTodo(GenerateSqlxUncheckedQueriesTodoOpts),
    /// Run configured development apps through the local dev proxy.
    #[command(name = tool_defs::cli_command::DEV)]
    Dev(DevOpts),
    /// Manage the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY, subcommand)]
    Proxy(ProxyCommand),
    /// Manage the local encrypted Jig vault.
    #[command(
        name = tool_defs::cli_command::VAULT,
        subcommand,
        after_help = VAULT_AFTER_HELP
    )]
    Vault(VaultCommand),
    /// Inspect or bootstrap local agent tooling.
    #[command(
        name = tool_defs::cli_command::AGENT,
        subcommand,
        after_help = AGENT_AFTER_HELP
    )]
    Agent(AgentCommand),
    /// Manage structured work plans, receipts, gates, and decisions.
    #[command(name = tool_defs::cli_command::WORK, subcommand)]
    Work(WorkCommand),
    /// Serve the Jig MCP server over stdio.
    #[command(name = tool_defs::cli_command::MCP)]
    Mcp,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentMapCommand {
    /// Rewrite agent-map.md from tracked AGENTS.md files.
    #[command(name = tool_defs::cli_command::AGENT_MAP_GENERATE)]
    Generate(AgentMapOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum CheckCommand {
    /// Run the configured Rust format check.
    #[command(name = tool_defs::cli_command::CHECK_FMT)]
    Fmt(ToolOpts),
    /// Run the configured Rust clippy check.
    #[command(name = tool_defs::cli_command::CHECK_CLIPPY)]
    Clippy(ToolOpts),
    /// Run the configured default test command.
    #[command(name = tool_defs::cli_command::CHECK_TEST)]
    Test(ToolOpts),
    /// Run the configured locked test command.
    #[command(name = tool_defs::cli_command::CHECK_TEST_LOCKED)]
    TestLocked(ToolOpts),
    /// Run the configured TypeScript lint command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_LINT)]
    TypeScriptLint(ToolOpts),
    /// Run the configured TypeScript typecheck command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_TYPECHECK)]
    TypeScriptTypecheck(ToolOpts),
    /// Run the configured TypeScript build command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_BUILD)]
    TypeScriptBuild(ToolOpts),
    /// Run the configured TypeScript coverage command.
    #[command(name = tool_defs::cli_command::CHECK_TYPESCRIPT_COVERAGE)]
    TypeScriptCoverage(ToolOpts),
    /// Verify committed SQLx metadata when SQLx is enabled.
    #[command(name = tool_defs::cli_command::CHECK_SQLX)]
    Sqlx(ToolOpts),
    /// Verify generated schema documentation when schema dumps are enabled.
    #[command(name = tool_defs::cli_command::CHECK_SCHEMA)]
    Schema(ToolOpts),
    /// Validate the generated Jig command contract and runtime wiring.
    #[command(name = tool_defs::cli_command::CHECK_CONTRACT)]
    Contract(ToolOpts),
    /// Check agent-map.md coverage and links.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_MAP)]
    AgentMap(AgentMapOpts),
    /// Verify crate-level AGENTS.md guide coverage and required sections.
    #[command(name = tool_defs::cli_command::CHECK_AGENT_GUIDES)]
    AgentGuides,
    /// Enforce Rust file-size policy for changed or tracked files.
    #[command(name = tool_defs::cli_command::CHECK_RUST_FILE_LOC)]
    RustFileLoc(CheckRustFileLocOpts),
    /// Fail if disallowed mod.rs files exist under configured crate roots.
    #[command(name = tool_defs::cli_command::CHECK_NO_MOD_RS)]
    NoModRs,
    /// Verify existing migrations were not mutated.
    #[command(name = tool_defs::cli_command::CHECK_MIGRATION_IMMUTABILITY)]
    MigrationImmutability(CheckMigrationImmutabilityOpts),
    /// Verify non-test SQLx queries use compile-time checked macros.
    #[command(name = tool_defs::cli_command::CHECK_SQLX_UNCHECKED_NON_TEST)]
    SqlxUncheckedNonTest,
}

#[derive(Args, Debug)]
pub(crate) struct AgentMapOpts {
    #[arg(
        long = "map",
        default_value = "agent-map.md",
        help = "Agent map file to generate or check"
    )]
    pub(crate) map_path: PathBuf,
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkCommand {
    /// Start a structured goal plan from an objective and validation contract.
    #[command(name = tool_defs::cli_command::WORK_GOAL)]
    Goal(WorkGoalOpts),
    /// Start a structured work plan and session.
    #[command(
        name = tool_defs::cli_command::WORK_START,
        after_help = WORK_START_AFTER_HELP
    )]
    Start(WorkStartOpts),
    /// Append progress text to an open work plan.
    #[command(name = tool_defs::cli_command::WORK_APPEND)]
    Append(WorkAppendOpts),
    /// Run configured or selected work gate checks for a plan.
    #[command(
        name = tool_defs::cli_command::WORK_CHECK,
        after_help = WORK_CHECK_AFTER_HELP
    )]
    Check(WorkCheckOpts),
    /// Show required gate status for a plan.
    #[command(
        name = tool_defs::cli_command::WORK_GATES,
        after_help = WORK_GATES_AFTER_HELP
    )]
    Gates(WorkGatesOpts),
    /// Record a durable decision for the current work.
    #[command(name = tool_defs::cli_command::WORK_DECIDE)]
    Decide(WorkDecisionAddOpts),
    /// List recorded command receipts.
    #[command(
        name = tool_defs::cli_command::WORK_RECEIPTS,
        after_help = WORK_RECEIPTS_AFTER_HELP
    )]
    Receipts(WorkReceiptsOpts),
    /// Summarize current structured work state.
    #[command(name = tool_defs::cli_command::WORK_STATUS)]
    Status(WorkStatusOpts),
    /// Close a work plan after required gates pass.
    #[command(
        name = tool_defs::cli_command::WORK_FINISH,
        after_help = WORK_FINISH_AFTER_HELP
    )]
    Finish(WorkFinishOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    /// Report local Codex marketplace readiness for this repo.
    #[command(name = tool_defs::cli_command::AGENT_DOCTOR)]
    Doctor(AgentDoctorOpts),
    /// Register the configured Codex skills marketplace.
    #[command(
        name = tool_defs::cli_command::AGENT_BOOTSTRAP,
        after_help = AGENT_BOOTSTRAP_AFTER_HELP
    )]
    Bootstrap(AgentBootstrapOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProxyCommand {
    /// Start the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY_START)]
    Start(ProxyStartOpts),
    /// Stop the local development proxy.
    #[command(name = tool_defs::cli_command::PROXY_STOP)]
    Stop(ProxyStopOpts),
    /// List proxy routes and runtime status.
    #[command(name = tool_defs::cli_command::PROXY_LIST)]
    List(ProxyListOpts),
    /// Remove stale proxy routes.
    #[command(name = tool_defs::cli_command::PROXY_PRUNE)]
    Prune(ProxyPruneOpts),
    /// Run an ad-hoc app command behind the proxy.
    #[command(name = tool_defs::cli_command::PROXY_RUN)]
    Run(ProxyRunOpts),
    /// Route a stable hostname to an already-running local service.
    #[command(name = tool_defs::cli_command::PROXY_ALIAS)]
    Alias(ProxyAliasOpts),
    /// Generate, inspect, trust, or untrust local proxy certificates.
    #[command(name = tool_defs::cli_command::PROXY_CERT, subcommand)]
    Cert(ProxyCertCommand),
    /// Install, uninstall, or inspect a per-user proxy service.
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
    #[arg(long, help = "Plain-language objective for the work")]
    pub(crate) objective: String,
    #[arg(long, help = "Observable condition that means the goal is complete")]
    pub(crate) success: String,
    #[arg(
        long = "validation",
        required = true,
        help = "Validation command or check that must pass; may be repeated"
    )]
    pub(crate) validations: Vec<String>,
    #[arg(
        long = "constraint",
        help = "Constraint to preserve while working; may be repeated"
    )]
    pub(crate) constraints: Vec<String>,
    #[arg(
        long = "checkpoint",
        help = "Progress checkpoint to include in the plan; may be repeated"
    )]
    pub(crate) checkpoints: Vec<String>,
    #[arg(long, help = "Optional plan title; defaults from the objective")]
    pub(crate) title: Option<String>,
    #[arg(long, help = "Additional notes to include in the generated plan")]
    pub(crate) notes: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct AgentBootstrapOpts {
    #[arg(
        long,
        help = "Marketplace source to register; defaults to the single configured source"
    )]
    pub(crate) marketplace: Option<String>,
}

#[derive(Args, Debug, Default)]
pub(crate) struct AgentDoctorOpts {
    #[arg(long, help = "Print a concise human-readable readiness summary")]
    pub(crate) summary: bool,
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
    #[arg(long = "app", help = "Configured app name to run; may be repeated")]
    pub(crate) apps: Vec<String>,
    #[arg(long, help = "Discover JavaScript workspace apps with dev scripts")]
    pub(crate) discover_workspace: bool,
    #[arg(long, help = "Run apps directly without publishing proxy routes")]
    pub(crate) no_proxy: bool,
    #[command(flatten)]
    pub(crate) proxy: ProxyRuntimeOpts,
}

#[derive(Args, Debug)]
pub(crate) struct ProxyStartOpts {
    #[arg(long, help = "Run the proxy in the foreground instead of detaching")]
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
    #[arg(long, help = "Print raw route and listener details")]
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
#[command(after_help = PROXY_RUN_AFTER_HELP)]
pub(crate) struct ProxyRunOpts {
    #[arg(help = "Route name to publish for the ad-hoc app")]
    pub(crate) name: String,
    #[arg(
        long,
        help = "App kind used for command setup, such as env-port or vite"
    )]
    pub(crate) kind: Option<String>,
    #[arg(long, help = "Working directory for the app command")]
    pub(crate) dir: Option<PathBuf>,
    #[arg(long, help = "Fixed backend port for the app", value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) port: Option<u16>,
    #[arg(long, help = "Run directly without publishing a proxy route")]
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
    #[arg(help = "Route name to publish for the existing service")]
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
    #[arg(
        long,
        help = "Regenerate certificate files even when usable files already exist"
    )]
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
    #[arg(long, help = "Structured work plan id to attach the receipt to")]
    pub(crate) plan_id: Option<String>,
    #[arg(
        long,
        conflicts_with = "plan_id",
        help = "Run without appending a receipt to .agent/state"
    )]
    pub(crate) no_receipt: bool,
}

#[derive(Args, Debug)]
#[command(after_help = MIGRATION_ADD_AFTER_HELP)]
pub(crate) struct MigrationAddOpts {
    /// Migration name, for example create_users.
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct CheckRustFileLocOpts {
    #[arg(long, help = "Check staged Rust files against HEAD.")]
    pub(crate) staged: bool,
    #[arg(
        long = "changed-against",
        help = "Check Rust files changed between the given git ref and HEAD."
    )]
    pub(crate) changed_against: Option<String>,
    #[arg(
        long,
        help = "Check all tracked Rust files against a zero baseline; existing oversized legacy files fail unless annotated."
    )]
    pub(crate) all: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CheckMigrationImmutabilityOpts {
    #[arg(long = "changed-against", help = "Git ref to compare against")]
    pub(crate) changed_against: String,
}

#[derive(Args, Debug)]
pub(crate) struct GenerateSqlxUncheckedQueriesTodoOpts {
    /// Optional output path for the generated TODO report.
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkStartOpts {
    #[arg(long, help = "Short human-readable plan title")]
    pub(crate) title: String,
    #[arg(long, help = "Initial plan body text")]
    pub(crate) body: Option<String>,
    #[arg(long, help = "Path to read the initial plan body from")]
    pub(crate) body_file: Option<PathBuf>,
    #[arg(long, help = "Print only the new plan id instead of JSON")]
    pub(crate) print_plan_id: bool,
}

#[derive(Args, Debug)]
pub(crate) struct WorkAppendOpts {
    #[arg(long, help = "Open plan id to append to")]
    pub(crate) plan_id: String,
    #[arg(long, help = "Progress text to append")]
    pub(crate) body: Option<String>,
    #[arg(long, help = "Path to read progress text from")]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkCheckOpts {
    #[arg(long, help = "Open plan id to check")]
    pub(crate) plan_id: String,

    #[arg(
        long = "tool",
        help = "Specific gate tool to run; defaults to configured gates"
    )]
    pub(crate) tools: Vec<String>,

    #[arg(long, help = "Print a concise human-readable check summary")]
    pub(crate) summary: bool,
}

#[derive(Args, Debug)]
pub(crate) struct WorkGatesOpts {
    #[arg(long, help = "Plan id to inspect")]
    pub(crate) plan_id: String,

    #[arg(long, help = "Print a concise human-readable gate summary")]
    pub(crate) summary: bool,
}

#[derive(Args, Debug, Default)]
pub(crate) struct WorkStatusOpts {
    #[arg(long, help = "Print a concise human-readable work summary")]
    pub(crate) summary: bool,
}

#[derive(Args, Debug)]
pub(crate) struct WorkFinishOpts {
    #[arg(long, help = "Open plan id to close")]
    pub(crate) plan_id: String,
    #[arg(long, help = "Resolution summary recorded on the plan")]
    pub(crate) resolution: Option<String>,
    #[arg(long, help = "Optional session outcome; defaults to the resolution")]
    pub(crate) outcome: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkReceiptsOpts {
    #[arg(long, help = "Filter by session id")]
    pub(crate) session_id: Option<String>,
    #[arg(long, help = "Filter by plan id")]
    pub(crate) plan_id: Option<String>,
    #[arg(long, help = "Filter by Jig tool name")]
    pub(crate) tool_name: Option<String>,
    #[arg(long, help = "Only show failed receipts")]
    pub(crate) failed_only: bool,
    #[arg(long, default_value_t = DEFAULT_RECEIPTS_LIMIT, help = "Maximum receipts to show")]
    pub(crate) limit: usize,
    #[arg(long, help = "Print a concise human-readable receipt summary")]
    pub(crate) summary: bool,
}

impl Default for WorkReceiptsOpts {
    fn default() -> Self {
        Self {
            session_id: None,
            plan_id: None,
            tool_name: None,
            failed_only: false,
            limit: DEFAULT_RECEIPTS_LIMIT,
            summary: false,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct WorkDecisionAddOpts {
    #[arg(long, help = "Short decision title")]
    pub(crate) title: String,
    #[arg(long, help = "Chosen option or approach")]
    pub(crate) selected_option: String,
    #[arg(long, help = "Reason the selected option was chosen")]
    pub(crate) rationale: String,
    #[arg(long, help = "Alternative considered; may be repeated")]
    pub(crate) alternatives: Vec<String>,
    #[arg(long, help = "Plan id to associate with the decision")]
    pub(crate) plan_id: Option<String>,
}

mod command_conversion;

mod run;

pub(crate) use run::{is_structured_json_failure, run, structured_error_exit_code};
#[cfg(test)]
use run::{
    moved_check_command_hint, require_json_ok, should_add_template_hint,
    test_command_reports_failure_with_ok,
};

fn parse_ip_literal_string(value: &str) -> std::result::Result<String, String> {
    value
        .parse::<std::net::IpAddr>()
        .map(|_| value.to_string())
        .map_err(|_| format!("'{value}' must be an IP literal"))
}

#[cfg(test)]
mod help_tests;
#[cfg(test)]
mod tests;
