mod bootstrap;
mod cli;
mod context;
mod git_receipts;
mod mcp;
mod process;
mod runtime;
mod state;
#[cfg(test)]
mod test_env;
mod tool_defs;

pub fn run() -> anyhow::Result<()> {
    cli::run()
}
