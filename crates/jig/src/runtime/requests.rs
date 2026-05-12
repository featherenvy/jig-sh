use std::path::PathBuf;

use anyhow::Result;

use crate::cli::{
    WorkAppendOpts, WorkDecisionAddOpts, WorkFinishOpts, WorkReceiptsOpts, WorkStartOpts,
};
use crate::state::{
    DecisionAddRequest, PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, ReceiptListFilter,
    SessionEndRequest,
};
use crate::tool_defs::{
    JsonObject, args, bool_arg, required_string_arg, string_arg, string_list_arg, usize_arg,
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

pub(super) fn plan_open_request_from_args(args_obj: &JsonObject) -> Result<PlanOpenRequest> {
    Ok(PlanOpenRequest {
        title: required_string_arg(args_obj, args::TITLE)?,
        body: string_arg(args_obj, args::BODY),
        body_file: string_arg(args_obj, args::BODY_FILE).map(PathBuf::from),
    })
}

pub(super) fn session_end_request_for_finish(outcome: Option<String>) -> SessionEndRequest {
    SessionEndRequest {
        session_id: None,
        outcome,
    }
}

pub(super) fn plan_append_request_from_args(args_obj: &JsonObject) -> Result<PlanAppendRequest> {
    Ok(PlanAppendRequest {
        plan_id: required_string_arg(args_obj, args::PLAN_ID)?,
        body: string_arg(args_obj, args::BODY),
        body_file: string_arg(args_obj, args::BODY_FILE).map(PathBuf::from),
    })
}

pub(super) fn receipt_list_filter_from_args(
    args_obj: &JsonObject,
    default_limit: usize,
) -> ReceiptListFilter {
    ReceiptListFilter {
        session_id: string_arg(args_obj, args::SESSION_ID),
        plan_id: string_arg(args_obj, args::PLAN_ID),
        tool_name: string_arg(args_obj, args::TOOL_NAME),
        failed_only: bool_arg(args_obj, args::FAILED_ONLY).unwrap_or_default(),
        limit: usize_arg(args_obj, args::LIMIT).unwrap_or(default_limit),
    }
}

pub(super) fn decision_add_request_from_args(args_obj: &JsonObject) -> Result<DecisionAddRequest> {
    Ok(DecisionAddRequest {
        title: required_string_arg(args_obj, args::TITLE)?,
        selected_option: required_string_arg(args_obj, args::SELECTED_OPTION)?,
        rationale: required_string_arg(args_obj, args::RATIONALE)?,
        alternatives: string_list_arg(args_obj, args::ALTERNATIVES),
        plan_id: string_arg(args_obj, args::PLAN_ID),
    })
}
