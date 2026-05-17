use std::ffi::OsString;
use std::io::{IsTerminal, Read};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::sync::{Mutex, MutexGuard};

use anyhow::{Context, Result, anyhow, bail};
use jig_vault::{BrokeredEnv, BrokeredRun, MAX_SECRET_VALUE_LEN, SecretBytes, Vault};
use secrecy::SecretString;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::command::{
    VaultAuditCommand, VaultCommand, VaultInitRequest, VaultRunRequest, VaultRuntimeOptions,
    VaultSecretCommand, VaultSecretListRequest, VaultSecretRemoveRequest, VaultSecretSetRequest,
    VaultSecretValueSource, VaultStatusRequest,
};

const PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
static CAPTURED_PASSPHRASE: Mutex<Option<SecretString>> = Mutex::new(None);

pub(crate) fn dispatch(command: VaultCommand) -> Result<Value> {
    match command {
        VaultCommand::Audit(command) => match command {
            VaultAuditCommand::Verify(request) => verify_audit(request),
        },
        VaultCommand::Init(request) => init(request),
        VaultCommand::Status(request) => status(request),
        VaultCommand::Secret(command) => match command {
            VaultSecretCommand::List(request) => list(request),
            VaultSecretCommand::Set(request) => set(request),
            VaultSecretCommand::Remove(request) => remove(request),
        },
        VaultCommand::Run(request) => run(request),
    }
}

fn init(request: VaultInitRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    vault.init(&passphrase)?;
    Ok(json!({
        "ok": true,
        "command": "vault init",
        "vault_home": vault.root().display().to_string(),
        "created": true,
    }))
}

fn status(request: VaultStatusRequest) -> Result<Value> {
    let status = Vault::status(request.vault.home)?;
    Ok(json!({
        "ok": true,
        "command": "vault status",
        "vault_home": status.root.display().to_string(),
        "exists": status.exists,
        "vault_file_exists": status.exists,
    }))
}

fn verify_audit(request: crate::command::VaultAuditVerifyRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    let verification = vault.verify_audit(&passphrase)?;
    Ok(json!({
        "ok": true,
        "command": "vault audit verify",
        "vault_home": vault.root().display().to_string(),
        "event_count": verification.event_count,
        "latest_mac": verification.latest_mac,
        "torn_tail_bytes": verification.torn_tail_bytes,
    }))
}

fn list(request: VaultSecretListRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    let secrets: Vec<Value> = vault
        .list(&passphrase)?
        .into_iter()
        .map(|record| {
            json!({
                "name": record.name,
                "created_at_ms": record.created_at_ms,
                "updated_at_ms": record.updated_at_ms,
                "value_len": record.value_len,
            })
        })
        .collect();
    Ok(json!({
        "ok": true,
        "command": "vault secret list",
        "vault_home": vault.root().display().to_string(),
        "secrets": secrets,
    }))
}

fn set(request: VaultSecretSetRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    let value = match request.value_source {
        VaultSecretValueSource::Stdin => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                bail!(
                    "--value-stdin requires piped or redirected stdin; use --value-prompt for hidden terminal input"
                );
            }
            read_secret_value(stdin.lock())?
        }
        VaultSecretValueSource::Prompt => read_secret_value_from_prompt()?,
    };
    vault.set_secret(&passphrase, &request.name, value)?;
    Ok(json!({
        "ok": true,
        "command": "vault secret set",
        "vault_home": vault.root().display().to_string(),
        "name": request.name,
    }))
}

fn remove(request: VaultSecretRemoveRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    let removed = vault.remove_secret(&passphrase, &request.name)?;
    Ok(json!({
        "ok": true,
        "command": "vault secret remove",
        "vault_home": vault.root().display().to_string(),
        "name": request.name,
        "removed": removed,
    }))
}

fn run(request: VaultRunRequest) -> Result<Value> {
    let vault = vault(&request.vault)?;
    let passphrase = passphrase()?;
    let env = parse_env_mappings(&request.env)?;
    let output = vault.run_brokered(&passphrase, BrokeredRun::new(request.command, env)?)?;
    Ok(json!({
        "ok": output.exit_status == 0,
        "command": "vault run",
        "vault_home": vault.root().display().to_string(),
        "result": {
            "exit_status": output.exit_status,
            "exit_signal": output.exit_signal,
            "stdout": output.stdout,
            "stderr": output.stderr,
        },
    }))
}

fn vault(options: &VaultRuntimeOptions) -> Result<Vault> {
    Ok(Vault::resolve(options.home.clone())?)
}

