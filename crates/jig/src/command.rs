//! Runtime-facing command DTOs.
//!
//! CLI parsing stays in `cli`, and runtime execution stays in `runtime`.
//! This module owns the neutral request shapes passed between them. Types that
//! also back MCP tool arguments derive `Deserialize` here so both CLI and MCP
//! paths reach runtime through the same request vocabulary.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    Bootstrap(ToolRequest),
    Check(CheckCommand),
    SchemaDump(ToolRequest),
    MigrationAdd(MigrationAddRequest),
    AgentMap(AgentMapCommand),
    GenerateSqlxUncheckedQueriesTodo(SqlxTodoRequest),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Dev(DevRequest),
    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    Proxy(ProxyCommand),
    Agent(AgentCommand),
    Work(WorkCommand),
}

#[derive(Clone, Debug)]
pub(crate) struct ToolRequest {
    plan_id: Option<String>,
    record_receipt: bool,
}

impl Default for ToolRequest {
    fn default() -> Self {
        Self {
            plan_id: None,
            record_receipt: true,
        }
    }
}

impl ToolRequest {
    pub(crate) fn new(plan_id: Option<String>, record_receipt: bool) -> Self {
        Self {
            plan_id,
            record_receipt,
        }
    }

    pub(crate) fn into_parts(self) -> (Option<String>, bool) {
        (self.plan_id, self.record_receipt)
    }
}

#[derive(Debug)]
pub(crate) enum CheckCommand {
    Fmt(ToolRequest),
    Clippy(ToolRequest),
    Test(ToolRequest),
    TestLocked(ToolRequest),
    TypeScriptLint(ToolRequest),
    TypeScriptTypecheck(ToolRequest),
    TypeScriptBuild(ToolRequest),
    TypeScriptCoverage(ToolRequest),
    Sqlx(ToolRequest),
    Schema(ToolRequest),
    Contract(ToolRequest),
    AgentMap(AgentMapRequest),
    AgentGuides,
    RustFileLoc(RustFileLocRequest),
    NoModRs,
    MigrationImmutability(MigrationImmutabilityRequest),
    SqlxUncheckedNonTest,
}

#[derive(Debug)]
pub(crate) enum AgentMapCommand {
    Generate(AgentMapRequest),
}

