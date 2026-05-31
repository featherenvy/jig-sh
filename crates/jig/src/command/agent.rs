//! Agent command DTOs.

#[derive(Debug)]
pub(crate) enum AgentCommand {
    Doctor,
    Bootstrap(AgentBootstrapRequest),
}

#[derive(Debug)]
pub(crate) struct AgentBootstrapRequest {
    pub(crate) marketplace: Option<String>,
}
