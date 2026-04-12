use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::{bootstrap, context::RepoContext, mcp, runtime};

#[derive(Debug, Parser)]
#[command(
    name = "jig",
    version,
    about = "Repo-local agent runtime and bootstrapper for jig.sh"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CommandKind {
    #[command(name = "init")]
    Init(bootstrap::InitOpts),
    #[command(name = "adopt")]
    Adopt(bootstrap::AdoptOpts),
    #[command(name = "update")]
    Update(bootstrap::UpdateOpts),
    #[command(name = "fmt-check")]
    FmtCheck(ToolOpts),
    #[command(name = "clippy")]
    Clippy(ToolOpts),
    #[command(name = "test")]
    Test(ToolOpts),
    #[command(name = "test-locked")]
    TestLocked(ToolOpts),
    #[command(name = "sqlx-check")]
    SqlxCheck(ToolOpts),
    #[command(name = "schema-check")]
    SchemaCheck(ToolOpts),
    #[command(name = "schema-dump")]
    SchemaDump(ToolOpts),
    #[command(name = "migration-add")]
    MigrationAdd(MigrationAddOpts),
    #[command(name = "contract-check")]
    ContractCheck(ToolOpts),
    #[command(name = "run-target")]
    RunTarget(RunTargetOpts),
    #[command(name = "session-start")]
    SessionStart,
    #[command(name = "session-end")]
    SessionEnd(SessionEndOpts),
    #[command(name = "plans-open")]
    PlansOpen(PlanOpenOpts),
    #[command(name = "plans-append")]
    PlansAppend(PlanAppendOpts),
    #[command(name = "plans-close")]
    PlansClose(PlanCloseOpts),
    #[command(name = "receipts-list")]
    ReceiptsList(ReceiptsListOpts),
    #[command(name = "decisions-add")]
    DecisionsAdd(DecisionAddOpts),
    #[command(name = "mcp")]
    Mcp,
}

#[derive(Debug, Args, Clone, Default)]
pub(crate) struct ToolOpts {
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrationAddOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Debug, Args)]
pub(crate) struct RunTargetOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Debug, Args)]
pub(crate) struct SessionEndOpts {
    #[arg(long)]
    pub(crate) session_id: Option<String>,
    #[arg(long)]
    pub(crate) outcome: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct PlanOpenOpts {
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PlanAppendOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct PlanCloseOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) resolution: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ReceiptsListOpts {
    #[arg(long)]
    pub(crate) session_id: Option<String>,
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub(crate) limit: usize,
}

#[derive(Debug, Args)]
pub(crate) struct DecisionAddOpts {
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) selected_option: String,
    #[arg(long)]
    pub(crate) rationale: String,
    #[arg(long)]
    pub(crate) alternatives: Vec<String>,
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
}

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        CommandKind::Init(opts) => print_json(&bootstrap::run_init(opts)?),
        CommandKind::Adopt(opts) => print_json(&bootstrap::run_adopt(opts)?),
        CommandKind::Update(opts) => print_json(&bootstrap::run_update(opts)?),
        CommandKind::Mcp => {
            let ctx = RepoContext::load()?;
            mcp::serve(&ctx)
        }
        other => {
            let ctx = RepoContext::load()?;
            let output = runtime::dispatch(&ctx, other)?;
            print_json(&output)
        }
    }
}

fn print_json(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    handle.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_command_with_repeatable_flags() {
        let cli = Cli::try_parse_from([
            "jig",
            "init",
            "/tmp/demo",
            "--template",
            "/tmp/template",
            "--template-mode",
            "working-tree",
            "--repo-name",
            "demo",
            "--rust-migration-dir",
            "migrations",
            "--rust-crate-root",
            "crates",
            "--rust-crate-root",
            "libs",
            "--frontend-app",
            "frontend:web:40",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Init(bootstrap::InitOpts {
                template_mode,
                answers,
                ..
            }) => {
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::WorkingTree));
                assert_eq!(answers.rust_crate_roots, vec!["crates", "libs"]);
                assert_eq!(answers.frontend_apps.len(), 1);
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn parses_update_recopy_flag() {
        let cli = Cli::try_parse_from([
            "jig",
            "update",
            "--recopy",
            "--template",
            "/tmp/template",
            "--template-mode",
            "committed",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Update(bootstrap::UpdateOpts {
                recopy,
                template,
                template_mode,
                ..
            }) => {
                assert!(recopy);
                assert_eq!(template.as_deref(), Some("/tmp/template"));
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
            }
            other => panic!("expected update command, got {other:?}"),
        }
    }
}
