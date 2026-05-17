use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;
use ulid::Ulid;

use crate::context::RepoContext;
use crate::git_receipts::DiffStat;

#[derive(Clone, Debug)]
pub(super) enum SessionEvent {
    Start {
        id: String,
        session_id: String,
        timestamp_ms: u64,
        summary: Value,
    },
    End {
        id: String,
        session_id: String,
        timestamp_ms: u64,
        outcome: Option<String>,
    },
    Unknown {
        id: String,
        session_id: String,
        event: String,
        timestamp_ms: u64,
    },
}

impl SessionEvent {
    pub(super) fn start(id: String, session_id: String, timestamp_ms: u64, summary: Value) -> Self {
        Self::Start {
            id,
            session_id,
            timestamp_ms,
            summary,
        }
    }

    pub(super) fn end(
        id: String,
        session_id: String,
        timestamp_ms: u64,
        outcome: Option<String>,
    ) -> Self {
        Self::End {
            id,
            session_id,
            timestamp_ms,
            outcome,
        }
    }

    pub(super) fn is_start(&self) -> bool {
        matches!(self, Self::Start { .. })
    }

    pub(super) fn session_id(&self) -> &str {
        match self {
            Self::Start { session_id, .. }
            | Self::End { session_id, .. }
            | Self::Unknown { session_id, .. } => session_id,
        }
    }

    pub(super) fn timestamp_ms(&self) -> u64 {
        match self {
            Self::Start { timestamp_ms, .. }
            | Self::End { timestamp_ms, .. }
            | Self::Unknown { timestamp_ms, .. } => *timestamp_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum PlanEvent {
    Open {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
    },
    Append {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        body_path: Option<String>,
    },
    Close {
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        resolution: Option<String>,
    },
    Unknown {
        id: String,
        plan_id: String,
        event: String,
        timestamp_ms: u64,
    },
}

impl PlanEvent {
    pub(super) fn open(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        title: String,
        body_path: Option<String>,
    ) -> Self {
        Self::Open {
            id,
            plan_id,
            timestamp_ms,
            title,
            body_path,
        }
    }

    pub(super) fn append(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        body_path: Option<String>,
    ) -> Self {
        Self::Append {
            id,
            plan_id,
            timestamp_ms,
            body_path,
        }
    }

    pub(super) fn close(
        id: String,
        plan_id: String,
        timestamp_ms: u64,
        resolution: Option<String>,
    ) -> Self {
        Self::Close {
            id,
            plan_id,
            timestamp_ms,
            resolution,
        }
    }

    pub(super) fn plan_id(&self) -> &str {
        match self {
            Self::Open { plan_id, .. }
            | Self::Append { plan_id, .. }
            | Self::Close { plan_id, .. }
            | Self::Unknown { plan_id, .. } => plan_id,
        }
    }

    pub(super) fn timestamp_ms(&self) -> u64 {
        match self {
            Self::Open { timestamp_ms, .. }
            | Self::Append { timestamp_ms, .. }
            | Self::Close { timestamp_ms, .. }
            | Self::Unknown { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    pub(super) fn body_path(&self) -> Option<&str> {
        match self {
            Self::Open { body_path, .. } | Self::Append { body_path, .. } => body_path.as_deref(),
            Self::Close { .. } | Self::Unknown { .. } => None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        matches!(self, Self::Open { .. })
    }
}

#[derive(Serialize, Deserialize)]
struct LegacySessionEvent {
    id: String,
    session_id: String,
    event: String,
    timestamp_ms: u64,
    outcome: Option<String>,
    summary: Option<Value>,
}

impl Serialize for SessionEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let legacy = match self {
            Self::Start {
                id,
                session_id,
                timestamp_ms,
                summary,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: "start".into(),
                timestamp_ms: *timestamp_ms,
                outcome: None,
                summary: Some(summary.clone()),
            },
            Self::End {
                id,
                session_id,
                timestamp_ms,
                outcome,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: "end".into(),
                timestamp_ms: *timestamp_ms,
                outcome: outcome.clone(),
                summary: None,
            },
            Self::Unknown {
                id,
                session_id,
                event,
                timestamp_ms,
            } => LegacySessionEvent {
                id: id.clone(),
                session_id: session_id.clone(),
                event: event.clone(),
                timestamp_ms: *timestamp_ms,
                outcome: None,
                summary: None,
            },
        };
        legacy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let legacy = LegacySessionEvent::deserialize(deserializer)?;
        Ok(match legacy.event.as_str() {
            "start" => Self::start(
                legacy.id,
                legacy.session_id,
                legacy.timestamp_ms,
                legacy.summary.unwrap_or(Value::Null),
            ),
            "end" => Self::end(
                legacy.id,
                legacy.session_id,
                legacy.timestamp_ms,
                legacy.outcome,
            ),
            _ => Self::Unknown {
                id: legacy.id,
                session_id: legacy.session_id,
                event: legacy.event,
                timestamp_ms: legacy.timestamp_ms,
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
struct LegacyPlanEvent {
    id: String,
    plan_id: String,
    event: String,
    timestamp_ms: u64,
    title: Option<String>,
    body_path: Option<String>,
    resolution: Option<String>,
}

impl Serialize for PlanEvent {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let legacy = match self {
            Self::Open {
                id,
                plan_id,
                timestamp_ms,
                title,
                body_path,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "open".into(),
                timestamp_ms: *timestamp_ms,
                title: Some(title.clone()),
                body_path: body_path.clone(),
                resolution: None,
            },
            Self::Append {
                id,
                plan_id,
                timestamp_ms,
                body_path,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "append".into(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: body_path.clone(),
                resolution: None,
            },
            Self::Close {
                id,
                plan_id,
                timestamp_ms,
                resolution,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: "close".into(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: None,
                resolution: resolution.clone(),
            },
            Self::Unknown {
                id,
                plan_id,
                event,
                timestamp_ms,
            } => LegacyPlanEvent {
                id: id.clone(),
                plan_id: plan_id.clone(),
                event: event.clone(),
                timestamp_ms: *timestamp_ms,
                title: None,
                body_path: None,
                resolution: None,
            },
        };
        legacy.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PlanEvent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let legacy = LegacyPlanEvent::deserialize(deserializer)?;
        Ok(match legacy.event.as_str() {
            "open" => Self::open(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.title.unwrap_or_else(|| "Untitled plan".into()),
                legacy.body_path,
            ),
            "append" => Self::append(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.body_path,
            ),
            "close" => Self::close(
                legacy.id,
                legacy.plan_id,
                legacy.timestamp_ms,
                legacy.resolution,
            ),
            _ => Self::Unknown {
                id: legacy.id,
                plan_id: legacy.plan_id,
                event: legacy.event,
                timestamp_ms: legacy.timestamp_ms,
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct ReceiptRecord {
    pub(super) id: String,
    pub(super) session_id: Option<String>,
    pub(super) plan_id: Option<String>,
    pub(super) tool_name: String,
    pub(super) args: Value,
    #[serde(default)]
    pub(super) invoked_command_key: Option<String>,
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
    FileExt::unlock(&file)?;
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
    FileExt::unlock(&file)?;
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
