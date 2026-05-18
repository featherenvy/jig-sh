use std::ffi::OsString;
#[cfg(unix)]
use std::fs::{File, Permissions};
use std::io::Read;
#[cfg(unix)]
use std::io::{Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Child;
use std::process::ChildStderr;
use std::process::ChildStdout;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::SecretBytes;
use crate::env_policy::is_preserved_env_var_name;
use crate::redact::Redactor;
use crate::types::{EnvVarName, SecretName};

// Keep this cap aligned with redaction cost: redaction scans the captured text
// once per raw/encoded secret needle.
pub const MAX_CAPTURED_STREAM_BYTES: usize = 1024 * 1024;
const BROKERED_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STREAM_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredEnv {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredFile {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredRun {
    pub(crate) command: Vec<String>,
    pub(crate) env: Vec<ResolvedBrokeredEnv>,
    pub(crate) files: Vec<ResolvedBrokeredFile>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunOutput {
    pub exit_status: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_brokered(request: ResolvedBrokeredRun) -> AnyResult<RunOutput> {
    run_brokered_with_timeout(request, BROKERED_RUN_TIMEOUT)
}

fn run_brokered_with_timeout(
    request: ResolvedBrokeredRun,
    timeout: Duration,
) -> AnyResult<RunOutput> {
    // Keep this guard for direct crate callers; clap enforces it for the CLI.
    if request.command.is_empty() {
        bail!("vault run requires a command after --");
    }
    let redactor = Redactor::from_secret_slices(
        request
            .env
            .iter()
            .map(|mapping| mapping.value.as_slice())
            .chain(request.files.iter().map(|mapping| mapping.value.as_slice())),
    );
    let file_env = BrokeredSecretFiles::create(&request.files)?;
    let mut env_values = Vec::<(String, Zeroizing<String>)>::new();
    for mapping in request.env {
        let env_value = match mapping.value.into_zeroizing_string() {
            Ok(value) => value,
            Err(_value) => {
                bail!(
                    "vault secret '{}' cannot be injected as env var {} because it is not valid UTF-8",
                    mapping.secret_name.as_str(),
                    mapping.var.as_str()
                );
            }
        };
        env_values.push((mapping.var.as_str().to_string(), env_value));
    }

    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]).env_clear();
    preserve_minimal_environment(&mut command);
    for (name, value) in &env_values {
        // std::process::Command copies env values into OsString storage; keep
        // our source copy zeroized, but the std-owned copy is dropped normally.
        command.env(name, value.as_str());
    }
    if let Some(file_env) = &file_env {
        for (name, path) in file_env.env() {
            command.env(name, path);
        }
    }
    configure_child_process(&mut command);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run brokered command '{}'", request.command[0]))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("brokered command stdout pipe was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("brokered command stderr pipe was not captured"))?;
    let (status, stdout, stderr) =
        wait_for_capped_output(child, stdout, stderr, &request.command[0], timeout)?;
    Ok(RunOutput {
        exit_status: status.exit_status,
        exit_signal: status.exit_signal,
        stdout: redactor.redact_bytes_lossy(stdout.as_slice()),
        stderr: redactor.redact_bytes_lossy(stderr.as_slice()),
    })
}

struct BrokeredSecretFiles {
    // Secret files intentionally live on disk while the child runs. TempDir
    // cleanup removes them on normal unwind; hard process kills can leave them
    // behind for OS temp cleanup. Drop runs before field drops, so keep `_dir`
    // before `files`: the explicit Drop wipe uses the retained file handles,
    // then TempDir removes the persisted paths during field drop.
    _dir: tempfile::TempDir,
    env: Vec<(String, OsString)>,
    #[cfg(unix)]
    files: Vec<(OsString, File)>,
}

#[cfg(unix)]
impl Drop for BrokeredSecretFiles {
    fn drop(&mut self) {
        for (path, file) in &mut self.files {
            wipe_secret_file_best_effort(file, std::path::Path::new(path));
        }
    }
}

impl BrokeredSecretFiles {
    fn create(files: &[ResolvedBrokeredFile]) -> AnyResult<Option<Self>> {
        if files.is_empty() {
            return Ok(None);
        }

        #[cfg(not(unix))]
        {
            bail!(
                "vault run --file mapping '{}={}' requires Unix-style owner-only temporary files; use --env on this platform",
                files[0].var.as_str(),
                files[0].secret_name.as_str()
            );
        }

        #[cfg(unix)]
        {
            let dir = tempfile::Builder::new()
                .prefix("jig-vault-run-")
                .permissions(Permissions::from_mode(0o700))
                .tempdir()
                .context("failed to create vault secret file temp dir")?;
            let mut env = Vec::with_capacity(files.len());
            let mut persisted_files = Vec::with_capacity(files.len());
            for mapping in files {
                // tempfile uses mkstemp on Unix and creates owner-only files;
                // keep the random path so the child can read it until TempDir cleanup.
                let mut secret_file = tempfile::Builder::new()
                    .prefix("secret-")
                    .tempfile_in(dir.path())
                    .with_context(|| {
                        format!(
                            "failed to create brokered temp file for vault secret '{}'",
                            mapping.secret_name.as_str()
                        )
                    })?;
                let path = secret_file.path().to_path_buf();
                write_secret_file(secret_file.as_file_mut(), &path, mapping.value.as_slice())
                    .with_context(|| {
                        format!(
                            "failed to write vault secret '{}' to a brokered temp file",
                            mapping.secret_name.as_str()
                        )
                    })?;
                // `keep` gives the child a stable path; the owning TempDir still
                // removes the persisted file tree when the brokered run ends.
                let (file, path) = secret_file.keep().with_context(|| {
                    format!(
                        "failed to persist brokered temp file for vault secret '{}'",
                        mapping.secret_name.as_str(),
                    )
                })?;
                let path = path.into_os_string();
                env.push((mapping.var.as_str().to_string(), path.clone()));
                persisted_files.push((path, file));
            }
            Ok(Some(Self {
                _dir: dir,
                env,
                files: persisted_files,
            }))
        }
    }

    fn env(&self) -> &[(String, OsString)] {
        &self.env
    }
}

#[cfg(unix)]
fn write_secret_file(file: &mut File, path: &std::path::Path, value: &[u8]) -> AnyResult<()> {
    file.write_all(value)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))
}

