use clap::{Args, Subcommand};

use crate::tool_defs;

pub(super) const STATE_ARCHIVE_AFTER_HELP: &str = "\
Archive old receipt records out of .agent/state/receipts.jsonl while retaining
latest gate evidence and supporting receipts. --before accepts YYYY-MM-DD
interpreted as UTC midnight, or a Unix millisecond timestamp.

Examples:
  jig state summary
  jig state archive --before 2026-01-01
  jig state archive --before 2026-01-01 --dry-run";

#[derive(Debug, Subcommand)]
pub(crate) enum StateCommand {
    /// Summarize runtime-owned Jig state.
    #[command(name = tool_defs::cli_command::STATE_SUMMARY)]
    Summary,
    /// Archive old receipt records while preserving latest gate evidence.
    #[command(
        name = tool_defs::cli_command::STATE_ARCHIVE,
        after_help = STATE_ARCHIVE_AFTER_HELP
    )]
    Archive(StateArchiveOpts),
}

#[derive(Args, Debug)]
pub(crate) struct StateArchiveOpts {
    #[arg(
        long,
        help = "Archive receipts older than YYYY-MM-DD UTC or a Unix millisecond timestamp"
    )]
    pub(crate) before: String,

    #[arg(long, help = "Report what would be archived without rewriting state")]
    pub(crate) dry_run: bool,
}