#[derive(Debug)]
pub(crate) struct AgentMapRequest {
    pub(crate) map_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct MigrationAddRequest {
    pub(crate) name: String,
    pub(crate) tool: ToolRequest,
}

#[derive(Debug)]
pub(crate) struct RustFileLocRequest {
    pub(crate) staged: bool,
    pub(crate) changed_against: Option<String>,
    pub(crate) all: bool,
}

#[derive(Debug)]
pub(crate) struct MigrationImmutabilityRequest {
    pub(crate) changed_against: String,
}

#[derive(Debug)]
pub(crate) struct SqlxTodoRequest {
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) enum VaultCommand {
    Audit(VaultAuditCommand),
    Init(VaultInitRequest),
    Status(VaultStatusRequest),
    Secret(VaultSecretCommand),
    Run(VaultRunRequest),
}

#[derive(Debug)]
pub(crate) enum VaultAuditCommand {
    Verify(VaultAuditVerifyRequest),
}

#[derive(Debug)]
pub(crate) enum VaultSecretCommand {
    List(VaultSecretListRequest),
    Set(VaultSecretSetRequest),
    Remove(VaultSecretRemoveRequest),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VaultRuntimeOptions {
    pub(crate) home: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct VaultInitRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultStatusRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultAuditVerifyRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultSecretListRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultSecretSetRequest {
    pub(crate) name: String,
    pub(crate) value_source: VaultSecretValueSource,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VaultSecretValueSource {
    Auto,
    Stdin,
    Prompt,
}

#[derive(Debug)]
pub(crate) struct VaultSecretRemoveRequest {
    pub(crate) name: String,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultRunRequest {
    pub(crate) env: Vec<String>,
    pub(crate) files: Vec<String>,
    pub(crate) command: Vec<String>,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) enum AgentCommand {
    Doctor,
    Bootstrap(AgentBootstrapRequest),
}

#[derive(Debug)]
pub(crate) struct AgentBootstrapRequest {
    pub(crate) marketplace: Option<String>,
}

#[derive(Debug)]
pub(crate) enum WorkCommand {
    Goal(WorkGoalRequest),
    Start(WorkStartRequest),
    Append(WorkAppendRequest),
    Check(WorkCheckRequest),
    Gates(WorkGatesRequest),
    Evidence(WorkEvidenceRequest),
    Decide(WorkDecisionRequest),
    Receipts(WorkReceiptsRequest),
    Status,
    Finish(WorkFinishRequest),
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkGoalRequest {
    pub(crate) objective: String,
    pub(crate) success: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) validations: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) constraints: Vec<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) checkpoints: Vec<String>,
    pub(crate) title: Option<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkStartRequest {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkAppendRequest {
    pub(crate) plan_id: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkCheckRequest {
    pub(crate) plan_id: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkGatesRequest {
    pub(crate) plan_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkEvidenceRequest {
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkDecisionRequest {
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) alternatives: Vec<String>,
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkReceiptsRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    #[serde(default, deserialize_with = "crate::serde_helpers::null_or_default")]
    pub(crate) failed_only: bool,
    // `usize::default()` is 0, but a null receipt limit should keep the
    // public default instead of asking for zero rows.
    #[serde(
        default = "crate::serde_helpers::default_receipts_limit",
        deserialize_with = "crate::serde_helpers::null_as_default_receipts_limit"
    )]
    pub(crate) limit: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkFinishRequest {
    pub(crate) plan_id: String,
    pub(crate) resolution: Option<String>,
    pub(crate) outcome: Option<String>,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct DevRequest {
    pub(crate) apps: Vec<String>,
    pub(crate) discover_workspace: bool,
    pub(crate) no_proxy: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Default)]
pub(crate) struct ProxyRuntimeOptions {
    pub(crate) state_dir: Option<PathBuf>,
    pub(crate) http_port: Option<u16>,
    pub(crate) https_port: Option<u16>,
    pub(crate) https: bool,
    pub(crate) no_https: bool,
    pub(crate) http2: bool,
    pub(crate) no_http2: bool,
    pub(crate) lan: bool,
    pub(crate) no_lan: bool,
    pub(crate) tld: Option<String>,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) enum ProxyCommand {
    Start(ProxyStartRequest),
    Stop(ProxyStopRequest),
    List(ProxyListRequest),
    Prune(ProxyPruneRequest),
    Run(ProxyRunRequest),
    Alias(ProxyAliasRequest),
    Cert(ProxyCertCommand),
    Service(ProxyServiceCommand),
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) enum ProxyCertCommand {
    Generate(ProxyCertGenerateRequest),
    Status(ProxyCertRuntimeRequest),
    Trust(ProxyCertTrustRequest),
    Untrust(ProxyCertUntrustRequest),
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) enum ProxyServiceCommand {
    Install(ProxyServiceInstallRequest),
    Uninstall(ProxyServiceRuntimeRequest),
    Status(ProxyServiceRuntimeRequest),
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct ProxyStartRequest {
    pub(crate) foreground: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyStopRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyListRequest {
    pub(crate) raw: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyPruneRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct ProxyRunRequest {
    pub(crate) name: String,
    pub(crate) kind: Option<String>,
    pub(crate) dir: Option<PathBuf>,
    pub(crate) port: Option<u16>,
    pub(crate) no_proxy: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
    pub(crate) command: Vec<String>,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug)]
pub(crate) struct ProxyAliasRequest {
    pub(crate) name: String,
    pub(crate) port: u16,
    pub(crate) host: String,
    pub(crate) accept_non_loopback_target: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyCertGenerateRequest {
    pub(crate) force: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyCertRuntimeRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyCertTrustRequest {
    pub(crate) accept_trust_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyCertUntrustRequest {
    pub(crate) accept_trust_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyServiceInstallRequest {
    pub(crate) accept_service_scope: bool,
    pub(crate) proxy: ProxyRuntimeOptions,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct ProxyServiceRuntimeRequest {
    pub(crate) proxy: ProxyRuntimeOptions,
}
