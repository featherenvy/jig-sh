use std::process::{Command, ExitStatus, Output};

use anyhow::{Context, Result, bail};

pub(crate) fn run_checked_output(
    command: &mut Command,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<Output> {
    let output = command.output()?;
    require_success(&output, failure_message)?;
    Ok(output)
}

pub(crate) fn run_checked_output_with_context(
    command: &mut Command,
    start_context: impl FnOnce() -> String,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<Output> {
    let output = command.output().with_context(start_context)?;
    require_success(&output, failure_message)?;
    Ok(output)
}

pub(crate) fn run_checked_stdout_trimmed(
    command: &mut Command,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<String> {
    let output = run_checked_output(command, failure_message)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn require_success(
    output: &Output,
    failure_message: impl FnOnce(&Output) -> String,
) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        bail!("{}", failure_message(output))
    }
}

pub(crate) fn format_exit_status(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => "termination by signal".to_string(),
    }
}
