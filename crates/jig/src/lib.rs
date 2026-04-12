mod bootstrap;
mod cli;
mod context;
mod git_receipts;
mod mcp;
mod runtime;
mod state;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
