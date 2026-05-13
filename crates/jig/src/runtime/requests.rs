use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::cli::{
    WorkAppendOpts, WorkDecisionAddOpts, WorkFinishOpts, WorkGoalOpts, WorkReceiptsOpts,
    WorkStartOpts,
};
use crate::state::{
    DecisionAddRequest, PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, ReceiptListFilter,
    SessionEndRequest,
};

impl From<WorkStartOpts> for PlanOpenRequest {
    fn from(opts: WorkStartOpts) -> Self {
        Self {
            title: opts.title,
            body: opts.body,
            body_file: opts.body_file,
        }
    }
}

impl From<WorkGoalOpts> for WorkGoalRequest {
    fn from(opts: WorkGoalOpts) -> Self {
        Self {
            objective: opts.objective,
            success: opts.success,
            validations: opts.validations,
            constraints: opts.constraints,
            checkpoints: opts.checkpoints,
            title: opts.title,
            notes: opts.notes,
        }
    }
}

impl From<WorkAppendOpts> for PlanAppendRequest {
    fn from(opts: WorkAppendOpts) -> Self {
        Self {
            plan_id: opts.plan_id,
            body: opts.body,
            body_file: opts.body_file,
        }
    }
}

impl From<&WorkFinishOpts> for PlanCloseRequest {
    fn from(opts: &WorkFinishOpts) -> Self {
        Self {
            plan_id: opts.plan_id.clone(),
            resolution: opts.resolution.clone(),
        }
    }
}

impl From<WorkReceiptsOpts> for ReceiptListFilter {
    fn from(opts: WorkReceiptsOpts) -> Self {
        Self {
            session_id: opts.session_id,
            plan_id: opts.plan_id,
            tool_name: opts.tool_name,
            failed_only: opts.failed_only,
            limit: opts.limit,
        }
    }
}

impl From<WorkDecisionAddOpts> for DecisionAddRequest {
    fn from(opts: WorkDecisionAddOpts) -> Self {
        Self {
            title: opts.title,
            selected_option: opts.selected_option,
            rationale: opts.rationale,
            alternatives: opts.alternatives,
            plan_id: opts.plan_id,
        }
    }
}

pub(super) fn request_from_args<T>(args: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(args).context("Invalid work tool arguments")
}

pub(super) fn session_end_request_for_finish(outcome: Option<String>) -> SessionEndRequest {
    SessionEndRequest {
        session_id: None,
        outcome,
    }
}

#[derive(Deserialize)]
pub(super) struct WorkGoalRequest {
    pub(super) objective: String,
    pub(super) success: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(super) validations: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(super) constraints: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(super) checkpoints: Vec<String>,
    pub(super) title: Option<String>,
    pub(super) notes: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct WorkCheckRequest {
    pub(super) plan_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(super) tools: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct WorkGatesRequest {
    pub(super) plan_id: String,
}

#[derive(Deserialize)]
pub(super) struct WorkFinishRequest {
    pub(super) plan_id: String,
    pub(super) resolution: Option<String>,
    pub(super) outcome: Option<String>,
}

fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}
