//! Migration command DTOs.

#[derive(Debug)]
pub(crate) struct MigrationAddRequest {
    pub(crate) name: String,
    pub(crate) tool: super::ToolRequest,
}