#[cfg(unix)]
fn wipe_secret_file_best_effort(file: &mut File, path: &std::path::Path) {
    if let Err(error) = wipe_secret_file(file, path) {
        eprintln!(
            "jig vault could not wipe brokered temp secret file {} before cleanup: {error:#}",
            path.display()
        );
    }
}

#[cfg(unix)]
fn wipe_secret_file(file: &mut File, path: &std::path::Path) -> AnyResult<()> {
    let len = file.seek(SeekFrom::End(0)).with_context(|| {
        format!(
            "failed to measure brokered temp secret file {}",
            path.display()
        )
    })?;
    file.rewind().with_context(|| {
        format!(
            "failed to seek brokered temp secret file {}",
            path.display()
        )
    })?;
    let zeros = [0_u8; 8192];
    let mut remaining = len;
    while remaining > 0 {
        let chunk_len = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..chunk_len]).with_context(|| {
            format!(
                "failed to wipe brokered temp secret file {}",
                path.display()
            )
        })?;
        remaining -= chunk_len as u64;
    }
    file.sync_all().with_context(|| {
        format!(
            "failed to sync wiped brokered temp secret file {}",
            path.display()
        )
    })
}

fn wait_for_capped_output(
    mut child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    command_name: &str,
    timeout: Duration,
) -> AnyResult<(PortableRunStatus, SecretBytes, SecretBytes)> {
    let (stream_tx, stream_rx) = mpsc::channel();
    let stdout_reader = spawn_stream_reader("stdout", stdout, stream_tx.clone());
    let stderr_reader = spawn_stream_reader("stderr", stderr, stream_tx);
    let mut child_status = None;
    let mut stdout = None;
    let mut stderr = None;
    let mut reader_error = None;
    let deadline = Instant::now() + timeout;

    while stdout.is_none() || stderr.is_none() {
        let now = Instant::now();
        if now >= deadline {
            terminate_child(&mut child);
            // Best-effort after timeout: preserve the timeout error that will
            // be returned below.
            let _ = child.wait();
            join_stream_reader("stdout", stdout_reader)?;
            join_stream_reader("stderr", stderr_reader)?;
            bail!("brokered command '{command_name}' exceeded the {timeout:?} run timeout");
        }
        let poll_interval =
            STREAM_RESULT_POLL_INTERVAL.min(deadline.saturating_duration_since(now));
        match stream_rx.recv_timeout(poll_interval) {
            Ok((label, result)) => match result {
                Ok(bytes) if label == "stdout" => stdout = Some(bytes),
                Ok(bytes) if label == "stderr" => stderr = Some(bytes),
                Ok(_) => bail!("brokered stream reader returned an unexpected label"),
                Err(error) => {
                    reader_error = Some(error);
                    terminate_child(&mut child);
                    break;
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                terminate_child(&mut child);
                break;
            }
        }
        if child_status.is_none() {
            child_status = child
                .try_wait()
                .with_context(|| format!("failed to poll brokered command '{command_name}'"))?
                .map(run_status);
        }
    }

    if let Some(error) = reader_error {
        // Best-effort after a stream reader error: preserve that reader error.
        let _ = child.wait();
        join_stream_reader("stdout", stdout_reader)?;
        join_stream_reader("stderr", stderr_reader)?;
        return Err(error);
    }

    let status =
        match child_status {
            Some(status) => status,
            None => run_status(child.wait().with_context(|| {
                format!("failed to wait for brokered command '{command_name}'")
            })?),
        };
    join_stream_reader("stdout", stdout_reader)?;
    join_stream_reader("stderr", stderr_reader)?;
    let stdout = stdout.ok_or_else(|| anyhow!("brokered command stdout reader stopped early"))?;
    let stderr = stderr.ok_or_else(|| anyhow!("brokered command stderr reader stopped early"))?;
    Ok((status, stdout, stderr))
}

fn spawn_stream_reader<R>(
    label: &'static str,
    reader: R,
    sender: Sender<(&'static str, AnyResult<SecretBytes>)>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        // The receiver can disconnect after timeout or another reader failure;
        // in that case the main thread already has the controlling error.
        let _ = sender.send((label, read_capped_stream(label, reader)));
    })
}

