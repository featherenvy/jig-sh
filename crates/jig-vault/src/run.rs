use std::io::Read;
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
use crate::redact::Redactor;
use crate::types::{EnvVarName, SecretName};

// Keep this cap aligned with redaction cost: redaction scans the captured text
// once per raw/encoded secret needle.
pub const MAX_CAPTURED_STREAM_BYTES: usize = 1024 * 1024;
const BROKERED_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const STREAM_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(unix)]
const PRESERVED_ENV_EXACT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
];
#[cfg(windows)]
const PRESERVED_ENV_EXACT: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "USERPROFILE",
    "USERNAME",
    "TEMP",
    "TMP",
];
#[cfg(not(any(unix, windows)))]
const PRESERVED_ENV_EXACT: &[&str] = &[];

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredEnv {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredRun {
    pub(crate) command: Vec<String>,
    pub(crate) env: Vec<ResolvedBrokeredEnv>,
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
    let redactor =
        Redactor::from_secret_slices(request.env.iter().map(|mapping| mapping.value.as_slice()));
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

fn configure_child_process(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
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

pub(crate) fn is_preserved_env_var_name(name: &str) -> bool {
    should_preserve_env_var(name, PRESERVED_ENV_EXACT)
}

pub(crate) fn env_var_names_equal(left: &str, right: &str) -> bool {
    env_var_names_equal_inner(left, right)
}

fn should_preserve_env_var(name: &str, exact: &[&str]) -> bool {
    // Exact matching is deliberate: do not reintroduce prefix forwarding.
    exact
        .iter()
        .any(|preserved| env_var_names_equal_inner(name, preserved))
}

#[cfg(windows)]
fn env_var_names_equal_inner(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
fn env_var_names_equal_inner(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

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
            },
            Duration::from_millis(20),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("run timeout"));
    }

    #[test]
    fn minimal_environment_does_not_preserve_arbitrary_lc_names() {
        assert!(should_preserve_env_var("LC_TIME", &["LC_TIME"]));
        assert!(!should_preserve_env_var(
            "LC_MALICIOUS",
            &["LC_ALL", "LC_CTYPE", "LC_TIME"]
        ));
    }

    #[test]
    fn preserved_environment_name_case_follows_platform_rules() {
        assert!(should_preserve_env_var("PATH", &["PATH"]));
        #[cfg(windows)]
        assert!(should_preserve_env_var("Path", &["PATH"]));
        #[cfg(not(windows))]
        assert!(!should_preserve_env_var("Path", &["PATH"]));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_reports_unix_signal_exit_status() {
        let output = run_brokered(ResolvedBrokeredRun {
            command: vec!["sh".into(), "-c".into(), "kill -TERM $$".into()],
            env: Vec::new(),
        })
        .unwrap();
        assert_eq!(output.exit_status, 143);
        assert_eq!(output.exit_signal, Some(15));
    }
}
