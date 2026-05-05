mod bootstrap;
mod cli;
mod context;
mod git_receipts;
mod mcp;
mod process;
mod runtime;
mod state;
mod tool_defs;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
