use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::file_ops;
use crate::host::validate_routed_hostname;
use crate::types::{Route, RouteMode};

const ROUTES_VERSION: u32 = 1;
const ROUTES_FILE: &str = "routes.json";
const LOCK_FILE: &str = "routes.lock";
const RUNTIME_LOCK_FILE: &str = "runtime.lock";
const PID_FILE: &str = "proxy.pid";
const EXE_FILE: &str = "proxy-exe.txt";
const HTTP_PORT_FILE: &str = "proxy-http.port";
const HTTPS_PORT_FILE: &str = "proxy-https.port";
const HEALTH_TOKEN_FILE: &str = "proxy-health-token";
const LEAF_HOSTS_FILE: &str = "leaf-hosts.json";
const CERT_LOCK_FILE: &str = "certs.lock";
const STATE_FILE_FALLBACK: &str = "jig-proxy-state";
#[cfg(not(test))]
const REPLACE_BACKUP_RECOVERY_DELAY: Duration = Duration::from_secs(30);
#[cfg(test)]
const REPLACE_BACKUP_RECOVERY_DELAY: Duration = Duration::ZERO;
const MISSING_FILE_READ_RETRY_DELAY: Duration = Duration::from_millis(25);
const MISSING_FILE_READ_ATTEMPTS: usize = 3;
const MAX_ROUTES_FILE_BYTES: u64 = 4 * 1024 * 1024;
const STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const STATE_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) type FileSignature = Option<(SystemTime, u64, [u8; 32])>;

static CLOCK_WARNING_PRINTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ROUTE_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
    static CERT_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct StateStore {
    // Clones are cheap handles to the same state directory. They do not share
    // in-memory lock state; every mutating operation reacquires the advisory
    // file lock for the specific route/runtime/cert file it touches.
    root: PathBuf,
    can_chmod_root: bool,
}

