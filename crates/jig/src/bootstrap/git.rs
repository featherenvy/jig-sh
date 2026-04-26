use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::process::{require_success, run_checked_output, run_checked_stdout_trimmed};

use super::{GIT_BIN_ENV, external_program};

pub(super) fn ensure_git_repo(path: &Path) -> Result<()> {
    if path.join(".git").exists() {
        return Ok(());
    }

    let git_program = external_program(GIT_BIN_ENV, "git");
    let output = Command::new(&git_program)
        .current_dir(path)
        .args(["init", "-b", "main"])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    if output.status.success() {
        return Ok(());
    }
    if !git_init_branch_flag_unsupported(&output) {
        bail!(
            "git init -b main failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    git(path, ["init"])?;
    set_git_head_branch(path, &git_program, "main")
}

pub(super) fn is_git_work_tree(path: &Path) -> bool {
    git_command(path, ["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(super) fn ensure_clean_git_work_tree(path: &Path) -> Result<()> {
    let status = git_stdout(path, ["status", "--short"])?;
    if !status.is_empty() {
        bail!(
            "Local committed template mode requires a clean git working tree: {}\n\
             Commit or stash template changes, or re-run with --template-mode working-tree.",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn git(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Result<()> {
    let mut command = git_command(path, args);
    run_checked_output(&mut command, |output| {
        git_command_failed_message(path, output)
    })?;
    Ok(())
}

pub(super) fn git_stdout(
    path: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String> {
    let mut command = git_command(path, args);
    run_checked_stdout_trimmed(&mut command, |output| {
        git_command_failed_message(path, output)
    })
}

pub(super) fn git_command(path: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Command {
    let git_program = external_program(GIT_BIN_ENV, "git");
    let mut command = Command::new(git_program);
    command.current_dir(path);
    for arg in args {
        command.arg(arg.as_ref());
    }
    command
}

pub(super) fn init_git_repo(destination: &Path, default_branch: &str) -> Result<bool> {
    if destination.join(".git").exists() {
        return Ok(false);
    }

    let git_program = external_program(GIT_BIN_ENV, "git");
    let with_branch = Command::new(&git_program)
        .current_dir(destination)
        .args(["init", "-b", default_branch])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;

    if with_branch.status.success() {
        return Ok(true);
    }
    if !git_init_branch_flag_unsupported(&with_branch) {
        bail!(
            "git init -b {default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&with_branch.stdout),
            String::from_utf8_lossy(&with_branch.stderr)
        );
    }

    let fallback = Command::new(&git_program)
        .current_dir(destination)
        .arg("init")
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    require_success(&fallback, |output| {
        format!(
            "git init failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    set_git_head_branch(destination, &git_program, default_branch)?;
    Ok(true)
}

fn git_init_branch_flag_unsupported(output: &std::process::Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    stderr.contains("unknown switch `b")
        || stderr.contains("unknown option `b")
        || stderr.contains("unknown option `initial-branch")
        || stderr.contains("unknown option `initial branch")
}

fn git_command_failed_message(path: &Path, output: &std::process::Output) -> String {
    format!(
        "git command failed in {}\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn set_git_head_branch(destination: &Path, git_program: &str, default_branch: &str) -> Result<()> {
    let output = Command::new(git_program)
        .current_dir(destination)
        .args([
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{default_branch}"),
        ])
        .output()
        .with_context(|| format!("Failed to start {}", git_program))?;
    require_success(&output, |output| {
        format!(
            "git symbolic-ref HEAD refs/heads/{default_branch} failed.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
