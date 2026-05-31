//! Runtime state command DTOs.

#[derive(Debug)]
pub(crate) enum StateCommand {
    Summary,
    Archive(StateArchiveRequest),
}

#[derive(Debug)]
pub(crate) struct StateArchiveRequest {
    pub(crate) before: String,
    pub(crate) dry_run: bool,
}
