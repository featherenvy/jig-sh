use std::fs;
use std::io::{Read, Write, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::process::{format_exit_status, require_success, run_checked_output_with_context};

const MAX_INLINE_UNTRACKED_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_INLINE_UNTRACKED_BYTES: u64 = 32 * 1024 * 1024;

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
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

pub(crate) fn collect_git_receipt_metadata(root: &Path) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, true)
}

pub(crate) fn collect_git_receipt_metadata_without_worktree_fingerprint(
    root: &Path,
) -> GitReceiptMetadata {
    collect_git_receipt_metadata_with_options(root, false)
}

fn collect_git_receipt_metadata_with_options(
    root: &Path,
    collect_worktree_fingerprint: bool,
) -> GitReceiptMetadata {
    let (changed_paths, git_status_error) = match repo_changed_paths(root) {
        Ok(changed_paths) => (changed_paths, None),
        Err(error) => (Vec::new(), Some(format!("{error:#}"))),
    };
    let (diff_stat, git_diff_stat_error) = match repo_diff_stat(root) {
        Ok(diff_stat) => (diff_stat, None),
        Err(error) => (DiffStat::default(), Some(format!("{error:#}"))),
    };
    let (worktree_fingerprint, worktree_fingerprint_error) = if collect_worktree_fingerprint {
        match repo_worktree_fingerprint(root) {
            Ok(fingerprint) => (Some(fingerprint), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        }
    } else {
        (None, None)
    };

    GitReceiptMetadata {
        changed_paths,
        diff_stat,
        git_status_error,
        git_diff_stat_error,
        worktree_fingerprint,
        worktree_fingerprint_error,
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

pub(crate) fn repo_worktree_fingerprint(root: &Path) -> Result<String> {
    let status = git_output(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git status --porcelain",
    )?;
    let unstaged = git_output(
        root,
        &["diff", "--binary", "--", ".", ":(exclude).agent/**"],
        "git diff --binary",
    )?;
    let staged = git_output(
        root,
        &[
            "diff",
            "--cached",
            "--binary",
            "--",
            ".",
            ":(exclude).agent/**",
        ],
        "git diff --cached --binary",
    )?;
    let untracked = untracked_file_contents(root, &status.stdout)?;

    let mut input = Vec::new();
    input.extend_from_slice(b"status\0");
    input.extend_from_slice(&status.stdout);
    input.extend_from_slice(b"\0unstaged\0");
    input.extend_from_slice(&unstaged.stdout);
    input.extend_from_slice(b"\0staged\0");
    input.extend_from_slice(&staged.stdout);
    input.extend_from_slice(b"\0untracked\0");
    input.extend_from_slice(&untracked);

    git_hash_object(root, &input)
}

fn untracked_file_contents(root: &Path, status_stdout: &[u8]) -> Result<Vec<u8>> {
    let stdout = String::from_utf8_lossy(status_stdout);
    let mut contents = Vec::new();
    let mut remaining_inline_bytes = MAX_TOTAL_INLINE_UNTRACKED_BYTES;
    for line in stdout.lines() {
        if !line.starts_with("?? ") {
            continue;
        }
        let path = line
            .strip_prefix("?? ")
            .context("Malformed git status untracked line")?;
        let path = unquote_status_path(path)?;
        let full_path = root.join(&path);
        let metadata = fs::symlink_metadata(&full_path).with_context(|| {
            format!(
                "Failed to read untracked path metadata {}",
                full_path.display()
            )
        })?;

        contents.extend_from_slice(path.as_os_str().as_encoded_bytes());
        contents.push(0);
        append_untracked_path_fingerprint(
            &mut contents,
            root,
            &full_path,
            &metadata,
            &mut remaining_inline_bytes,
        )?;
        contents.push(0);
    }
    Ok(contents)
}

fn append_untracked_path_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
) -> Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        contents.extend_from_slice(b"symlink\0");
        let target = fs::read_link(full_path)
            .with_context(|| format!("Failed to read symlink target {}", full_path.display()))?;
        contents.extend_from_slice(target.as_os_str().as_encoded_bytes());
        return Ok(());
    }

    if metadata.is_dir() {
        contents.extend_from_slice(b"dir");
        return Ok(());
    }

    if metadata.is_file() {
        append_untracked_file_fingerprint(
            contents,
            root,
            full_path,
            metadata,
            remaining_inline_bytes,
        )?;
        return Ok(());
    }

    contents.extend_from_slice(b"other\0");
    append_metadata_fallback(contents, metadata);
    Ok(())
}

fn append_untracked_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
    metadata: &fs::Metadata,
    remaining_inline_bytes: &mut u64,
) -> Result<()> {
    if metadata.len() > MAX_INLINE_UNTRACKED_BYTES || metadata.len() > *remaining_inline_bytes {
        append_hashed_file_fingerprint(contents, root, full_path)?;
        return Ok(());
    }

    let mut file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_INLINE_UNTRACKED_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read untracked file {}", full_path.display()))?;

    if bytes.len() as u64 > MAX_INLINE_UNTRACKED_BYTES {
        append_hashed_file_fingerprint(contents, root, full_path)?;
        return Ok(());
    }

    contents.extend_from_slice(b"file\0");
    contents.extend_from_slice(&bytes);
    *remaining_inline_bytes = remaining_inline_bytes.saturating_sub(bytes.len() as u64);
    Ok(())
}

