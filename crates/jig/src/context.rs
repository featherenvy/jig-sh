use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const CURRENT_SESSION_FILE: &str = "jig-current-session.txt";

#[derive(Debug, Clone, Deserialize)]
struct RepoConfig {
    #[serde(rename = "_src_path")]
    src_path: String,
    #[serde(rename = "_commit")]
    commit: String,
    repo_name: String,
    default_branch: String,
    jig_version: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ContractManifest {
    contract_version: u32,
    memory_schema_version: u32,
    tool_namespace: String,
    jig_version: String,
    required_make_targets: Vec<String>,
    #[allow(dead_code)]
    optional_make_targets: Vec<String>,
    tools: Vec<ManifestTool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ManifestTool {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) description: String,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepoContext {
    root: PathBuf,
    current_session_path: PathBuf,
    config: RepoConfig,
    manifest: ContractManifest,
}

impl RepoContext {
    pub(crate) fn load() -> Result<Self> {
        let root = find_repo_root()?;
        let config_path = root.join(".jig.yml");
        let config_text = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let config: RepoConfig = serde_yaml::from_str(&config_text)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;

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
        if manifest.memory_schema_version != 1 {
            bail!(
                "Unsupported jig memory schema version: {}",
                manifest.memory_schema_version
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
                "jig version mismatch between .jig.yml ({}) and manifest ({})",
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

fn find_repo_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("JIG_REPO_ROOT") {
        return Ok(PathBuf::from(root));
    }

    find_repo_root_from(&std::env::current_dir()?)
}

pub(crate) fn find_repo_root_from(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".jig.yml").exists() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("Could not find repo root containing .jig.yml");
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
        let config_text = fs::read_to_string(root.join(".jig.yml"))?;
        let config: RepoConfig = serde_yaml::from_str(&config_text)?;
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
    use tempfile::tempdir;

    #[test]
    fn runtime_commands_still_require_adopted_repo_context() {
        let temp = tempdir().unwrap();
        let error = find_repo_root_from(temp.path()).unwrap_err().to_string();
        assert!(error.contains("Could not find repo root containing .jig.yml"));
    }
}