pub(crate) fn capture_passphrase() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::Unlock)
}

pub(crate) fn capture_new_passphrase() -> Result<()> {
    capture_passphrase_with_prompt(PromptKind::NewVault)
}

fn capture_passphrase_with_prompt(kind: PromptKind) -> Result<()> {
    if std::env::var_os(PASSPHRASE_ENV).is_some() {
        return capture_passphrase_from_env();
    }
    if hidden_terminal_input_available() {
        clear_captured_passphrase()?;
        let passphrase = prompt_passphrase(kind)?;
        set_captured_passphrase(passphrase)?;
        return Ok(());
    }
    capture_passphrase_from_env()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromptKind {
    Unlock,
    NewVault,
}

pub(crate) fn capture_passphrase_from_env() -> Result<()> {
    let Some(value) = std::env::var_os(PASSPHRASE_ENV) else {
        let mut captured = captured_passphrase_lock()?;
        *captured = None;
        return Ok(());
    };
    // Keep a malformed environment value intact so the operator can inspect or
    // retry it. Only the successfully captured process copy is cleared below.
    let passphrase = passphrase_from_os(value)?;
    // SAFETY: `cli::run` invokes this immediately before entering
    // `runtime::dispatch_vault`, and before `Vault::run_brokered`, the only
    // vault path that spawns threads. Clearing the child process environment
    // after successful capture does not affect the parent shell's environment
    // for later invocations.
    unsafe {
        std::env::remove_var(PASSPHRASE_ENV);
    }
    let mut captured = captured_passphrase_lock()?;
    *captured = Some(passphrase);
    Ok(())
}

fn prompt_passphrase(kind: PromptKind) -> Result<SecretString> {
    match kind {
        PromptKind::Unlock => {
            let passphrase = prompt_zeroizing("Jig Vault passphrase: ")
                .context("failed to read vault passphrase from terminal")?;
            Ok(secret_string_from_zeroizing(passphrase))
        }
        PromptKind::NewVault => {
            let passphrase = prompt_zeroizing("New Jig Vault passphrase: ")
                .context("failed to read new vault passphrase from terminal")?;
            let confirmation = prompt_zeroizing("Confirm Jig Vault passphrase: ")
                .context("failed to read vault passphrase confirmation from terminal")?;
            if *passphrase != *confirmation {
                bail!("vault passphrase confirmation did not match");
            }
            Ok(secret_string_from_zeroizing(passphrase))
        }
    }
}

fn prompt_zeroizing(prompt: &str) -> Result<Zeroizing<String>> {
    Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
}

fn secret_string_from_zeroizing(mut value: Zeroizing<String>) -> SecretString {
    SecretString::from(std::mem::take(&mut *value))
}

fn set_captured_passphrase(passphrase: SecretString) -> Result<()> {
    let mut captured = captured_passphrase_lock()?;
    *captured = Some(passphrase);
    Ok(())
}

fn clear_captured_passphrase() -> Result<()> {
    let mut captured = captured_passphrase_lock()?;
    *captured = None;
    Ok(())
}

fn passphrase() -> Result<SecretString> {
    let mut captured = captured_passphrase_lock()?;
    // Each CLI invocation dispatches exactly one vault operation after capture,
    // so consume the passphrase instead of keeping process-global key material.
    if let Some(passphrase) = captured.take() {
        return Ok(passphrase);
    }
    Err(anyhow!(
        "{PASSPHRASE_ENV} is required for non-interactive `jig vault` commands; run from a terminal to be prompted, or export {PASSPHRASE_ENV}. Command-line passphrases are intentionally unsupported"
    ))
}

fn captured_passphrase_lock() -> Result<MutexGuard<'static, Option<SecretString>>> {
    CAPTURED_PASSPHRASE
        .lock()
        .map_err(|error| anyhow!("vault passphrase capture lock is poisoned: {error}"))
}

#[cfg(unix)]
fn passphrase_from_os(value: OsString) -> Result<SecretString> {
    SecretBytes::new(value.into_vec())
        .into_secret_string()
        .map_err(|_bytes| {
            // The rejected bytes are passphrase material; discard them instead
            // of preserving the conversion payload in diagnostics.
            anyhow!(
                "{PASSPHRASE_ENV} must be valid UTF-8 for `jig vault`; run from a terminal to be prompted, or export valid UTF-8. Command-line passphrases are intentionally unsupported"
            )
        })
}

