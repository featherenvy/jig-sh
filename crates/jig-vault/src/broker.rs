use crate::Result;
use crate::run::{env_var_names_equal, is_preserved_env_var_name};
use crate::types::{EnvVarName, SecretName};

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct BrokeredEnv {
    var: EnvVarName,
    secret_name: SecretName,
}

impl BrokeredEnv {
    pub fn new(var: EnvVarName, secret_name: SecretName) -> Result<Self> {
        if is_preserved_env_var_name(var.as_str()) {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!(
                    "vault env mapping cannot inject a secret into preserved environment variable '{}'",
                    var.as_str()
                ),
            ));
        }
        Ok(Self { var, secret_name })
    }

    pub fn parse(value: &str) -> Result<Self> {
        let (var, secret_name) = value.split_once('=').ok_or_else(|| {
            crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!("vault env mapping '{value}' must have the form VAR=SECRET_NAME"),
            )
        })?;
        if var.is_empty() || secret_name.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!("vault env mapping '{value}' must have non-empty VAR and SECRET_NAME"),
            ));
        }
        Self::new(EnvVarName::parse(var)?, SecretName::parse(secret_name)?)
    }

    pub fn var(&self) -> &EnvVarName {
        &self.var
    }

    pub fn secret_name(&self) -> &SecretName {
        &self.secret_name
    }

    pub(crate) fn into_parts(self) -> (EnvVarName, SecretName) {
        (self.var, self.secret_name)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct BrokeredRun {
    command: Vec<String>,
    env: Vec<BrokeredEnv>,
}

impl BrokeredRun {
    pub fn new(command: Vec<String>, env: Vec<BrokeredEnv>) -> Result<Self> {
        if command.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                "vault run requires a command after --",
            ));
        }
        for (index, mapping) in env.iter().enumerate() {
            if env[..index]
                .iter()
                .any(|prior| env_var_names_equal(prior.var().as_str(), mapping.var().as_str()))
            {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    format!(
                        "vault env mapping specifies environment variable '{}' more than once",
                        mapping.var().as_str()
                    ),
                ));
            }
        }
        Ok(Self { command, env })
    }

    pub fn command(&self) -> &[String] {
        &self.command
    }

    pub fn env(&self) -> &[BrokeredEnv] {
        &self.env
    }

    pub(crate) fn into_parts(self) -> (Vec<String>, Vec<BrokeredEnv>) {
        (self.command, self.env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    use crate::VaultErrorKind;
    use crate::store::VaultStore;

    #[test]
    fn brokered_env_rejects_equals_in_secret_name() {
        let error = BrokeredEnv::parse("TOKEN=bad=name")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported characters"));
    }

    #[test]
    fn brokered_env_rejects_preserved_environment_names() {
        let error = BrokeredEnv::parse("PATH=api_token")
            .unwrap_err()
            .to_string();
        assert!(error.contains("preserved environment variable"));
    }

    #[test]
    fn brokered_run_rejects_duplicate_env_names() {
        let error = BrokeredRun::new(
            vec!["true".into()],
            vec![
                BrokeredEnv::parse("TOKEN=api_token").unwrap(),
                BrokeredEnv::parse("TOKEN=other_token").unwrap(),
            ],
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("more than once"));
    }

    #[test]
    fn brokered_run_resolution_failure_records_failure_audit_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_string());
        store.init(&passphrase).unwrap();

        let error = store
            .run_brokered(
                &passphrase,
                BrokeredRun::new(
                    vec!["true".into()],
                    vec![
                        BrokeredEnv::new(
                            EnvVarName::parse("TOKEN").unwrap(),
                            SecretName::parse("missing_token").unwrap(),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(error.kind(), VaultErrorKind::NotFound);
        let audit = store.read_audit_text().unwrap().unwrap();
        assert!(audit.contains("\"run_id\""));
        assert!(audit.contains("\"stage\":\"resolve\""));
        assert!(audit.contains("\"action\":\"brokered_run_start\""));
        let verification = store.verify_audit(&passphrase).unwrap();
        assert_eq!(verification.event_count, 3);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_brokered_runs_keep_a_valid_audit_chain() {
        use std::sync::{Arc, Barrier};

        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_string());
        store.init(&passphrase).unwrap();
        store
            .set_secret(
                &passphrase,
                "api_token",
                crate::SecretBytes::new(b"secret-value".to_vec()),
            )
            .unwrap();

        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                store
                    .run_brokered(
                        &SecretString::from("correct horse battery staple".to_string()),
                        BrokeredRun::new(
                            vec![
                                "sh".into(),
                                "-c".into(),
                                "sleep 0.05; printf '%s' \"$TOKEN\"".into(),
                            ],
                            vec![
                                BrokeredEnv::new(
                                    EnvVarName::parse("TOKEN").unwrap(),
                                    SecretName::parse("api_token").unwrap(),
                                )
                                .unwrap(),
                            ],
                        )
                        .unwrap(),
                    )
                    .unwrap()
            }));
        }

        for handle in handles {
            let output = handle.join().unwrap();
            assert_eq!(output.exit_status, 0);
            assert_eq!(output.stdout, "[REDACTED]");
        }
        let verification = store.verify_audit(&passphrase).unwrap();
        assert_eq!(verification.event_count, 6);
        assert_eq!(verification.torn_tail_bytes, 0);
    }
}
