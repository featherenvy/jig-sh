use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[cfg(test)]
use crate::tool_defs::tool;
use crate::{
    bootstrap, context::RepoContext, mcp, runtime, state::DEFAULT_RECEIPTS_LIMIT, tool_defs,
};

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
    #[command(name = tool_defs::cli_command::INIT)]
    Init(bootstrap::InitOpts),
    #[command(name = tool_defs::cli_command::ADOPT)]
    Adopt(bootstrap::AdoptOpts),
    #[command(name = tool_defs::cli_command::UPDATE)]
    Update(bootstrap::UpdateOpts),
    #[command(name = tool_defs::cli_command::FMT_CHECK)]
    FmtCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::CLIPPY)]
    Clippy(ToolOpts),
    #[command(name = tool_defs::cli_command::TEST)]
    Test(ToolOpts),
    #[command(name = tool_defs::cli_command::TEST_LOCKED)]
    TestLocked(ToolOpts),
    #[command(name = tool_defs::cli_command::SQLX_CHECK)]
    SqlxCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::SCHEMA_CHECK)]
    SchemaCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::SCHEMA_DUMP)]
    SchemaDump(ToolOpts),
    #[command(name = tool_defs::cli_command::MIGRATION_ADD)]
    MigrationAdd(MigrationAddOpts),
    #[command(name = tool_defs::cli_command::CONTRACT_CHECK)]
    ContractCheck(ToolOpts),
    #[command(name = tool_defs::cli_command::RUN_TARGET)]
    RunTarget(RunTargetOpts),
    #[command(name = tool_defs::cli_command::AGENT, subcommand)]
    Agent(AgentCommand),
    #[command(name = tool_defs::cli_command::WORK, subcommand)]
    Work(WorkCommand),
    #[command(name = tool_defs::cli_command::MCP)]
    Mcp,
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkCommand {
    #[command(name = tool_defs::cli_command::WORK_START)]
    Start(WorkStartOpts),
    #[command(name = tool_defs::cli_command::WORK_APPEND)]
    Append(WorkAppendOpts),
    #[command(name = tool_defs::cli_command::WORK_CHECK)]
    Check(WorkCheckOpts),
    #[command(name = tool_defs::cli_command::WORK_GATES)]
    Gates(WorkGatesOpts),
    #[command(name = tool_defs::cli_command::WORK_DECIDE)]
    Decide(WorkDecisionAddOpts),
    #[command(name = tool_defs::cli_command::WORK_RECEIPTS)]
    Receipts(WorkReceiptsOpts),
    #[command(name = tool_defs::cli_command::WORK_STATUS)]
    Status,
    #[command(name = tool_defs::cli_command::WORK_FINISH)]
    Finish(WorkFinishOpts),
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    #[command(name = tool_defs::cli_command::AGENT_DOCTOR)]
    Doctor,
    #[command(name = tool_defs::cli_command::AGENT_BOOTSTRAP)]
    Bootstrap(AgentBootstrapOpts),
}

#[derive(Args, Debug)]
pub(crate) struct AgentBootstrapOpts {
    #[arg(long)]
    pub(crate) marketplace: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub(crate) struct ToolOpts {
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct MigrationAddOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct RunTargetOpts {
    pub(crate) name: String,
    #[command(flatten)]
    pub(crate) tool: ToolOpts,
}

#[derive(Args, Debug)]
pub(crate) struct WorkStartOpts {
    #[arg(long)]
    pub(crate) title: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkAppendOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) body: Option<String>,
    #[arg(long)]
    pub(crate) body_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkCheckOpts {
    #[arg(long)]
    pub(crate) plan_id: String,

    #[arg(long = "tool")]
    pub(crate) tools: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkGatesOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
}

#[derive(Args, Debug)]
pub(crate) struct WorkFinishOpts {
    #[arg(long)]
    pub(crate) plan_id: String,
    #[arg(long)]
    pub(crate) resolution: Option<String>,
    #[arg(long)]
    pub(crate) outcome: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct WorkReceiptsOpts {
    #[arg(long)]
    pub(crate) session_id: Option<String>,
    #[arg(long)]
    pub(crate) plan_id: Option<String>,
    #[arg(long)]
    pub(crate) tool_name: Option<String>,
    #[arg(long)]
    pub(crate) failed_only: bool,
    #[arg(long, default_value_t = DEFAULT_RECEIPTS_LIMIT)]
    pub(crate) limit: usize,
}

impl Default for WorkReceiptsOpts {
    fn default() -> Self {
        Self {
            session_id: None,
            plan_id: None,
            tool_name: None,
            failed_only: false,
            limit: DEFAULT_RECEIPTS_LIMIT,
        }
    }
}

#[derive(Args, Debug)]
pub(crate) struct WorkDecisionAddOpts {
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
            "committed",
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
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
                assert_eq!(answers.rust_crate_roots, vec!["crates", "libs"]);
                assert_eq!(answers.frontend_apps.len(), 1);
            }
            other => panic!("expected init command, got {other:?}"),
        }
    }

