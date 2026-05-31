//! Prompt command DTOs.

use std::path::PathBuf;

pub(crate) const PROMPT_BODY_KEY: &str = "body";

#[derive(Debug)]
pub(crate) enum PromptCommand {
    Get(PromptRenderRequest),
    Copy(PromptRenderRequest),
    Add(PromptAddRequest),
    Edit(PromptEditRequest),
    Remove(PromptNameRequest),
    List(PromptListRequest),
    Search(PromptSearchRequest),
    Export(PromptExportRequest),
    Import(PromptImportRequest),
}

#[derive(Debug)]
pub(crate) struct PromptRenderRequest {
    pub(crate) name: String,
    pub(crate) vars: Vec<String>,
    pub(crate) raw: bool,
}

#[derive(Debug)]
pub(crate) struct PromptAddRequest {
    pub(crate) name: String,
    pub(crate) body: Option<String>,
    pub(crate) file: Option<PathBuf>,
    pub(crate) description: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) use_editor: bool,
}

#[derive(Debug)]
pub(crate) struct PromptEditRequest {
    pub(crate) name: String,
    pub(crate) open_editor: bool,
}

#[derive(Debug)]
pub(crate) struct PromptNameRequest {
    pub(crate) name: String,
}

#[derive(Debug)]
pub(crate) struct PromptListRequest {
    pub(crate) include_packs: bool,
}

#[derive(Debug)]
pub(crate) struct PromptSearchRequest {
    pub(crate) query: String,
    pub(crate) include_body: bool,
}

#[derive(Debug)]
pub(crate) struct PromptExportRequest {
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct PromptImportRequest {
    pub(crate) file: PathBuf,
}
