use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use tempfile::NamedTempFile;
use wait_timeout::ChildExt;

use crate::context::{RepoContext, ReviewScopeArg, WorkReviewGate, parse_review_scope_arg};

const CODEX_BIN_ENV: &str = "JIG_CODEX_BIN";
const CODEX_TIMEOUT_ENV: &str = "JIG_CODEX_TIMEOUT_SECS";
const DEFAULT_CODEX_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub(super) struct CodexReviewCommandOutput {
    pub(super) output: Output,
    pub(super) codex_stdout: String,
}

pub(super) fn run_codex_review(
    ctx: &RepoContext,
    gate: &WorkReviewGate,
    prompt: &str,
    schema: &Value,
) -> Result<CodexReviewCommandOutput> {
    let schema_file = NamedTempFile::new().context("Failed to create review schema file")?;
    fs::write(
        schema_file.path(),
        serde_json::to_vec_pretty(schema).context("Failed to encode review schema JSON")?,
    )
    .context("Failed to write review schema file")?;
    let output_file = NamedTempFile::new().context("Failed to create review output file")?;
    let mut command = Command::new(codex_bin());
    command
        .current_dir(ctx.root())
        .arg("exec")
        .arg("review")
        .arg("--ephemeral");
    apply_review_scope(&mut command, gate)?;
    if let Some(model) = gate.model.as_deref() {
        command.arg("--model").arg(model);
    }
    command
        .arg("--output-schema")
        .arg(schema_file.path())
        .arg("-o")
        .arg(output_file.path())
        .arg(prompt);

    let output = run_codex_command(command, None).context("Failed to run Codex review")?;
    let codex_stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let output_metadata = output_file
        .path()
        .metadata()
        .context("Failed to inspect Codex review output file")?;
    if output_metadata.len() > 0 {
        let last_message =
            fs::read(output_file.path()).context("Failed to read Codex review output")?;
        let mut combined = output;
        combined.stdout = last_message;
        return Ok(CodexReviewCommandOutput {
            output: combined,
            codex_stdout,
        });
    }
    Ok(CodexReviewCommandOutput {
        output,
        codex_stdout,
    })
}

pub(super) fn run_codex_refine(
    ctx: &RepoContext,
    prompt: &str,
    model: Option<&str>,
) -> Result<Output> {
    let mut command = codex_refine_command(codex_bin(), ctx.root(), model);
    command.arg("-");
    run_codex_command(command, Some(prompt)).context("Failed to run Codex")
}

fn codex_refine_command(bin: String, root: &Path, model: Option<&str>) -> Command {
    let mut command = Command::new(bin);
    command
        .current_dir(root)
        .arg("--ask-for-approval")
        .arg("never")
        .arg("exec")
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--ephemeral");
    if let Some(model) = model {
        command.arg("--model").arg(model);
    }
    command
}

fn apply_review_scope(command: &mut Command, gate: &WorkReviewGate) -> Result<()> {
    match parse_review_scope_arg(&gate.scope)? {
        ReviewScopeArg::Uncommitted => {
            command.arg("--uncommitted");
        }
        ReviewScopeArg::Base(base) => {
            command.arg("--base").arg(base);
        }
        ReviewScopeArg::Commit(commit) => {
            command.arg("--commit").arg(commit);
        }
    }
    Ok(())
}

fn run_codex_command(mut command: Command, stdin_prompt: Option<&str>) -> Result<Output> {
    let stdout_file = NamedTempFile::new().context("Failed to create Codex stdout file")?;
    let stderr_file = NamedTempFile::new().context("Failed to create Codex stderr file")?;
    command
        .stdout(
            stdout_file
                .reopen()
                .context("Failed to open Codex stdout file")?,
        )
        .stderr(
            stderr_file
                .reopen()
                .context("Failed to open Codex stderr file")?,
        );
    if stdin_prompt.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().context("Failed to start Codex")?;
    let writer = if let Some(prompt) = stdin_prompt {
        let mut stdin = child.stdin.take().context("Failed to open Codex stdin")?;
        let prompt = prompt.to_owned();
        Some(thread::spawn(move || -> Result<()> {
            stdin
                .write_all(prompt.as_bytes())
                .context("Failed to write Codex refinement prompt")?;
            Ok(())
        }))
    } else {
        None
    };

    let timeout = codex_timeout()?;
    let status = match child
        .wait_timeout(timeout)
        .context("Failed to wait for Codex")?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Codex timed out after {} seconds", timeout.as_secs());
        }
    };

    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| anyhow!("Codex stdin writer thread panicked"))??;
    }

    Ok(Output {
        status,
        stdout: fs::read(stdout_file.path()).context("Failed to read Codex stdout")?,
        stderr: fs::read(stderr_file.path()).context("Failed to read Codex stderr")?,
    })
}

fn codex_timeout() -> Result<Duration> {
    let Ok(value) = env::var(CODEX_TIMEOUT_ENV) else {
        return Ok(DEFAULT_CODEX_TIMEOUT);
    };
    let seconds = value
        .parse::<u64>()
        .with_context(|| format!("Invalid {CODEX_TIMEOUT_ENV} value '{value}'"))?;
    if seconds == 0 {
        bail!("{CODEX_TIMEOUT_ENV} must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

fn codex_bin() -> String {
    env::var(CODEX_BIN_ENV).unwrap_or_else(|_| "codex".into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::codex_refine_command;

    #[test]
    fn codex_refine_approval_policy_is_a_top_level_codex_arg() {
        let command = codex_refine_command("codex".into(), Path::new("/tmp/repo"), Some("gpt-x"));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "--ask-for-approval",
                "never",
                "exec",
                "--sandbox",
                "workspace-write",
                "--ephemeral",
                "--model",
                "gpt-x",
            ]
        );
    }
}
