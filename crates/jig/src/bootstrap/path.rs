use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub(super) const INVOCATION_CWD_ENV: &str = "JIG_INVOKE_CWD";

pub(super) fn absolute_path_from(path: &Path, base: &Path) -> Result<PathBuf> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    if resolved.exists() {
        fs::canonicalize(&resolved)
            .with_context(|| format!("Failed to canonicalize {}", resolved.display()))
    } else {
        Ok(resolved)
    }
}

pub(super) fn bootstrap_invocation_cwd() -> Result<PathBuf> {
    let Some(value) = env::var_os(INVOCATION_CWD_ENV) else {
        let cwd = env::current_dir().context("Failed to resolve current directory")?;
        return fs::canonicalize(&cwd).with_context(|| {
            format!(
                "Failed to canonicalize current directory: {}",
                cwd.display()
            )
        });
    };

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        bail!(
            "{INVOCATION_CWD_ENV} must be an absolute path: {}",
            path.display()
        );
    }
    if !path.is_dir() {
        bail!(
            "{INVOCATION_CWD_ENV} is not a directory: {}",
            path.display()
        );
    }
    fs::canonicalize(&path).with_context(|| {
        format!(
            "Failed to canonicalize {INVOCATION_CWD_ENV}: {}",
            path.display()
        )
    })
}
