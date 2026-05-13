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
pub(crate) const DEFAULT_CODEX_MARKETPLACE_SOURCE: &str = "featherenvy/jig-skills";
pub(crate) const DEFAULT_CODEX_MARKETPLACE_PLUGINS: &[&str] = &[
    "jig-rust@jig-skills",
    "jig-swift@jig-skills",
    "jig-typescript@jig-skills",
    "jig-exec-plans@jig-skills",
];

#[derive(Clone, Debug, Deserialize)]
struct RepoConfig {
    #[serde(rename = "_src_path")]
    src_path: String,
    #[serde(rename = "_commit")]
    commit: String,
    repo_name: String,
    default_branch: String,
    jig_version: String,
    #[serde(default)]
    work: WorkConfig,
    #[serde(default)]
    agent_tooling: AgentToolingConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
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
pub(crate) struct AgentToolingConfig {
    #[serde(default)]
    pub(crate) codex: CodexToolingConfig,
}

#[derive(Clone, Debug, Deserialize)]
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
pub(crate) struct CodexMarketplaceConfig {
    pub(crate) id: String,
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) plugins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
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
    required_make_targets: Vec<String>,
    #[allow(dead_code)]
    optional_make_targets: Vec<String>,
    tools: Vec<ManifestTool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ManifestTool {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) description: String,
    pub(crate) target: Option<String>,
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
        let root = find_repo_root()?;
        let config_path = root.join(".jig.toml");
        let config_text = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let config: RepoConfig = toml::from_str(&config_text)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;
        validate_work_config(&config)?;

        let manifest_path = root.join(".agent/jig-contract.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        let manifest: ContractManifest = serde_json::from_str(&manifest_text)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

        if manifest.contract_version != 1 {
            bail!(
                "Unsupported jig contract version: {}",
                manifest.contract_version
            );
        }
        if manifest.tool_namespace != "jig" {
            bail!("Unsupported tool namespace: {}", manifest.tool_namespace);
        }
        if manifest.required_make_targets.is_empty() {
            bail!("jig contract manifest does not declare required make targets");
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

fn find_repo_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("JIG_REPO_ROOT") {
        return Ok(PathBuf::from(root));
    }

    find_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".jig.toml").exists() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("Could not find repo root containing .jig.toml");
        }
    }
}

fn resolve_current_session_path(root: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--git-path", CURRENT_SESSION_FILE])
        .output();

    if let Ok(output) = output
        && output.status.success()
    {
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

    Ok(root.join(".agent/.cache").join(CURRENT_SESSION_FILE))
}

#[cfg(test)]
impl RepoContext {
    pub(crate) fn load_from(root: &Path) -> Result<Self> {
        let config_text = fs::read_to_string(root.join(".jig.toml"))?;
        let config: RepoConfig = toml::from_str(&config_text)?;
        validate_work_config(&config)?;
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
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn runtime_commands_still_require_adopted_repo_context() {
        let temp = tempdir().unwrap();
        let error = find_repo_root_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("Could not find repo root containing .jig.toml"));
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
        assert_eq!(marketplaces[0].source, "featherenvy/jig-skills");
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