impl StateStore {
    pub(crate) fn resolve(explicit: Option<PathBuf>) -> Result<Self> {
        let (root, can_chmod_existing) = if let Some(path) = explicit {
            (path, false)
        } else if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
            (PathBuf::from(path), false)
        } else {
            (
                dirs::home_dir()
                    .context("Could not resolve home directory for Jig proxy state")?
                    .join(".jig/proxy"),
                true,
            )
        };
        if path_is_symlink(&root)? {
            anyhow::bail!(
                "Proxy state dir {} must not be a symlink. Use a dedicated real directory.",
                root.display()
            );
        }
        ensure_state_create_ancestor_is_not_shared_writable(&root)?;
        let default_parent_existed = can_chmod_existing
            && root
                .parent()
                .is_some_and(|parent| parent.try_exists().unwrap_or(false));
        let existed = root.exists();
        fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create proxy state dir {}", root.display()))?;
        if path_is_symlink(&root)? {
            anyhow::bail!(
                "Proxy state dir {} became a symlink while it was being prepared. Use a dedicated real directory.",
                root.display()
            );
        }
        let root = fs::canonicalize(&root)
            .with_context(|| format!("Failed to resolve proxy state dir {}", root.display()))?;
        if can_chmod_existing {
            ensure_default_state_parent_permissions(&root, default_parent_existed)?;
        }
        let can_chmod_root = can_chmod_existing || !existed || existing_dir_is_empty(&root);
        ensure_state_dir_has_no_symlinks(&root)?;
        ensure_state_dir_permissions(&root, can_chmod_root)?;
        // Re-scan after chmod/ACL hardening because Windows `icacls /T` is
        // recursive and must not be applied through a just-created symlink.
        ensure_state_dir_has_no_symlinks(&root)?;
        recover_replace_backups_with_lock(&root)?;
        Ok(Self {
            root,
            can_chmod_root,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn ca_path(&self) -> PathBuf {
        self.root.join("ca.pem")
    }

    pub(crate) fn ca_key_path(&self) -> PathBuf {
        self.root.join("ca-key.pem")
    }

    pub(crate) fn leaf_path(&self) -> PathBuf {
        self.root.join("leaf.pem")
    }

    pub(crate) fn leaf_key_path(&self) -> PathBuf {
        self.root.join("leaf-key.pem")
    }

    pub(crate) fn leaf_hosts_path(&self) -> PathBuf {
        self.root.join(LEAF_HOSTS_FILE)
    }

    pub(crate) fn trusted_ca_path(&self) -> PathBuf {
        self.root.join("trusted-ca.json")
    }

    pub(crate) fn log_path(&self) -> PathBuf {
        self.root.join("proxy.log")
    }

    pub(crate) fn pid_path(&self) -> PathBuf {
        self.root.join(PID_FILE)
    }

    pub(crate) fn proxy_exe_path(&self) -> PathBuf {
        self.root.join(EXE_FILE)
    }

    pub(crate) fn http_port_path(&self) -> PathBuf {
        self.root.join(HTTP_PORT_FILE)
    }

    pub(crate) fn https_port_path(&self) -> PathBuf {
        self.root.join(HTTPS_PORT_FILE)
    }

    pub(crate) fn health_token_path(&self) -> PathBuf {
        self.root.join(HEALTH_TOKEN_FILE)
    }

    pub(crate) fn read_routes(&self, prune_dead: bool) -> Result<Vec<Route>> {
        let routes = self.with_route_lock(read_routes_from_path)?;
        if !prune_dead {
            return Ok(routes);
        }
        // Prune-on-read is intentionally in-memory only. Use `prune` when the
        // caller needs the persisted routes file rewritten.
        Ok(routes.into_iter().filter(route_is_alive).collect())
    }

    #[cfg(test)]
    pub(crate) fn add_route(&self, route: Route) -> Result<()> {
        if route.mode == RouteMode::Process && !process_start_tokens_supported() {
            anyhow::bail!(
                "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
            );
        }
        self.with_route_lock(|path| add_route_to_path(path, route))
    }

    pub(crate) fn add_verified_route<F>(&self, route: Route, mut verify: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        if route.mode == RouteMode::Process && !process_start_tokens_supported() {
            anyhow::bail!(
                "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
            );
        }
        self.with_route_lock(|path| add_route_to_path_verified(path, route, &mut verify))
    }

    pub(crate) fn add_alias_route(&self, route: Route) -> Result<()> {
        if route.mode != RouteMode::Alias {
            anyhow::bail!("add_alias_route requires RouteMode::Alias");
        }
        self.with_route_lock(|path| add_route_to_path(path, route))
    }

    pub(crate) fn remove_route(&self, hostname: &str) -> Result<()> {
        let hostname = hostname.to_ascii_lowercase();
        self.with_route_lock(|path| {
            let mut routes = read_routes_from_path(path)?;
            routes.retain(|existing| existing.hostname.as_str() != hostname);
            routes.retain(route_is_alive);
            write_routes_to_path(path, &routes)
        })
    }

    pub(crate) fn prune(&self) -> Result<Vec<Route>> {
        self.with_route_lock(|path| {
            let routes = read_routes_from_path(path)?;
            let original_len = routes.len();
            let pruned: Vec<_> = routes.into_iter().filter(route_is_alive).collect();
            if pruned.len() != original_len {
                write_routes_to_path(path, &pruned)?;
            }
            Ok(pruned)
        })
    }

    #[cfg(test)]
    pub(crate) fn write_pid(&self, pid: u32) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(self.pid_path(), &pid.to_string(), STATE_FILE_FALLBACK)
        })
    }

    pub(crate) fn read_pid(&self) -> Result<Option<u32>> {
        self.with_runtime_lock(|| self.read_pid_unlocked())
    }

    fn read_pid_unlocked(&self) -> Result<Option<u32>> {
        // PID files are only identity hints; callers that need process identity
        // should pair this with the health-token handshake or a start token.
        let path = self.pid_path();
        let Some(text) = file_ops::read_text_no_follow(&path)? else {
            return Ok(None);
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        match text.parse() {
            Ok(pid) => Ok(Some(pid)),
            Err(error) => anyhow::bail!(
                "Invalid Jig proxy PID file {}: {error}. Remove the file or run with a clean JIG_PROXY_STATE_DIR.",
                self.pid_path().display()
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn write_proxy_exe(&self, path: &Path) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.proxy_exe_path(),
                &path.to_string_lossy(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    pub(crate) fn read_proxy_exe_status(&self) -> Result<ProxyExeStatus> {
        self.with_runtime_lock(|| Ok(self.read_proxy_exe_status_unlocked()))
    }

    fn read_proxy_exe_status_unlocked(&self) -> ProxyExeStatus {
        let text = match file_ops::read_text_no_follow(&self.proxy_exe_path()) {
            Ok(Some(text)) => text,
            Ok(None) => return ProxyExeStatus::default(),
            Err(error) => {
                return ProxyExeStatus {
                    path: None,
                    warning: Some(format!(
                        "Could not read recorded proxy executable path {}: {error}",
                        self.proxy_exe_path().display()
                    )),
                };
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return ProxyExeStatus::default();
        }
        let recorded = PathBuf::from(text);
        let path = match recorded.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                return ProxyExeStatus {
                    path: None,
                    warning: Some(format!(
                        "Recorded proxy executable {} is not available: {error}",
                        recorded.display()
                    )),
                };
            }
        };
        if proxy_exe_is_usable(&path) {
            ProxyExeStatus {
                path: Some(path),
                warning: None,
            }
        } else {
            ProxyExeStatus {
                path: None,
                warning: Some(format!(
                    "Recorded proxy executable {} is not a usable executable file",
                    path.display()
                )),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn write_http_port(&self, port: u16) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.http_port_path(),
                &port.to_string(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn write_https_port(&self, port: u16) -> Result<()> {
        self.with_runtime_lock(|| {
            file_ops::write_atomic_text(
                self.https_port_path(),
                &port.to_string(),
                STATE_FILE_FALLBACK,
            )
        })
    }

    pub(crate) fn read_http_port(&self) -> Result<Option<u16>> {
        self.with_runtime_lock(|| Ok(read_port_file(&self.http_port_path())))
    }

    pub(crate) fn read_https_port(&self) -> Result<Option<u16>> {
        self.with_runtime_lock(|| Ok(read_port_file(&self.https_port_path())))
    }

    #[cfg(test)]
    pub(crate) fn ensure_health_token(&self) -> Result<String> {
        self.with_runtime_lock(|| {
            if let Some(token) = read_health_token_file(&self.health_token_path())? {
                return Ok(token);
            }
            let token = random_health_token()?;
            file_ops::write_atomic_text(self.health_token_path(), &token, STATE_FILE_FALLBACK)?;
            Ok(token)
        })
    }

    pub(crate) fn read_health_token(&self) -> Result<Option<String>> {
        self.with_runtime_lock(|| read_health_token_file(&self.health_token_path()))
    }

    pub(crate) fn clear_runtime_files(&self) {
        if let Err(error) = self.try_clear_runtime_files() {
            eprintln!(
                "jig proxy could not clear runtime files in {}: {error}",
                self.root.display()
            );
        }
    }

    pub(crate) fn try_clear_runtime_files(&self) -> Result<()> {
        self.with_runtime_lock(|| self.remove_runtime_files_unlocked())
    }

    pub(crate) fn replace_runtime_files(
        &self,
        current_exe: &Path,
        http_port: u16,
        https_port: Option<u16>,
    ) -> Result<String> {
        self.with_runtime_lock(|| {
            self.remove_runtime_files_unlocked()?;
            let health_token = random_health_token()?;
            file_ops::write_atomic_text(
                self.health_token_path(),
                &health_token,
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.pid_path(),
                &std::process::id().to_string(),
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.proxy_exe_path(),
                &current_exe.to_string_lossy(),
                STATE_FILE_FALLBACK,
            )?;
            file_ops::write_atomic_text(
                self.http_port_path(),
                &http_port.to_string(),
                STATE_FILE_FALLBACK,
            )?;
            if let Some(port) = https_port {
                file_ops::write_atomic_text(
                    self.https_port_path(),
                    &port.to_string(),
                    STATE_FILE_FALLBACK,
                )?;
            }
            Ok(health_token)
        })
    }

    pub(crate) fn with_cert_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let _lock_order_guard = enter_lock_order_guard(LockKind::Cert)?;
        let lock = open_lock_file(self.root.join(CERT_LOCK_FILE))?;
        lock_state_file(&lock, "cert lock")?;
        let lock = LockedFile::new(lock, "cert lock");
        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("cert lock", result, unlock_result)
    }

    pub(crate) fn routes_signature(&self) -> FileSignature {
        file_signature(&self.routes_path())
    }

    fn routes_path(&self) -> PathBuf {
        self.root.join(ROUTES_FILE)
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_FILE)
    }

    fn runtime_lock_path(&self) -> PathBuf {
        self.root.join(RUNTIME_LOCK_FILE)
    }

    fn with_route_lock<T>(&self, f: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.lock_path())?;
        lock_state_file(&lock, "route lock")?;
        let lock = LockedFile::new(lock, "route lock");
        let _lock_order_guard = enter_lock_order_guard(LockKind::Route)?;
        recover_replace_backups(&self.root)?;

        let routes_path = self.routes_path();
        let result = f(&routes_path);
        let unlock_result = lock.unlock();
        finish_with_unlock("route lock", result, unlock_result)
    }

    fn with_runtime_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.root)?;
        ensure_state_dir_has_no_symlinks(&self.root)?;
        ensure_state_dir_permissions(&self.root, self.can_chmod_root)?;
        // Keep the post-hardening scan close to every recursive ACL/chmod pass.
        ensure_state_dir_has_no_symlinks(&self.root)?;
        let lock = open_lock_file(self.runtime_lock_path())?;
        lock_state_file(&lock, "runtime lock")?;
        let lock = LockedFile::new(lock, "runtime lock");

        let result = f();
        let unlock_result = lock.unlock();
        finish_with_unlock("runtime lock", result, unlock_result)
    }

    fn remove_runtime_files_unlocked(&self) -> Result<()> {
        remove_runtime_file(self.pid_path())?;
        remove_runtime_file(self.proxy_exe_path())?;
        remove_runtime_file(self.http_port_path())?;
        remove_runtime_file(self.https_port_path())?;
        remove_runtime_file(self.health_token_path())?;
        Ok(())
    }
}

fn remove_runtime_file(path: PathBuf) -> Result<()> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("Failed to remove runtime file {}", path.display()))
        }
    }
}