    #[test]
    fn rejects_working_tree_template_mode() {
        let error = Cli::try_parse_from([
            "jig",
            "init",
            "/tmp/demo",
            "--template",
            "/tmp/template",
            "--template-mode",
            "working-tree",
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("invalid value 'working-tree'"));
        assert!(error.contains("committed"));
    }

    #[test]
    fn parses_update_recopy_flag() {
        let cli = Cli::try_parse_from([
            "jig",
            "update",
            "--recopy",
            "--force",
            "--template",
            "/tmp/template",
            "--template-mode",
            "committed",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Update(bootstrap::UpdateOpts {
                recopy,
                force,
                template,
                template_mode,
                ..
            }) => {
                assert!(recopy);
                assert!(force);
                assert_eq!(template.as_deref(), Some("/tmp/template"));
                assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
            }
            other => panic!("expected update command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_receipts_filters() {
        let cli = Cli::try_parse_from([
            "jig",
            "work",
            "receipts",
            "--session-id",
            "session_1",
            "--plan-id",
            "plan_1",
            "--tool-name",
            tool::TEST,
            "--failed-only",
            "--limit",
            "5",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Receipts(opts)) => {
                assert_eq!(opts.session_id.as_deref(), Some("session_1"));
                assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
                assert_eq!(opts.tool_name.as_deref(), Some(tool::TEST));
                assert!(opts.failed_only);
                assert_eq!(opts.limit, 5);
            }
            other => panic!("expected work receipts command, got {other:?}"),
        }
    }

    #[test]
    fn parses_agent_doctor_command() {
        let cli = Cli::try_parse_from(["jig", "agent", "doctor"]).unwrap();

        match cli.command {
            CommandKind::Agent(AgentCommand::Doctor) => {}
            other => panic!("expected agent doctor command, got {other:?}"),
        }
    }

    #[test]
    fn parses_agent_bootstrap_marketplace() {
        let cli = Cli::try_parse_from([
            "jig",
            "agent",
            "bootstrap",
            "--marketplace",
            "../jig-skills",
        ])
        .unwrap();

        match cli.command {
            CommandKind::Agent(AgentCommand::Bootstrap(opts)) => {
                assert_eq!(opts.marketplace.as_deref(), Some("../jig-skills"));
            }
            other => panic!("expected agent bootstrap command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_status_command() {
        let cli = Cli::try_parse_from(["jig", "work", "status"]).unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Status) => {}
            other => panic!("expected work status command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_check_tools() {
        let cli = Cli::try_parse_from([
            "jig",
            "work",
            "check",
            "--plan-id",
            "plan_1",
            "--tool",
            tool::CONTRACT_CHECK,
            "--tool",
            tool::TEST,
        ])
        .unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Check(opts)) => {
                assert_eq!(opts.plan_id, "plan_1");
                assert_eq!(opts.tools, vec![tool::CONTRACT_CHECK, tool::TEST]);
            }
            other => panic!("expected work check command, got {other:?}"),
        }
    }

    #[test]
    fn parses_work_gates_command() {
        let cli = Cli::try_parse_from(["jig", "work", "gates", "--plan-id", "plan_1"]).unwrap();

        match cli.command {
            CommandKind::Work(WorkCommand::Gates(opts)) => {
                assert_eq!(opts.plan_id, "plan_1");
            }
            other => panic!("expected work gates command, got {other:?}"),
        }
    }
}
