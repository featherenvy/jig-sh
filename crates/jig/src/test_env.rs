use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub(crate) fn lock_env() -> EnvLockGuard {
    // Tests mutate process-global environment; every env-mutating test must
    // hold this single crate-wide lock. Several env-driven flows also depend
    // on current_dir(), so the same guard serializes cwd mutation.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static CWD_LOCK: Mutex<()> = Mutex::new(());
    let lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cwd_lock = CWD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    EnvLockGuard {
        _jig_repo_root: EnvVarGuard::remove("JIG_REPO_ROOT"),
        _jig_invoke_cwd: EnvVarGuard::remove("JIG_INVOKE_CWD"),
        _cwd_lock: cwd_lock,
        _lock: lock,
    }
}

pub(crate) struct EnvLockGuard {
    _jig_repo_root: EnvVarGuard,
    _jig_invoke_cwd: EnvVarGuard,
    _cwd_lock: MutexGuard<'static, ()>,
    _lock: MutexGuard<'static, ()>,
}

pub(crate) struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub(crate) fn set(path: &Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).unwrap();
    }
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    pub(crate) fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