#[derive(Default)]
pub(crate) struct ProxyExeStatus {
    pub(crate) path: Option<PathBuf>,
    pub(crate) warning: Option<String>,
}

struct LockedFile {
    file: File,
    label: &'static str,
    unlocked: bool,
}

#[derive(Clone, Copy)]
enum LockKind {
    Route,
    Cert,
}

struct LockOrderGuard {
    kind: LockKind,
}

fn enter_lock_order_guard(kind: LockKind) -> Result<LockOrderGuard> {
    match kind {
        LockKind::Route => {
            ROUTE_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
        LockKind::Cert => {
            if ROUTE_LOCK_DEPTH.with(|depth| depth.get()) != 0 {
                anyhow::bail!("cert lock cannot be acquired while a route lock is held");
            }
            CERT_LOCK_DEPTH.with(|depth| depth.set(depth.get() + 1));
        }
    }
    Ok(LockOrderGuard { kind })
}

impl Drop for LockOrderGuard {
    fn drop(&mut self) {
        match self.kind {
            LockKind::Route => {
                ROUTE_LOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            }
            LockKind::Cert => {
                CERT_LOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
            }
        }
    }
}

impl LockedFile {
    fn new(file: File, label: &'static str) -> Self {
        Self {
            file,
            label,
            unlocked: false,
        }
    }

    fn unlock(mut self) -> std::io::Result<()> {
        let result = FileExt::unlock(&self.file);
        if result.is_ok() {
            self.unlocked = true;
        }
        result
    }
}

impl Drop for LockedFile {
    fn drop(&mut self) {
        if !self.unlocked {
            if let Err(error) = FileExt::unlock(&self.file) {
                let _ = writeln!(
                    std::io::stderr(),
                    "jig proxy failed to unlock {} while dropping lock guard: {error}",
                    self.label
                );
            }
        }
    }
}

fn add_route_to_path(path: &Path, route: Route) -> Result<()> {
    validate_route_for_write(&route)?;
    let mut routes = read_routes_from_path(path)?;
    ensure_no_live_process_route_replacement(&routes, &route)?;
    routes.retain(|existing| existing.hostname != route.hostname);
    routes.retain(route_is_alive);
    routes.push(route);
    write_routes_to_path(path, &routes)
}

fn add_route_to_path_verified<F>(path: &Path, route: Route, verify: &mut F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    validate_route_for_write(&route)?;
    let mut routes = read_routes_from_path(path)?;
    ensure_no_live_process_route_replacement(&routes, &route)?;
    verify()?;
    let rollback_routes = routes.clone();
    routes.retain(|existing| existing.hostname != route.hostname);
    routes.retain(route_is_alive);
    routes.push(route.clone());
    write_routes_to_path(path, &routes)?;
    if let Err(error) = verify() {
        if let Err(cleanup_error) = write_routes_to_path(path, &rollback_routes) {
            return Err(error).context(format!(
                "verification failed for route '{}', and rollback also failed: {cleanup_error}",
                route.hostname
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn finish_with_unlock<T>(
    label: &str,
    result: Result<T>,
    unlock_result: std::io::Result<()>,
) -> Result<T> {
    match (result, unlock_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(error)) => Err(error.into()),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(unlock_error)) => {
            eprintln!("jig proxy failed to unlock {label} after an earlier error: {unlock_error}");
            Err(error)
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(error) => {
            if !CLOCK_WARNING_PRINTED.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "jig proxy system clock is before the Unix epoch; route timestamps will use 0: {error}"
                );
            }
            0
        }
    }
}

pub(crate) fn route_is_alive(route: &Route) -> bool {
    match route.mode {
        RouteMode::Alias => true,
        RouteMode::Process => route
            .owner_pid
            .is_some_and(|pid| pid_matches_owner(pid, route.owner_start_token.as_deref())),
    }
}

fn pid_matches_owner(pid: u32, expected_start_token: Option<&str>) -> bool {
    if !pid_is_alive(pid) {
        return false;
    }
    let Some(expected_start_token) = expected_start_token else {
        return false;
    };
    if !process_start_tokens_supported() {
        return false;
    }
    process_start_token(pid)
        .as_deref()
        .is_some_and(|current_start_token| current_start_token == expected_start_token)
}

pub(crate) fn process_start_tokens_supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

pub(crate) fn pid_is_alive(pid: u32) -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(pid) = i32::try_from(pid).ok() else {
            return false;
        };
        #[cfg(target_os = "linux")]
        if linux_process_is_zombie(pid as u32) {
            return false;
        }
        unsafe {
            // SAFETY: pid was range-checked above. Signal 0 performs permission
            // and existence checks without delivering a signal.
            libc::kill(pid, 0) == 0
        }
    }
    #[cfg(target_os = "macos")]
    {
        macos_proc_bsdinfo(pid).is_some_and(|info| info.pbi_status != libc::SZOMB)
    }
    #[cfg(windows)]
    {
        unsafe {
            // SAFETY: OpenProcess does not take ownership of any Rust memory. The
            // returned handle is closed on every non-null path below.
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut exit_code = 0u32;
            // SAFETY: `handle` is a live process handle from OpenProcess and
            // `exit_code` is a valid out pointer for the duration of the call.
            let ok = GetExitCodeProcess(handle, &mut exit_code);
            // SAFETY: `handle` was returned by OpenProcess and has not been closed.
            let _ = CloseHandle(handle);
            ok != 0 && exit_code == STILL_ACTIVE as u32
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(target_os = "linux")]
fn linux_process_is_zombie(pid: u32) -> bool {
    // Hardened procfs mounts can hide status from non-owners. Treat an
    // unreadable status as non-zombie; route liveness still requires the
    // process start token to match before a process route is trusted.
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("State:"))
                .map(str::trim_start)
                .and_then(|state| state.chars().next())
        })
        == Some('Z')
}

