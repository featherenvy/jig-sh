use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const CURRENT_SESSION_FILE: &str = "jig-current-session.txt";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_ID: &str = "jig-skills";
// jig.sh generated repos default to the shared Jig skills marketplace; forks can
// override or opt out through agent_tooling.codex.marketplaces in .jig.toml.
pub(crate) const DEFAULT_CODEX_MARKETPLACE_SOURCE: &str = "bpcakes/jig-skills";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_PLUGINS: &[&str] = &[
    "jig-rust@jig-skills",
    "jig-swift@jig-skills",
    "jig-typescript@jig-skills",
    "jig-exec-plans@jig-skills",
];

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoConfig {
    #[serde(rename = "_src_path")]
    src_path: String,
    #[serde(rename = "_commit")]
    commit: String,
    #[allow(dead_code)]
    #[serde(default, rename = "_template_mode")]
    template_mode: String,
    #[allow(dead_code)]
    #[serde(default, rename = "_template_local_path")]
    template_local_path: String,
    repo_name: String,
    default_branch: String,
    #[allow(dead_code)]
    #[serde(default)]
    ci_github_runner: String,
    jig_version: String,
    #[allow(dead_code)]
    #[serde(default)]
    template_source_url: String,
    #[allow(dead_code)]
    #[serde(default = "default_true")]
    makefile_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    sqlx_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    rust_crate_roots: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    rust_migration_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_sqlx_metadata_dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    schema_dump_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    schema_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    sqlx_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    migration_add_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    bootstrap_command: String,
    #[allow(dead_code)]
    #[serde(default = "default_contract_check_command")]
    contract_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    dev_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_fmt_check_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_clippy_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_test_command: String,
    #[allow(dead_code)]
    #[serde(default)]
    rust_test_locked_command: String,
    #[serde(default = "default_web_package_manager")]
    web_package_manager: String,
    #[serde(default)]
    frontend_apps: Vec<FrontendAppConfig>,
    #[serde(default)]
    dev: DevConfig,
    #[serde(default)]
    work: WorkConfig,
    #[serde(default)]
    agent_tooling: AgentToolingConfig,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrontendAppConfig {
    pub(crate) name: String,
    pub(crate) dir: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) coverage_threshold: u32,
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevConfig {
    #[serde(default = "default_proxy_http_port")]
    pub(crate) proxy_port: u16,
    #[serde(default = "default_proxy_https_port")]
    pub(crate) https_port: Option<u16>,
    #[serde(default)]
    pub(crate) https: bool,
    #[serde(default = "default_true")]
    pub(crate) http2: bool,
    #[serde(default)]
    pub(crate) lan: bool,
    #[serde(default = "default_dev_tld")]
    pub(crate) tld: String,
    #[serde(default)]
    pub(crate) workspace_discovery: bool,
    #[serde(default)]
    pub(crate) apps: Vec<DevAppConfig>,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            proxy_port: default_proxy_http_port(),
            https_port: default_proxy_https_port(),
            https: false,
            http2: true,
            lan: false,
            tld: default_dev_tld(),
            workspace_discovery: false,
            apps: Vec::new(),
        }
    }
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevAppConfig {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) dir: Option<String>,
    #[serde(default = "default_dev_app_kind")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) argv: Vec<String>,
    #[serde(default)]
    pub(crate) port: Option<u16>,
    #[serde(default)]
    pub(crate) host: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) proxy: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkConfig {
    #[serde(default)]
    checks: Vec<String>,
    #[serde(default)]
    gates: Vec<WorkGateConfig>,
    #[allow(dead_code)]
    #[serde(default)]
    refinements: Vec<WorkRefinementConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentToolingConfig {
    #[serde(default)]
    pub(crate) codex: CodexToolingConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexToolingConfig {
    #[serde(default = "default_codex_marketplaces")]
    pub(crate) marketplaces: Vec<CodexMarketplaceConfig>,
}

impl Default for CodexToolingConfig {
    fn default() -> Self {
        Self {
            marketplaces: default_codex_marketplaces(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodexMarketplaceConfig {
    pub(crate) id: String,
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) plugins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkGateConfig {
    pub(crate) id: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) tool: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) skill: Option<String>,
    #[serde(default = "default_required")]
    pub(crate) required: bool,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkRefinementConfig {
    id: String,
    skill: String,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ContractManifest {
    contract_version: u32,
    tool_namespace: String,
    jig_version: String,
    #[serde(default)]
    required_make_targets: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    optional_make_targets: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    required_commands: Vec<String>,
    tools: Vec<ManifestTool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManifestTool {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) command: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RepoContext {
    root: PathBuf,
    current_session_path: PathBuf,
    config: RepoConfig,
    manifest: ContractManifest,
}

impl RepoContext {
    pub(crate) fn load() -> Result<Self> {
        let root = if let Ok(root) = std::env::var("JIG_REPO_ROOT") {
            PathBuf::from(root)
        } else {
            find_repo_root_from(&std::env::current_dir()?)?
        };
        Self::load_from_root(root)
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn load_optional() -> Result<Option<Self>> {
        // Contextless cleanup/status commands should still work when a shell
        // inherited a stale JIG_REPO_ROOT. Required repo commands use load()
        // instead, where the env var remains an explicit override.
        if let Ok(root) = std::env::var("JIG_REPO_ROOT") {
            let root = PathBuf::from(root);
            match Self::load_from_root(root.clone()) {
                Ok(ctx) => return Ok(Some(ctx)),
                Err(error) => {
                    eprintln!(
                        "jig ignored invalid JIG_REPO_ROOT={} for contextless command lookup: {error:#}",
                        root.display()
                    );
                }
            }
        }
        let Some(root) = find_optional_repo_root()? else {
            return Ok(None);
        };
        Self::load_from_root(root).map(Some)
    }

    fn load_from_root(root: PathBuf) -> Result<Self> {
        let config_path = root.join(".jig.toml");
        let config_text = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let config: RepoConfig = toml::from_str(&config_text).with_context(|| {
            format!(
                "Failed to parse {}. Jig rejects unknown .jig.toml keys during upgrades; remove typos or experimental keys and retry.",
                config_path.display()
            )
        })?;
        validate_config(&config)?;

        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        if !matches!(manifest.contract_version, 1 | 2) {
            bail!(
                "Unsupported jig contract version: {}",
                manifest.contract_version
            );
        }
        if manifest.tool_namespace != "jig" {
            bail!("Unsupported tool namespace: {}", manifest.tool_namespace);
        }
        if manifest.contract_version == 1 && manifest.required_make_targets.is_empty() {
            bail!("jig contract manifest does not declare required make targets");
        }
        if manifest.contract_version == 2 && manifest.required_commands.is_empty() {
            bail!("jig contract manifest does not declare required commands");
        }
        if config.jig_version != manifest.jig_version {
            bail!(
                "jig version mismatch between .jig.toml ({}) and manifest ({})",
                config.jig_version,
                manifest.jig_version
            );
        }

        let current_session_path = resolve_current_session_path(&root)?;

        Ok(Self {
            root,
            current_session_path,
            config,
            manifest,
        })
    }

    pub(crate) fn tool_specs(&self) -> &[ManifestTool] {
        &self.manifest.tools
    }

    pub(crate) fn tool_spec(&self, name: &str) -> Option<&ManifestTool> {
        self.manifest.tools.iter().find(|tool| tool.name == name)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn repo_name(&self) -> &str {
        &self.config.repo_name
    }

    pub(crate) fn default_branch(&self) -> &str {
        &self.config.default_branch
    }

    pub(crate) fn source_commit(&self) -> &str {
        &self.config.commit
    }

    pub(crate) fn source_path(&self) -> &str {
        &self.config.src_path
    }

    pub(crate) fn command_for_key(&self, key: &str) -> Result<&str> {
        // Keep this whitelist aligned with RepoConfig, bootstrap::AnswerOpts,
        // bootstrap::answers::RawAnswers, and .jig.toml.jinja.
        let command = match key {
            "bootstrap_command" => &self.config.bootstrap_command,
            "contract_check_command" => &self.config.contract_check_command,
            "migration_add_command" => &self.config.migration_add_command,
            "rust_clippy_command" => &self.config.rust_clippy_command,
            "rust_fmt_check_command" => &self.config.rust_fmt_check_command,
            "rust_test_command" => &self.config.rust_test_command,
            "rust_test_locked_command" => &self.config.rust_test_locked_command,
            "schema_check_command" => &self.config.schema_check_command,
            "schema_dump_command" => &self.config.schema_dump_command,
            "sqlx_check_command" => &self.config.sqlx_check_command,
            _ => bail!("Unsupported command key in jig contract: {key}"),
        };
        if command.trim().is_empty() {
            bail!("Command key {key} is empty in .jig.toml");
        }
        Ok(command)
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn web_package_manager(&self) -> &str {
        &self.config.web_package_manager
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn frontend_apps(&self) -> &[FrontendAppConfig] {
        &self.config.frontend_apps
    }

    #[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
    pub(crate) fn dev_config(&self) -> &DevConfig {
        &self.config.dev
    }

    pub(crate) fn work_gates(&self) -> Vec<WorkGateConfig> {
        let mut gates = self.config.work.gates.clone();
        let mut existing_ids = gates
            .iter()
            .map(|gate| gate.id.clone())
            .collect::<HashSet<_>>();

        for tool in &self.config.work.checks {
            if gates
                .iter()
                .any(|gate| gate.kind == "check" && gate.tool.as_ref() == Some(tool))
            {
                continue;
            }

            let id = unique_gate_id(gate_id_from_tool_name(tool), &mut existing_ids);
            gates.push(WorkGateConfig {
                id,
                kind: "check".into(),
                tool: Some(tool.clone()),
                skill: None,
                required: true,
            });
        }

        gates
    }

    pub(crate) fn work_check_tools(&self) -> Vec<String> {
        self.work_gates()
            .into_iter()
            .filter(|gate| gate.kind == "check")
            .filter_map(|gate| gate.tool)
            .collect()
    }

    pub(crate) fn codex_marketplaces(&self) -> &[CodexMarketplaceConfig] {
        &self.config.agent_tooling.codex.marketplaces
    }

    pub(crate) fn state_dir(&self) -> PathBuf {
        self.root.join(".agent/state")
    }

    pub(crate) fn state_file(&self, name: &str) -> PathBuf {
        self.state_dir().join(name)
    }

    pub(crate) fn plan_body_path(&self, plan_id: &str) -> PathBuf {
        self.root.join(".agent/plans").join(format!("{plan_id}.md"))
    }

    pub(crate) fn current_session_path(&self) -> PathBuf {
        self.current_session_path.clone()
    }
}

fn default_required() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_proxy_http_port() -> u16 {
    1355
}

fn default_proxy_https_port() -> Option<u16> {
    Some(1443)
}

fn default_dev_tld() -> String {
    "localhost".into()
}

fn default_dev_app_kind() -> String {
    "env-port".into()
}

fn default_web_package_manager() -> String {
    "bun".into()
}

fn default_contract_check_command() -> String {
    "scripts/check-jig-contract.sh".into()
}

fn default_codex_marketplaces() -> Vec<CodexMarketplaceConfig> {
    vec![CodexMarketplaceConfig {
        id: DEFAULT_CODEX_MARKETPLACE_ID.into(),
        source: DEFAULT_CODEX_MARKETPLACE_SOURCE.into(),
        plugins: default_codex_marketplace_plugins(),
    }]
}

pub(crate) fn default_codex_marketplace_plugins() -> Vec<String> {
    DEFAULT_CODEX_MARKETPLACE_PLUGINS
        .iter()
        .map(|plugin| (*plugin).into())
        .collect()
}

fn validate_config(config: &RepoConfig) -> Result<()> {
    validate_web_package_manager(&config.web_package_manager)?;
    validate_dev_config(config)?;
    validate_work_config(config)
}

fn validate_web_package_manager(value: &str) -> Result<()> {
    match value {
        "bun" | "pnpm" | "npm" | "yarn" => Ok(()),
        _ => bail!(
            "Unsupported web_package_manager '{value}'. Expected one of: bun, pnpm, npm, yarn."
        ),
    }
}

fn validate_dev_config(config: &RepoConfig) -> Result<()> {
    if !config.frontend_apps.is_empty() && !config.dev.apps.is_empty() {
        bail!(
            "[dev.apps] and legacy [[frontend_apps]] cannot both be configured. Move legacy entries into [[dev.apps]] or remove them."
        );
    }
    let mut app_names = HashSet::new();
    for app in &config.dev.apps {
        if !app_names.insert(app.name.as_str()) {
            bail!("Duplicate dev app name '{}' in [[dev.apps]]", app.name);
        }
    }
    Ok(())
}

fn validate_work_config(config: &RepoConfig) -> Result<()> {
    if let Some(refinement) = config.work.refinements.first() {
        bail!(
            "work.refinements is not supported yet (first unsupported refinement: {}). Remove work.refinements until refinement execution is implemented.",
            refinement.id
        );
    }

    Ok(())
}

fn gate_id_from_tool_name(tool: &str) -> String {
    tool.strip_prefix("jig.")
        .unwrap_or(tool)
        .replace(['_', '.'], "-")
}

fn unique_gate_id(base: String, existing_ids: &mut HashSet<String>) -> String {
    if existing_ids.insert(base.clone()) {
        return base;
    }

    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if existing_ids.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded gate id search should always find an unused suffix")
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
fn find_optional_repo_root() -> Result<Option<PathBuf>> {
    find_optional_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    let Some(root) = find_optional_repo_root_from(start)? else {
        bail!("Could not find repo root containing .jig.toml");
    };
    Ok(root)
}

fn find_optional_repo_root_from(start: &Path) -> Result<Option<PathBuf>> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".jig.toml").exists() {
            return Ok(Some(current));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn resolve_current_session_path(root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--git-path", CURRENT_SESSION_FILE])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !resolved.is_empty() {
                let path = PathBuf::from(&resolved);
                return Ok(if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                });
            }
        }
    }

    Ok(root.join(".agent/.cache").join(CURRENT_SESSION_FILE))
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_text = fs::read_to_string(root.join(".jig.toml"))?;
        let config: RepoConfig = toml::from_str(&config_text)?;
        validate_config(&config)?;
        let manifest_text = fs::read_to_string(root.join(".agent/jig-contract.json"))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)?;
        Ok(Self {
            root: root.to_path_buf(),
            current_session_path: root.join(".agent/.cache").join(CURRENT_SESSION_FILE),
            config,
            manifest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::{EnvVarGuard, lock_env};
    use serde_json::json;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn runtime_commands_still_require_adopted_repo_context() {
        let temp = tempdir().unwrap();
        let error = find_repo_root_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("Could not find repo root containing .jig.toml"));
    }

    #[test]
    fn load_optional_returns_none_outside_adopted_repo() {
        let _guard = CWD_LOCK.lock().unwrap();
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();
        let result = RepoContext::load_optional();
        std::env::set_current_dir(original).unwrap();
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn load_optional_ignores_stale_jig_repo_root() {
        let _guard = CWD_LOCK.lock().unwrap();
        let _env = lock_env();
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing");
        let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing);
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();
        let result = RepoContext::load_optional();
        std::env::set_current_dir(original).unwrap();

        assert!(result.unwrap().is_none());
    }

    #[test]
    fn legacy_work_checks_become_required_check_gates() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[work]
checks = ["jig.contract_check"]
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [
                    {
                        "name": "jig.contract_check",
                        "kind": "make",
                        "description": "Run make contract-check.",
                        "target": "contract-check"
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let gates = ctx.work_gates();
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].id, "contract-check");
        assert_eq!(gates[0].kind, "check");
        assert_eq!(gates[0].tool.as_deref(), Some("jig.contract_check"));
        assert!(gates[0].required);
    }

    #[test]
    fn missing_agent_tooling_uses_jig_skills_defaults() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let marketplaces = ctx.codex_marketplaces();
        assert_eq!(marketplaces.len(), 1);
        assert_eq!(marketplaces[0].id, "jig-skills");
        assert_eq!(marketplaces[0].source, "bpcakes/jig-skills");
        assert_eq!(
            marketplaces[0].plugins,
            vec![
                "jig-rust@jig-skills",
                "jig-swift@jig-skills",
                "jig-typescript@jig-skills",
                "jig-exec-plans@jig-skills",
            ]
        );
    }

    #[test]
    fn explicit_agent_tooling_config_is_loaded() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"
plugins = ["local-rust@local-skills"]
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let marketplaces = ctx.codex_marketplaces();
        assert_eq!(marketplaces.len(), 1);
        assert_eq!(marketplaces[0].id, "local-skills");
        assert_eq!(marketplaces[0].source, "../jig-skills");
        assert_eq!(marketplaces[0].plugins, vec!["local-rust@local-skills"]);
    }

    #[test]
    fn dev_config_defaults_and_apps_are_loaded() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
dev_command = "cargo run"
web_package_manager = "pnpm"

[dev]
proxy_port = 1555
https = true
workspace_discovery = true

[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4545

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["pnpm", "run", "dev"]
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        assert_eq!(ctx.web_package_manager(), "pnpm");
        assert_eq!(ctx.dev_config().proxy_port, 1555);
        assert!(ctx.dev_config().https);
        assert!(ctx.dev_config().workspace_discovery);
        assert_eq!(ctx.dev_config().apps.len(), 2);
        assert_eq!(ctx.dev_config().apps[0].name, "api");
        assert_eq!(ctx.dev_config().apps[0].port, Some(4545));
        assert_eq!(ctx.dev_config().apps[1].argv, vec!["pnpm", "run", "dev"]);
    }

    #[test]
    fn duplicate_dev_app_names_are_rejected_at_config_load() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
command = "bun run dev"

[[dev.apps]]
name = "web"
command = "bun run dev"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

        assert!(error.contains("Duplicate dev app name"));
    }

    #[test]
    fn unsupported_web_package_manager_is_rejected() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "/tmp/run-anything"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

        assert!(error.contains("Unsupported web_package_manager"));
    }

    #[test]
    fn template_dev_defaults_match_runtime_defaults() {
        let template = include_str!("../../../templates/project/.jig.toml.jinja");
        let defaults = DevConfig::default();

        assert!(template.contains(&format!("proxy_port = {}", defaults.proxy_port)));
        assert!(template.contains(&format!("https_port = {}", defaults.https_port.unwrap())));
        assert!(template.contains(&format!("https = {}", defaults.https)));
        assert!(template.contains(&format!("http2 = {}", defaults.http2)));
        assert!(template.contains(&format!("lan = {}", defaults.lan)));
        assert!(template.contains(&format!(r#"tld = "{}""#, defaults.tld)));
        assert!(template.contains(&format!(
            "workspace_discovery = {}",
            defaults.workspace_discovery
        )));
    }

    #[test]
    fn unknown_dev_config_fields_are_rejected() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
proxy_port = 1555
proxy_por = 1556
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("unknown field"));
        assert!(error.contains("proxy_por"));
    }

    #[test]
    fn unknown_dev_app_config_fields_are_rejected() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
command = "bun run dev"
commmand = "typo"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("unknown field"));
        assert!(error.contains("commmand"));
    }

    #[test]
    fn unknown_top_level_config_fields_are_rejected() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
proxy_porrt = 1355
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("unknown field"));
        assert!(error.contains("proxy_porrt"));
    }

    #[test]
    fn legacy_work_checks_are_merged_with_explicit_gates() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[work]
checks = ["jig.contract_check", "jig.test"]

[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"
required = false
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check", "test"],
                "optional_make_targets": [],
                "tools": [
                    {
                        "name": "jig.contract_check",
                        "kind": "make",
                        "description": "Run make contract-check.",
                        "target": "contract-check"
                    },
                    {
                        "name": "jig.test",
                        "kind": "make",
                        "description": "Run make test.",
                        "target": "test"
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let gates = ctx.work_gates();
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].id, "contract");
        assert_eq!(gates[0].tool.as_deref(), Some("jig.contract_check"));
        assert!(!gates[0].required);
        assert_eq!(gates[1].id, "test");
        assert_eq!(gates[1].tool.as_deref(), Some("jig.test"));
        assert!(gates[1].required);
    }

    #[test]
    fn unsupported_work_refinements_are_rejected() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".agent")).unwrap();
        fs::write(
            temp.path().join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[work.refinements]]
id = "rust-simplify"
skill = "jig-rust:rust-simplify"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["contract-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();

        let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("work.refinements is not supported yet"));
        assert!(error.contains("rust-simplify"));
    }
}