fn append_hashed_file_fingerprint(
    contents: &mut Vec<u8>,
    root: &Path,
    full_path: &Path,
) -> Result<()> {
    contents.extend_from_slice(b"file-hash\0");
    contents.extend_from_slice(git_hash_file(root, full_path)?.as_bytes());
    Ok(())
}

fn append_metadata_fallback(contents: &mut Vec<u8>, metadata: &fs::Metadata) {
    contents.extend_from_slice(format!("len={}\0", metadata.len()).as_bytes());
    contents.extend_from_slice(
        format!("modified={}\0", system_time_key(metadata.modified().ok())).as_bytes(),
    );
}

fn system_time_key(time: Option<SystemTime>) -> u128 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn unquote_status_path(path: &str) -> Result<PathBuf> {
    if !path.starts_with('"') {
        return Ok(PathBuf::from(path));
    }

    let decoded = unescape_c_quoted_path(path)?;
    Ok(PathBuf::from(decoded))
}

fn unescape_c_quoted_path(path: &str) -> Result<String> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        bail!("Malformed quoted git status path: {path}");
    }

    let mut decoded = Vec::new();
    let mut index = 1;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if byte != b'\\' {
            decoded.push(byte);
            index += 1;
            continue;
        }

        index += 1;
        if index + 1 >= bytes.len() {
            bail!("Malformed escape in git status path: {path}");
        }
        match bytes[index] {
            b'\\' => decoded.push(b'\\'),
            b'"' => decoded.push(b'"'),
            b'n' => decoded.push(b'\n'),
            b't' => decoded.push(b'\t'),
            b'r' => decoded.push(b'\r'),
            b'b' => decoded.push(8),
            b'f' => decoded.push(12),
            b'0'..=b'7' => {
                if index + 2 >= bytes.len() - 1 {
                    bail!("Malformed octal escape in git status path: {path}");
                }
                let value = parse_octal_escape(&bytes[index..index + 3], path)?;
                decoded.push(value);
                index += 2;
            }
            other => bail!("Unsupported escape in git status path: \\{}", other as char),
        }
        index += 1;
    }

    String::from_utf8(decoded).with_context(|| format!("Git status path is not UTF-8: {path}"))
}

fn parse_octal_escape(bytes: &[u8], path: &str) -> Result<u8> {
    let mut value = 0_u16;
    for byte in bytes {
        if !(b'0'..=b'7').contains(byte) {
            bail!("Malformed octal escape in git status path: {path}");
        }
        value = value * 8 + u16::from(byte - b'0');
    }
    u8::try_from(value).with_context(|| format!("Octal escape is out of range in path: {path}"))
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

fn git_hash_object(root: &Path, input: &[u8]) -> Result<String> {
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start git hash-object in {}", root.display()))?;

    child
        .stdin
        .as_mut()
        .context("git hash-object stdin was not available")?
        .write_all(input)
        .context("Failed to write worktree fingerprint input to git hash-object")?;

    let output = child
        .wait_with_output()
        .context("Failed to wait for git hash-object")?;
    require_success(&output, |output| {
        format!(
            "git hash-object failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_hash_file(root: &Path, full_path: &Path) -> Result<String> {
    let mut file = fs::File::open(full_path)
        .with_context(|| format!("Failed to open untracked file {}", full_path.display()))?;
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start git hash-object in {}", root.display()))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .context("git hash-object stdin was not available")?;
        copy(&mut file, &mut stdin)
            .with_context(|| format!("Failed to hash untracked file {}", full_path.display()))?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git hash-object")?;
    require_success(&output, |output| {
        format!(
            "git hash-object failed with {}.\nstdout:\n{}\nstderr:\n{}",
            format_exit_status(&output.status),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    })?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
    use std::time::{Duration, UNIX_EPOCH};
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
        assert!(metadata.worktree_fingerprint.is_none());
        assert!(metadata.worktree_fingerprint_error.is_some());
    }

    #[test]
    fn worktree_fingerprint_changes_when_untracked_file_content_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        std::fs::write(temp.path().join("new.txt"), "one").unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();
        std::fs::write(temp.path().join("new.txt"), "two").unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn worktree_fingerprint_changes_when_large_untracked_file_content_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        let large_path = temp.path().join("large.bin");
        let fixed_mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        std::fs::write(
            &large_path,
            vec![b'a'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::File::open(&large_path)
            .unwrap()
            .set_modified(fixed_mtime)
            .unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();

        std::fs::write(
            &large_path,
            vec![b'b'; MAX_INLINE_UNTRACKED_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::File::open(&large_path)
            .unwrap()
            .set_modified(fixed_mtime)
            .unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    #[cfg(unix)]
    #[test]
    fn worktree_fingerprint_changes_when_untracked_symlink_target_changes() {
        let temp = tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(
            temp.path(),
            &["config", "user.email", "fixture@example.com"],
        );
        run_git(temp.path(), &["config", "user.name", "Fixture"]);
        std::fs::write(temp.path().join("tracked.txt"), "tracked").unwrap();
        run_git(temp.path(), &["add", "tracked.txt"]);
        run_git(temp.path(), &["commit", "-m", "initial fixture"]);

        let first_target = temp.path().join("outside-one");
        let second_target = temp.path().join("outside-two");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&first_target, &link).unwrap();
        let first = repo_worktree_fingerprint(temp.path()).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second_target, &link).unwrap();
        let second = repo_worktree_fingerprint(temp.path()).unwrap();

        assert_ne!(first, second);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