#[cfg(test)]
fn windows_tasklist_csv_pid(line: &str) -> Option<u32> {
    csv_fields(line).get(1)?.parse().ok()
}

#[cfg(test)]
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                let _ = chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

#[cfg(target_os = "linux")]
pub(crate) fn process_start_token(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    let start_time_ticks = fields.split_whitespace().nth(19)?;
    Some(format!("linux:{start_time_ticks}"))
}

#[cfg(target_os = "macos")]
pub(crate) fn process_start_token(pid: u32) -> Option<String> {
    let info = macos_proc_bsdinfo(pid)?;
    if info.pbi_status == libc::SZOMB {
        return None;
    }
    Some(format!(
        "macos:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "macos")]
fn macos_proc_bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = i32::try_from(pid).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let bytes = unsafe {
        // SAFETY: info points to a writable proc_bsdinfo-sized buffer, pid was
        // range-checked above, and PROC_PIDTBSDINFO writes at most the supplied
        // buffer size. The byte count is checked before assume_init.
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size.try_into().ok()?,
        )
    };
    if bytes < size.try_into().ok()? {
        return None;
    }
    Some(unsafe {
        // SAFETY: proc_pidinfo reported that it initialized the full
        // proc_bsdinfo-sized buffer above.
        info.assume_init()
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos",)))]
pub(crate) fn process_start_token(_pid: u32) -> Option<String> {
    None
}

#[cfg(windows)]
fn windows_system32_tool(name: &str) -> PathBuf {
    PathBuf::from(r"C:\Windows\System32").join(name)
}

fn read_routes_from_file(file: &mut File) -> Result<Vec<Route>> {
    file.seek(SeekFrom::Start(0))?;
    let len = file.metadata()?.len();
    if len > MAX_ROUTES_FILE_BYTES {
        anyhow::bail!(
            "Jig proxy routes file is {len} bytes, above the {MAX_ROUTES_FILE_BYTES} byte limit"
        );
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let routes = match serde_json::from_str::<RoutesFile>(&text)
        .context("Failed to parse Jig proxy routes")?
    {
        RoutesFile::Versioned(document) if document.version == ROUTES_VERSION => document.routes,
        RoutesFile::Versioned(document) => {
            let version = document.version;
            anyhow::bail!("Unsupported Jig proxy routes version {version}");
        }
        RoutesFile::Legacy(routes) => routes,
    };
    for route in &routes {
        validate_route_for_read(route)?;
    }
    Ok(routes)
}

fn read_routes_from_path(path: &Path) -> Result<Vec<Route>> {
    let mut file = match open_read_no_follow_maybe_missing(path)? {
        Some(file) => file,
        None => return Ok(Vec::new()),
    };
    ensure_private_state_file_permissions(path, &file)?;
    read_routes_from_file(&mut file)
}

fn ensure_private_state_file_permissions(path: &Path, file: &File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "private state file {} must have mode 600, found {:o}",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

fn open_read_no_follow_maybe_missing(path: &Path) -> std::io::Result<Option<File>> {
    for attempt in 0..MISSING_FILE_READ_ATTEMPTS {
        match file_ops::open_read_no_follow(path) {
            Ok(file) => return Ok(Some(file)),
            Err(error)
                if cfg!(windows)
                    && error.kind() == std::io::ErrorKind::NotFound
                    && attempt + 1 < MISSING_FILE_READ_ATTEMPTS =>
            {
                std::thread::sleep(MISSING_FILE_READ_RETRY_DELAY);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return missing_file_read_result(path, cfg!(windows));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn missing_file_read_result(
    path: &Path,
    fail_on_replace_backup: bool,
) -> std::io::Result<Option<File>> {
    if fail_on_replace_backup && file_ops::replace_backup_for_path_exists(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "state file {} is temporarily unavailable during replacement",
                path.display()
            ),
        ));
    }
    Ok(None)
}

fn validate_route(route: &Route) -> Result<()> {
    validate_routed_hostname(&route.hostname)?;
    crate::host::validate_route_target_host(&route.target_host)?;
    if route.target_port == 0 {
        anyhow::bail!("Jig proxy route '{}' has target port 0", route.hostname);
    }
    Ok(())
}

fn validate_route_for_write(route: &Route) -> Result<()> {
    validate_route(route)?;
    match route.mode {
        RouteMode::Alias => {}
        RouteMode::Process => {
            if route.owner_pid.is_none() || route.owner_start_token.is_none() {
                anyhow::bail!(
                    "Process route '{}' must include owner PID and start token before it can be persisted",
                    route.hostname
                );
            }
        }
    }
    Ok(())
}

fn validate_route_for_read(route: &Route) -> Result<()> {
    validate_route_for_write(route)?;
    if route.mode == RouteMode::Process && !process_start_tokens_supported() {
        anyhow::bail!(
            "Process route '{}' cannot be trusted on this platform because process start-token verification is unavailable",
            route.hostname
        );
    }
    Ok(())
}

fn ensure_no_live_process_route_replacement(routes: &[Route], route: &Route) -> Result<()> {
    if routes.iter().any(|existing| {
        existing.hostname == route.hostname
            && existing.mode == RouteMode::Process
            && route_is_alive(existing)
    }) {
        anyhow::bail!(
            "Proxy route '{}' would replace a live process route. Stop the running app or remove its route before reusing that hostname.",
            route.hostname
        );
    }
    Ok(())
}

fn write_routes_to_path(path: &Path, routes: &[Route]) -> Result<()> {
    let tmp = file_ops::temp_path(path, "jig-proxy-state");
    let mut file = file_ops::create_new_file(&tmp, 0o600)?;
    serde_json::to_writer_pretty(
        &mut file,
        &RoutesDocument {
            version: ROUTES_VERSION,
            routes,
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path, "jig-proxy-state")?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RoutesFile {
    Versioned(RoutesDocumentOwned),
    Legacy(Vec<Route>),
}

#[derive(Deserialize)]
struct RoutesDocumentOwned {
    version: u32,
    routes: Vec<Route>,
}

#[derive(Serialize)]
struct RoutesDocument<'a> {
    version: u32,
    routes: &'a [Route],
}

fn read_port_file(path: &Path) -> Option<u16> {
    file_ops::read_text_no_follow(path)
        .ok()
        .flatten()?
        .trim()
        .parse()
        .ok()
}

fn read_health_token_file(path: &Path) -> Result<Option<String>> {
    let Some(text) = read_private_text_no_follow(path)? else {
        return Ok(None);
    };
    let token = text.trim();
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(Some(token.to_string()))
    } else {
        eprintln!(
            "jig proxy ignored corrupt health token file {}; a new token will be written when the proxy starts",
            path.display()
        );
        Ok(None)
    }
}

fn read_private_text_no_follow(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = match file_ops::open_read_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o7777;
        if mode != 0o600 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "private state file {} must have mode 600, found {:o}",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

fn random_health_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| {
        anyhow::anyhow!("Failed to generate proxy health token with getrandom: {error}")
    })?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut token, "{byte:02x}")?;
    }
    Ok(token)
}

fn open_lock_file(path: PathBuf) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "state lock is not a regular file",
        )
        .into());
    }
    #[cfg(unix)]
    {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn lock_state_file(file: &File, label: &str) -> Result<()> {
    let deadline = Instant::now() + STATE_LOCK_TIMEOUT;
    loop {
        match file.try_lock_exclusive() {
            Ok(true) => return Ok(()),
            Ok(false) => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "Timed out waiting for Jig proxy {label} after {:?}",
                        STATE_LOCK_TIMEOUT
                    );
                }
                std::thread::sleep(STATE_LOCK_POLL_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn file_signature(path: &Path) -> FileSignature {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len(), file_hash(path)?))
}

fn file_hash(path: &Path) -> Option<[u8; 32]> {
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Some(out)
}

fn ensure_state_dir_permissions(path: &Path, can_chmod: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if can_chmod {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            return Ok(());
        }
        let mode = fs::metadata(path)?.permissions().mode() & 0o7777;
        if mode != 0o700 {
            anyhow::bail!(
                "Proxy state dir {} already exists with permissions {:o}. Use a dedicated directory with mode 700 or let Jig create it.",
                path.display(),
                mode
            );
        }
    }
    #[cfg(windows)]
    {
        let _ = can_chmod;
        harden_windows_state_dir(path)?;
        return Ok(());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, can_chmod);
    }
    Ok(())
}

