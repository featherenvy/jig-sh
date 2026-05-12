use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use ulid::Ulid;

use crate::context::RepoContext;
use crate::git_receipts::DiffStat;

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct SessionEvent {
    pub(super) id: String,
    pub(super) session_id: String,
    pub(super) event: String,
    pub(super) timestamp_ms: u64,
    pub(super) outcome: Option<String>,
    pub(super) summary: Option<Value>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct PlanEvent {
    pub(super) id: String,
    pub(super) plan_id: String,
    pub(super) event: String,
    pub(super) timestamp_ms: u64,
    pub(super) title: Option<String>,
    pub(super) body_path: Option<String>,
    pub(super) resolution: Option<String>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct ReceiptRecord {
    pub(super) id: String,
    pub(super) session_id: Option<String>,
    pub(super) plan_id: Option<String>,
    pub(super) tool_name: String,
    pub(super) args: Value,
    pub(super) invoked_make_target: Option<String>,
    pub(super) started_at_ms: u64,
    pub(super) ended_at_ms: u64,
    pub(super) exit_status: i32,
    pub(super) stdout_preview: String,
    pub(super) stderr_preview: String,
    pub(super) changed_paths: Vec<String>,
    pub(super) diff_stat: DiffStat,
    #[serde(default)]
    pub(super) git_status_error: Option<String>,
    #[serde(default)]
    pub(super) git_diff_stat_error: Option<String>,
    #[serde(default)]
    pub(super) worktree_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct DecisionRecord {
    pub(super) id: String,
    pub(super) session_id: Option<String>,
    pub(super) plan_id: Option<String>,
    pub(super) title: String,
    pub(super) selected_option: String,
    pub(super) rationale: String,
    pub(super) alternatives: Vec<String>,
    pub(super) timestamp_ms: u64,
}

pub(super) fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.lock_exclusive()?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    file.unlock()?;
    Ok(())
}

pub(super) fn append_text(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.lock_exclusive()?;
    file.write_all(content)?;
    file.sync_data()?;
    file.unlock()?;
    Ok(())
}

pub(super) fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(&line).with_context(|| {
            format!(
                "Failed to parse JSONL record {} in {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(value);
    }
    Ok(items)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn truncate(value: &str) -> String {
    const LIMIT: usize = 4000;
    if value.len() <= LIMIT {
        value.to_string()
    } else {
        let mut end = LIMIT;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

pub(super) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::new())
}

pub(super) fn ensure_state_layout(ctx: &RepoContext) -> Result<()> {
    fs::create_dir_all(ctx.state_dir())?;
    fs::create_dir_all(ctx.root().join(".agent/plans"))?;
    if let Some(parent) = ctx.current_session_path().parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub(super) fn rel_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .display()
        .to_string())
}