fn configure_child_process(_command: &mut Command) {
    #[cfg(unix)]
    {
        _command.process_group(0);
    }
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        if pid <= i32::MAX as u32 {
            // The brokered child is started as its own process group leader on
            // Unix. Killing the group keeps capped output from leaving helper
            // grandchildren alive with inherited pipe descriptors. A very early
            // failure can race the child's setpgid; child.kill below still
            // covers the primary child, and this path is fail-closed cleanup
            // after capture overflow or reader failure.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
    // Best-effort cleanup after timeout, overflow, or reader failure; callers
    // keep reporting the original failure path.
    let _ = child.kill();
}

fn read_capped_stream(label: &'static str, mut reader: impl Read) -> AnyResult<SecretBytes> {
    // Allocate the full cap up front so captured secret-bearing bytes do not
    // pass through discarded intermediate Vec buffers during growth.
    let mut output = SecretBytes::with_capacity(MAX_CAPTURED_STREAM_BYTES);
    let mut buffer = Zeroizing::new([0_u8; 8192]);
    loop {
        let read = reader
            .read(&mut buffer[..])
            .with_context(|| format!("failed to read brokered command {label}"))?;
        if read == 0 {
            return Ok(output);
        }
        if output.len() + read > MAX_CAPTURED_STREAM_BYTES {
            let remaining = MAX_CAPTURED_STREAM_BYTES.saturating_sub(output.len());
            output.extend_from_slice(&buffer[..remaining])?;
            bail!(
                "brokered command {label} exceeded the {} byte capture limit",
                MAX_CAPTURED_STREAM_BYTES
            );
        }
        output.extend_from_slice(&buffer[..read])?;
    }
}

fn join_stream_reader(label: &'static str, handle: thread::JoinHandle<()>) -> AnyResult<()> {
    handle
        .join()
        .map_err(|_| anyhow!("brokered command {label} reader panicked"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PortableRunStatus {
    exit_status: i32,
    exit_signal: Option<i32>,
}

fn run_status(status: ExitStatus) -> PortableRunStatus {
    if let Some(code) = status.code() {
        return PortableRunStatus {
            exit_status: code,
            exit_signal: None,
        };
    }
    #[cfg(unix)]
    {
        let signal = status.signal();
        PortableRunStatus {
            exit_status: signal.map(|signal| 128 + signal).unwrap_or(1),
            exit_signal: signal,
        }
    }
    #[cfg(not(unix))]
    {
        PortableRunStatus {
            exit_status: 1,
            exit_signal: None,
        }
    }
}

fn preserve_minimal_environment(command: &mut Command) {
    // Env forwarding is allowlist-only. Loader/interpreter hooks such as
    // LD_PRELOAD, DYLD_*, PYTHONPATH, NODE_OPTIONS, SSH_AUTH_SOCK, XDG_*,
    // and TZ stay out unless deliberately added to the exact list below.
    for (name, value) in std::env::vars() {
        if is_preserved_env_var_name(&name) {
            command.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn brokered_run_injects_and_redacts_env_secret() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf '%s' \"$TOKEN\"; printf '%s' \"$TOKEN\" >&2".into(),
            ],
            env: vec![ResolvedBrokeredEnv {
                var: EnvVarName::parse("TOKEN").unwrap(),
                secret_name: SecretName::parse("api_token").unwrap(),
                value: SecretBytes::new(b"secret-value".to_vec()),
            }],
            files: Vec::new(),
        })
        .unwrap();
        assert_eq!(output.exit_status, 0);
        assert_eq!(output.exit_signal, None);
        assert_eq!(output.stdout, "[REDACTED]");
        assert_eq!(output.stderr, "[REDACTED]");
    }

    #[test]
    fn brokered_run_rejects_non_utf8_env_secret() {
        let error = run_brokered(ResolvedBrokeredRun {
            command: vec!["true".into()],
            env: vec![ResolvedBrokeredEnv {
                var: EnvVarName::parse("TOKEN").unwrap(),
                secret_name: SecretName::parse("binary_token").unwrap(),
                value: SecretBytes::new(vec![0xff, 0xfe, 0xfd, 0xfc]),
            }],
            files: Vec::new(),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("not valid UTF-8"));
    }

    #[test]
    fn read_capped_stream_rejects_oversized_output() {
        let input = vec![b'x'; MAX_CAPTURED_STREAM_BYTES + 1];
        let error = read_capped_stream("stdout", std::io::Cursor::new(input))
            .unwrap_err()
            .to_string();
        assert!(error.contains("capture limit"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_rejects_oversized_stdout() {
        let error = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                format!("head -c {} /dev/zero", MAX_CAPTURED_STREAM_BYTES + 1),
            ],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("capture limit"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_terminates_other_stream_after_stdout_overflow() {
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        "(sleep 5 >&2) & head -c {} /dev/zero",
                        MAX_CAPTURED_STREAM_BYTES + 1
                    ),
                ],
                env: Vec::new(),
                files: Vec::new(),
            },
            Duration::from_secs(2),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("capture limit"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_times_out() {
        let error = run_brokered_with_timeout(
            ResolvedBrokeredRun {
                command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
                env: Vec::new(),
                files: Vec::new(),
            },
            Duration::from_millis(20),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("run timeout"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_reports_unix_signal_exit_status() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "kill -TERM $$".into()],
            env: Vec::new(),
            files: Vec::new(),
        })
        .unwrap();
        assert_eq!(output.exit_status, 143);
        assert_eq!(output.exit_signal, Some(15));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_delivers_and_redacts_secret_file() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec![
                "sh".into(),
                "-c".into(),
                "test -f \"$TOKEN_FILE\" && cat \"$TOKEN_FILE\"".into(),
            ],
            env: Vec::new(),
            files: vec![ResolvedBrokeredFile {
                var: EnvVarName::parse("TOKEN_FILE").unwrap(),
                secret_name: SecretName::parse("api_token").unwrap(),
                value: SecretBytes::new(b"secret-value".to_vec()),
            }],
        })
        .unwrap();

        assert_eq!(output.exit_status, 0);
        assert_eq!(output.exit_signal, None);
        assert_eq!(output.stdout, "[REDACTED]");
        assert_eq!(output.stderr, "");
    }

    #[cfg(unix)]
    #[test]
    fn brokered_secret_files_create_owner_only_paths() {
        let files = [ResolvedBrokeredFile {
            var: EnvVarName::parse("TOKEN_FILE").unwrap(),
            secret_name: SecretName::parse("api_token").unwrap(),
            value: SecretBytes::new(b"secret-value".to_vec()),
        }];

        let secret_files = BrokeredSecretFiles::create(&files).unwrap().unwrap();
        let file_path = std::path::PathBuf::from(secret_files.env()[0].1.clone());
        let dir_path = file_path.parent().unwrap();

        assert_eq!(
            fs::metadata(dir_path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(file_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn wipe_secret_file_overwrites_contents_before_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("secret");
        fs::write(&path, b"secret-value").unwrap();
        let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();

        wipe_secret_file(&mut file, &path).unwrap();

        assert_eq!(fs::read(&path).unwrap(), vec![0_u8; "secret-value".len()]);
    }
}