#[cfg(windows)]
fn harden_windows_state_dir(path: &Path) -> Result<()> {
    let account = current_windows_account()?;
    let grant = format!("{account}:(OI)(CI)F");
    let output = Command::new(windows_system32_tool("icacls.exe"))
        .arg(path)
        .args([
            "/inheritance:r",
            "/grant:r",
            &grant,
            "/remove:g",
            "*S-1-1-0",
            "*S-1-5-11",
            "*S-1-5-32-545",
            "/T",
        ])
        .output()
        .with_context(|| format!("Failed to run icacls for {}", path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "Failed to apply owner-only ACL to proxy state dir {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn current_windows_account() -> Result<String> {
    let user = std::env::var("USERNAME").context("USERNAME is not set")?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    if domain.is_empty() {
        Ok(user)
    } else {
        Ok(format!("{domain}\\{user}"))
    }
}

fn ensure_default_state_parent_permissions(path: &Path, existed: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(parent) = path.parent() {
            if existed {
                let mode = fs::metadata(parent)?.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    anyhow::bail!(
                        "Default proxy state parent {} already exists with permissions {:o}. Tighten it to mode 700 before using the default state dir, or set JIG_PROXY_STATE_DIR to a dedicated private directory.",
                        parent.display(),
                        mode
                    );
                }
            } else {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (path, existed);
    Ok(())
}

fn ensure_state_create_ancestor_is_not_shared_writable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut ancestor = path;
        while !ancestor.try_exists()? {
            ancestor = ancestor
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
        }
        let metadata = fs::symlink_metadata(ancestor)?;
        let metadata = if metadata.file_type().is_symlink() {
            let parent = ancestor
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            ensure_existing_state_ancestor_is_not_shared_writable(path, parent)?;
            fs::metadata(ancestor)?
        } else {
            metadata
        };
        if !metadata.is_dir() {
            anyhow::bail!(
                "Proxy state dir {} would be created under non-directory ancestor {}.",
                path.display(),
                ancestor.display()
            );
        }
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            anyhow::bail!(
                "Proxy state dir {} would be created under shared-writable ancestor {} with permissions {:o}. Use a dedicated private directory.",
                path.display(),
                ancestor.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn ensure_existing_state_ancestor_is_not_shared_writable(
    path: &Path,
    ancestor: &Path,
) -> Result<()> {
    let metadata = fs::metadata(ancestor)?;
    if !metadata.is_dir() {
        anyhow::bail!(
            "Proxy state dir {} would be created under non-directory ancestor {}.",
            path.display(),
            ancestor.display()
        );
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o022 != 0 {
        anyhow::bail!(
            "Proxy state dir {} would be created under shared-writable ancestor {} with permissions {:o}. Use a dedicated private directory.",
            path.display(),
            ancestor.display(),
            mode
        );
    }
    Ok(())
}

fn ensure_state_dir_has_no_symlinks(path: &Path) -> Result<()> {
    ensure_state_tree_has_no_symlinks(path, path)
}

fn ensure_state_tree_has_no_symlinks(root: &Path, path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)?;
        if metadata.file_type().is_symlink() {
            anyhow::bail!(
                "Proxy state dir {} contains symlink {}. Use a dedicated state directory without symlinks.",
                root.display(),
                entry_path.display()
            );
        }
        if metadata.is_dir() {
            ensure_state_tree_has_no_symlinks(root, &entry_path)?;
        }
    }
    Ok(())
}

fn recover_replace_backups(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some((original_name, backup_pid)) = file_ops::replace_backup_parts(file_name) else {
            continue;
        };
        let original = root.join(original_name);
        if original.exists() {
            if replace_backup_is_stale(&entry.path()) {
                let _ = fs::remove_file(entry.path());
            }
        } else if replace_backup_can_be_promoted(&entry.path(), backup_pid) {
            match fs::rename(entry.path(), original) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

fn recover_replace_backups_with_lock(root: &Path) -> Result<()> {
    let lock = open_lock_file(root.join(LOCK_FILE))?;
    lock_state_file(&lock, "route recovery lock")?;
    let lock = LockedFile::new(lock, "route recovery lock");
    let result = recover_replace_backups(root);
    let unlock_result = lock.unlock();
    finish_with_unlock("route recovery lock", result, unlock_result)
}

fn replace_backup_can_be_promoted(path: &Path, backup_pid: &str) -> bool {
    if !process_start_tokens_supported() {
        return false;
    }
    let stale = replace_backup_is_stale(path);
    // A fresh backup from this process can only be from the currently executing
    // recovery/write path. Older same-pid backups are treated as stale PID reuse.
    if backup_pid == std::process::id().to_string() && !stale {
        return false;
    }
    stale
        || backup_pid
            .parse()
            .ok()
            .is_some_and(|pid| !pid_is_alive(pid))
}

fn replace_backup_is_stale(path: &Path) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    match modified.elapsed() {
        Ok(age) => age >= REPLACE_BACKUP_RECOVERY_DELAY,
        Err(_) => true,
    }
}

fn existing_dir_is_empty(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn path_is_symlink(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn proxy_exe_is_usable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use tempfile::tempdir;

    use super::*;

    fn write_private_routes_fixture(store: &StateStore, contents: impl AsRef<[u8]>) {
        fs::write(store.routes_path(), contents).unwrap();
        #[cfg(unix)]
        fs::set_permissions(store.routes_path(), fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn add_replaces_existing_route() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4001,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();
        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].target_port, 4001);
        let text = fs::read_to_string(store.routes_path()).unwrap();
        assert!(text.contains(r#""version": 1"#));
        assert!(text.contains(r#""routes""#));
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.routes_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.lock_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn verified_route_rolls_back_if_post_write_verification_fails() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let mut calls = 0usize;
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 3999,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let error = store
            .add_verified_route(
                Route {
                    hostname: "web.localhost".into(),
                    target_host: "127.0.0.1".into(),
                    target_port: 4000,
                    owner_pid: None,
                    owner_start_token: None,
                    mode: RouteMode::Alias,
                    created_at_ms: now_ms(),
                },
                || {
                    calls += 1;
                    if calls == 2 {
                        Err(anyhow::anyhow!("listener changed after publish"))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err()
            .to_string();

        assert!(error.contains("listener changed"));
        assert_eq!(calls, 2);
        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].target_port, 3999);
    }

    #[test]
    fn routes_are_lowercased_on_write() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "Web.LocalHost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let routes = store.read_routes(false).unwrap();
        let text = fs::read_to_string(store.routes_path()).unwrap();

        assert_eq!(routes[0].hostname, "web.localhost");
        assert!(text.contains(r#""hostname": "web.localhost""#));
    }

    #[test]
    fn concurrent_add_route_keeps_all_routes() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let handles = (0..16)
            .map(|index| {
                let store = store.clone();
                thread::spawn(move || {
                    store
                        .add_alias_route(Route {
                            hostname: format!("app-{index}.localhost").into(),
                            target_host: "127.0.0.1".into(),
                            target_port: 4000 + index,
                            owner_pid: None,
                            owner_start_token: None,
                            mode: RouteMode::Alias,
                            created_at_ms: now_ms(),
                        })
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 16);
        for index in 0..16 {
            assert!(
                routes
                    .iter()
                    .any(|route| route.hostname == format!("app-{index}.localhost"))
            );
        }
    }

    #[test]
    fn remove_route_matches_case_insensitively() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();

        store.remove_route("Web.LocalHost").unwrap();

        assert!(store.read_routes(false).unwrap().is_empty());
    }

    #[test]
    fn legacy_route_arrays_remain_readable() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(
            &store,
            serde_json::to_string(&vec![Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            }])
            .unwrap(),
        );

        let routes = store.read_routes(false).unwrap();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "web.localhost");
    }

    #[test]
    fn invalid_route_file_returns_error() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(&store, "{not json");

        let error = store.read_routes(false).unwrap_err().to_string();
        assert!(error.contains("Failed to parse Jig proxy routes"));
    }

    #[test]
    fn oversized_route_file_returns_error() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(&store, vec![b' '; (MAX_ROUTES_FILE_BYTES + 1) as usize]);

        let error = store.read_routes(false).unwrap_err().to_string();

        assert!(error.contains("above the"));
    }

    #[test]
    fn invalid_route_entries_return_error() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(
            &store,
            r#"{"version":1,"routes":[{"hostname":"bad,host","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1}]}"#,
        );

        let error = format!("{:#}", store.read_routes(false).unwrap_err());
        assert!(error.contains("Failed to parse Jig proxy routes"));
    }

    #[test]
    fn process_route_reads_require_owner_identity() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(
            &store,
            r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"owner_start_token":null,"mode":"process","created_at_ms":1}]}"#,
        );

        let error = store.read_routes(false).unwrap_err().to_string();

        assert!(error.contains("owner PID and start token"));
    }

    #[test]
    fn route_files_ignore_unknown_top_level_fields() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(
            &store,
            r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1}],"unexpected":true}"#,
        );

        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "web.localhost");
    }

    #[test]
    fn route_files_ignore_unknown_route_fields() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        write_private_routes_fixture(
            &store,
            r#"{"version":1,"routes":[{"hostname":"web.localhost","target_host":"127.0.0.1","target_port":4000,"owner_pid":null,"mode":"alias","created_at_ms":1,"unexpected":true}]}"#,
        );

        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "web.localhost");
    }

    #[test]
    fn reading_missing_routes_file_does_not_create_it() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let routes = store.read_routes(false).unwrap();

        assert!(routes.is_empty());
        assert!(!store.routes_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn route_reads_reject_symlink_file() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let route_file = temp.path().join("routes.json");
        let outside = temp.path().join("outside-routes.json");
        fs::write(&outside, "[]").unwrap();
        symlink(&outside, &route_file).unwrap();

        assert!(read_routes_from_path(&route_file).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn route_reads_reject_loose_permissions() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        fs::write(store.routes_path(), "[]").unwrap();
        fs::set_permissions(store.routes_path(), fs::Permissions::from_mode(0o644)).unwrap();

        let error = store.read_routes(false).unwrap_err().to_string();

        assert!(error.contains("must have mode 600"));
    }

    #[cfg(unix)]
    #[test]
    fn private_state_reads_reject_symlink_file() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target = temp.path().join("outside-token");
        let link = temp.path().join("proxy-health-token");
        fs::write(
            &target,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap();
        symlink(&target, &link).unwrap();

        assert!(read_health_token_file(&link).is_err());
        assert_eq!(read_port_file(&link), None);
    }

    #[test]
    fn invalid_pid_file_is_reported() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        fs::write(store.pid_path(), "not-a-pid").unwrap();

        let error = store.read_pid().unwrap_err().to_string();

        assert!(error.contains("Invalid Jig proxy PID file"));
    }

    #[test]
    fn health_token_is_private_and_reused_until_runtime_clear() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let first = store.ensure_health_token().unwrap();
        let second = store.ensure_health_token().unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(store.read_health_token().unwrap(), Some(first));
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(store.health_token_path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        store.clear_runtime_files();

        assert_eq!(store.read_health_token().unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn health_token_reads_reject_loose_permissions() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        fs::write(store.health_token_path(), token).unwrap();
        fs::set_permissions(store.health_token_path(), fs::Permissions::from_mode(0o644)).unwrap();

        let error = store.read_health_token().unwrap_err().to_string();

        assert!(error.contains("must have mode 600"));
    }

    #[test]
    fn replace_runtime_files_rewrites_state_under_one_runtime_lock() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        fs::write(store.https_port_path(), "1443").unwrap();
        let token = store
            .replace_runtime_files(Path::new("/tmp/jig"), 1355, None)
            .unwrap();

        assert_eq!(store.read_health_token().unwrap(), Some(token));
        assert_eq!(store.read_pid().unwrap(), Some(std::process::id()));
        assert_eq!(
            fs::read_to_string(store.proxy_exe_path()).unwrap(),
            "/tmp/jig"
        );
        assert_eq!(store.read_http_port().unwrap(), Some(1355));
        assert_eq!(store.read_https_port().unwrap(), None);
    }

    #[test]
    fn windows_tasklist_csv_pid_reads_second_field_only() {
        assert_eq!(
            windows_tasklist_csv_pid(r#""jig.exe","1234","Console","1","10,000 K""#),
            Some(1234)
        );
        assert_eq!(
            windows_tasklist_csv_pid(r#""bad "",""1234"", suffix","9999","Console","1","1 K""#),
            Some(9999)
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_is_owner_only() {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");

        let store = StateStore::resolve(Some(state_dir)).unwrap();
        let mode = fs::metadata(store.root()).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn existing_explicit_state_dir_must_already_be_private() {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("marker"), "not-empty").unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o755)).unwrap();

        let error = StateStore::resolve(Some(state_dir.clone()))
            .unwrap_err()
            .to_string();
        let mode = fs::metadata(&state_dir).unwrap().permissions().mode() & 0o777;

        assert!(error.contains("already exists with permissions"));
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn existing_explicit_state_dir_must_be_writable_and_searchable() {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(state_dir.join("marker"), "not-empty").unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o500)).unwrap();

        let error = StateStore::resolve(Some(state_dir.clone()))
            .unwrap_err()
            .to_string();

        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(error.contains("mode 700"));
    }

    #[cfg(unix)]
    #[test]
    fn missing_state_dir_rejects_shared_writable_creation_ancestor() {
        let temp = tempdir().unwrap();
        let parent = temp.path().join("shared");
        let state_dir = parent.join("state");
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();

        let error = StateStore::resolve(Some(state_dir))
            .unwrap_err()
            .to_string();

        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(error.contains("shared-writable ancestor"));
    }

    #[cfg(unix)]
    #[test]
    fn existing_default_state_parent_must_already_be_private() {
        let temp = tempdir().unwrap();
        let parent = temp.path().join(".jig");
        let state_dir = parent.join("proxy");
        fs::create_dir_all(&state_dir).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();

        let error = ensure_default_state_parent_permissions(&state_dir, true)
            .unwrap_err()
            .to_string();
        let mode = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;

        assert!(error.contains("Default proxy state parent"));
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_rejects_symlinked_entries() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(temp.path().join("target"), state_dir.join("proxy.pid")).unwrap();

        let error = StateStore::resolve(Some(state_dir))
            .unwrap_err()
            .to_string();

        assert!(error.contains("contains symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_rejects_nested_symlinked_entries() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        let nested = state_dir.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(temp.path().join("target"), nested.join("proxy.pid")).unwrap();

        let error = StateStore::resolve(Some(state_dir))
            .unwrap_err()
            .to_string();

        assert!(error.contains("contains symlink"));
    }

    #[test]
    fn read_proxy_exe_reports_missing_path() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .write_proxy_exe(&temp.path().join("missing-jig"))
            .unwrap();

        let status = store.read_proxy_exe_status().unwrap();
        assert_eq!(status.path, None);
        assert!(
            status
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("not available"))
        );
    }

    #[test]
    fn resolve_recovers_interrupted_replace_backup() {
        let temp = tempdir().unwrap();
        let state_dir = temp.path().join("state");
        fs::create_dir_all(&state_dir).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let backup = state_dir.join("routes.json.4294967295.123456.7.replace-backup");
        fs::write(&backup, "[]").unwrap();

        let store = StateStore::resolve(Some(state_dir)).unwrap();

        assert!(!backup.exists());
        assert_eq!(fs::read_to_string(store.routes_path()).unwrap(), "[]");
    }

    #[test]
    fn replace_backup_detection_matches_state_file_name() {
        let temp = tempdir().unwrap();
        let routes = temp.path().join("routes.json");
        let ports = temp.path().join("proxy-port");
        fs::write(
            temp.path().join("routes.json.42.123456.7.replace-backup"),
            "[]",
        )
        .unwrap();
        fs::write(
            temp.path().join("routes.json.not-a-pid.replace-backup"),
            "[]",
        )
        .unwrap();

        assert!(file_ops::replace_backup_for_path_exists(&routes));
        assert!(!file_ops::replace_backup_for_path_exists(&ports));
        assert_eq!(
            file_ops::replace_backup_parts("routes.json.42.123456.7.replace-backup"),
            Some(("routes.json", "42"))
        );
        assert_eq!(
            file_ops::replace_backup_parts("routes.json.not-a-pid.replace-backup"),
            None
        );
    }

    #[test]
    fn missing_route_file_with_replace_backup_fails_closed() {
        let temp = tempdir().unwrap();
        let routes = temp.path().join("routes.json");
        fs::write(
            temp.path().join("routes.json.42.123456.7.replace-backup"),
            "[]",
        )
        .unwrap();

        let error = missing_file_read_result(&routes, true).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("temporarily unavailable"));
    }

    #[test]
    fn backup_promotion_requires_start_token_support() {
        if process_start_tokens_supported() {
            return;
        }
        let temp = tempdir().unwrap();
        let backup = temp.path().join("routes.json.4294967295.replace-backup");
        fs::write(&backup, "[]").unwrap();

        assert!(!replace_backup_can_be_promoted(&backup, "4294967295"));
    }

    #[test]
    fn cert_lock_inside_route_lock_returns_error() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = store
            .with_route_lock(|_| store.with_cert_lock(|| Ok(())))
            .unwrap_err()
            .to_string();

        assert!(error.contains("cert lock cannot be acquired"));
        assert!(
            !store.root().join(CERT_LOCK_FILE).exists(),
            "route-held cert-lock attempts must fail before opening the cert lock"
        );
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_rejects_symlink_root() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        let link = temp.path().join("state-link");
        symlink(&target, &link).unwrap();

        let error = StateStore::resolve(Some(link)).unwrap_err().to_string();

        assert!(error.contains("must not be a symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn state_dir_canonicalizes_symlink_ancestor() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        let link = temp.path().join("state-parent-link");
        symlink(&target, &link).unwrap();

        let store = StateStore::resolve(Some(link.join("state"))).unwrap();

        assert!(store.root().starts_with(fs::canonicalize(target).unwrap()));
    }

    #[test]
    fn file_signature_changes_for_same_size_rewrites() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("routes.json");
        fs::write(&path, "aa").unwrap();
        let first = file_signature(&path).unwrap();

        fs::write(&path, "bb").unwrap();

        assert_ne!(file_signature(&path).unwrap(), first);
    }

    #[test]
    fn prune_skips_rewrite_when_routes_are_unchanged() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap();
        let before = store.routes_signature();

        store.prune().unwrap();

        assert_eq!(store.routes_signature(), before);
    }

    #[test]
    fn process_route_with_mismatched_start_token_is_dead() {
        let route = Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: Some("not-this-process".into()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };

        assert!(!route_is_alive(&route));
    }

    #[test]
    fn process_route_without_start_token_is_dead() {
        let route = Route {
            hostname: "web.localhost".into(),
            target_host: "127.0.0.1".into(),
            target_port: 4000,
            owner_pid: Some(std::process::id()),
            owner_start_token: None,
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };

        assert!(!route_is_alive(&route));
    }

    #[test]
    fn process_routes_are_rejected_without_start_token_support() {
        if process_start_tokens_supported() {
            return;
        }
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: Some(std::process::id()),
                owner_start_token: None,
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("Process routes require process start-token verification"));
    }

    #[test]
    fn add_route_refuses_to_replace_live_process_route() {
        if !process_start_tokens_supported() {
            return;
        }
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();
        store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: Some(std::process::id()),
                owner_start_token: process_start_token(std::process::id()),
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let error = store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4001,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("would replace a live process route"));
    }

    #[test]
    fn add_alias_route_requires_alias_mode() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = store
            .add_alias_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires RouteMode::Alias"));
    }

    #[test]
    fn add_alias_route_rejects_public_suffix_hostname() {
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = store
            .add_alias_route(Route {
                hostname: crate::host::RouteHostname::unchecked("api.example.com"),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: None,
                owner_start_token: None,
                mode: RouteMode::Alias,
                created_at_ms: now_ms(),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("private/local suffix"));
    }

    #[test]
    fn add_process_route_requires_owner_identity() {
        if !process_start_tokens_supported() {
            return;
        }
        let temp = tempdir().unwrap();
        let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

        let error = store
            .add_route(Route {
                hostname: "web.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: Some(std::process::id()),
                owner_start_token: None,
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("owner PID and start token"));
    }
}
