use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};

use crate::process::{format_exit_status, run_checked_output_with_context};

#[derive(Debug, Default, serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DiffStat {
    pub(crate) files: usize,
    pub(crate) insertions: u64,
    pub(crate) deletions: u64,
}

#[derive(Debug, Default)]
pub(crate) struct GitReceiptMetadata {
    pub(crate) changed_paths: Vec<String>,
    pub(crate) diff_stat: DiffStat,
    pub(crate) git_status_error: Option<String>,
    pub(crate) git_diff_stat_error: Option<String>,
}

pub(crate) fn collect_git_receipt_metadata(root: &Path) -> GitReceiptMetadata {
    let (changed_paths, git_status_error) = match repo_changed_paths(root) {
        Ok(changed_paths) => (changed_paths, None),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    };
    let (diff_stat, git_diff_stat_error) = match repo_diff_stat(root) {
        Ok(diff_stat) => (diff_stat, None),
        Err(error) => (DiffStat::default(), Some(format!("{error:#}"))),
    };

    GitReceiptMetadata {
        changed_paths,
        diff_stat,
        git_status_error,
        git_diff_stat_error,
    }
}

fn repo_changed_paths(root: &Path) -> Result<Vec<String>> {
    let output = git_output(root, &["status", "--short"], "git status --short")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| line.split_whitespace().last().map(str::to_string))
        .collect())
}

fn repo_diff_stat(root: &Path) -> Result<DiffStat> {
    let output = git_output(root, &["diff", "--numstat"], "git diff --numstat")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diff_stat_output(&stdout)
}

fn git_output(root: &Path, args: &[&str], label: &str) -> Result<Output> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);

    run_checked_output_with_context(
        &mut command,
        || format!("Failed to run {label} in {}", root.display()),
        |output| {
            format!(
                "{label} failed with {}.\nstdout:\n{}\nstderr:\n{}",
                format_exit_status(&output.status),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        },
    )
}

pub(crate) fn parse_diff_stat_output(stdout: &str) -> Result<DiffStat> {
    let mut diff_stat = DiffStat::default();
    for (index, line) in stdout.lines().enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("Unexpected git diff --numstat line {}: {}", index + 1, line);
        }
        diff_stat.files += 1;
        diff_stat.insertions += parse_numstat_count(fields[0], index + 1, "insertions")?;
        diff_stat.deletions += parse_numstat_count(fields[1], index + 1, "deletions")?;
    }
    Ok(diff_stat)
}

fn parse_numstat_count(field: &str, line_number: usize, kind: &str) -> Result<u64> {
    if field == "-" {
        return Ok(0);
    }
    field.parse::<u64>().with_context(|| {
        format!("Invalid git diff --numstat {kind} count on line {line_number}: {field}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_diff_stat_output_counts_binary_files_without_swallowing_other_errors() {
        let diff_stat =
            parse_diff_stat_output("12\t3\tsrc/main.rs\n-\t-\tassets/logo.png\n").unwrap();
        assert_eq!(diff_stat.files, 2);
        assert_eq!(diff_stat.insertions, 12);
        assert_eq!(diff_stat.deletions, 3);
    }

    #[test]
    fn parse_diff_stat_output_rejects_invalid_counts() {
        let error = parse_diff_stat_output("oops\t3\tsrc/main.rs\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("Invalid git diff --numstat insertions count"));
    }

    #[test]
    fn collect_git_receipt_metadata_records_git_failures() {
        let temp = tempdir().unwrap();
        let metadata = collect_git_receipt_metadata(temp.path());

        assert!(metadata.changed_paths.is_empty());
        assert_eq!(metadata.diff_stat.files, 0);
        assert!(metadata.git_status_error.is_some());
        assert!(metadata.git_diff_stat_error.is_some());
    }
}