#[cfg(not(unix))]
fn passphrase_from_os(value: OsString) -> Result<SecretString> {
    value.into_string().map(SecretString::from).map_err(|_value| {
        // The rejected value is passphrase material; discard it instead of
        // preserving the conversion payload in diagnostics.
        anyhow!(
            "{PASSPHRASE_ENV} must be valid UTF-8 for `jig vault`; run from a terminal to be prompted, or export valid UTF-8. Command-line passphrases are intentionally unsupported"
        )
    })
}

fn hidden_terminal_input_available() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn parse_env_mappings(values: &[String]) -> Result<Vec<BrokeredEnv>> {
    values
        .iter()
        .map(|value| Ok(BrokeredEnv::parse(value)?))
        .collect()
}

fn read_secret_value(mut input: impl Read) -> Result<SecretBytes> {
    // Allocate the full cap up front so secret bytes from stdin do not pass
    // through discarded intermediate Vec buffers during growth.
    let mut value = SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN);
    let mut buffer = Zeroizing::new([0_u8; 8192]);
    loop {
        let read = input
            .read(&mut buffer[..])
            .context("failed to read secret value from stdin")?;
        if read == 0 {
            return Ok(value);
        }
        if value.len() + read > MAX_SECRET_VALUE_LEN {
            bail!("secret value is larger than the {MAX_SECRET_VALUE_LEN} byte limit");
        }
        value.extend_from_slice(&buffer[..read])?;
    }
}

fn read_secret_value_from_prompt() -> Result<SecretBytes> {
    if !hidden_terminal_input_available() {
        bail!("--value-prompt requires an interactive terminal; use --value-stdin for automation");
    }
    let mut value =
        prompt_zeroizing("Secret value: ").context("failed to read secret value from terminal")?;
    Ok(SecretBytes::new(std::mem::take(&mut *value).into_bytes()))
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use tempfile::tempdir;

    use crate::test_env::{EnvVarGuard, lock_env};

    use super::*;

    #[test]
    fn parses_env_mappings() {
        let parsed = parse_env_mappings(&["TOKEN=api_token".into()]).unwrap();
        assert_eq!(parsed[0].var().as_str(), "TOKEN");
        assert_eq!(parsed[0].secret_name().as_str(), "api_token");
    }

    #[test]
    fn rejects_invalid_env_mapping_shape() {
        let error = parse_env_mappings(&["TOKEN".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("VAR=SECRET_NAME"));
    }

    #[test]
    fn rejects_invalid_env_mapping_secret_name_before_unlock() {
        let error = parse_env_mappings(&["TOKEN=bad secret".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported characters"));
    }

    #[test]
    fn read_secret_value_rejects_oversized_input() {
        let value = vec![b'x'; MAX_SECRET_VALUE_LEN + 1];
        let error = read_secret_value(std::io::Cursor::new(value))
            .unwrap_err()
            .to_string();
        assert!(error.contains("larger than"));
    }

    #[test]
    fn passphrase_clears_environment_after_reading() {
        let _env = lock_env();
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, "correct horse battery staple");
        capture_passphrase_from_env().unwrap();
        let _captured = passphrase().unwrap();
        assert!(std::env::var_os(PASSPHRASE_ENV).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn passphrase_parse_error_keeps_environment_for_retry() {
        use std::os::unix::ffi::OsStringExt;

        let _env = lock_env();
        let invalid = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
        let _passphrase = EnvVarGuard::set(PASSPHRASE_ENV, invalid);

        let error = capture_passphrase_from_env().unwrap_err().to_string();

        assert!(error.contains("valid UTF-8"));
        assert!(std::env::var_os(PASSPHRASE_ENV).is_some());
    }

    #[test]
    fn status_does_not_require_passphrase() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let output = status(VaultStatusRequest {
            vault: VaultRuntimeOptions {
                home: Some(home.clone()),
            },
        })
        .unwrap();
        assert_eq!(output["exists"], false);
        assert_eq!(output["vault_file_exists"], false);
        assert!(!home.exists());
    }

    #[test]
    fn status_reports_existing_vault() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("vault");
        let vault = Vault::resolve(Some(home.clone())).unwrap();
        vault
            .init(&SecretString::from(
                "correct horse battery staple".to_string(),
            ))
            .unwrap();
        let output = status(VaultStatusRequest {
            vault: VaultRuntimeOptions { home: Some(home) },
        })
        .unwrap();
        assert_eq!(output["exists"], true);
        assert_eq!(output["vault_file_exists"], true);
    }
}
