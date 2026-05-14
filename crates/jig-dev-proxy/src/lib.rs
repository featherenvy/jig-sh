//! Internal local-development proxy runtime for the `jig-sh` CLI.
//!
//! This crate is published only so the CLI can split and test the proxy
//! runtime independently. Its `anyhow::Result<serde_json::Value>` API is not a
//! stable third-party integration surface outside the matching `jig-sh`
//! release. See `crates/jig-dev-proxy/AGENTS.md` for the security and platform
//! invariants that maintainers should preserve when editing this crate.

mod certs;
mod file_ops;
mod host;
mod ports;
mod processes;
mod server;
mod service;
mod state;
mod types;
mod workspace;

use std::collections::HashMap;
use std::net::IpAddr;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::host::{RouteHostname, TargetHost};
use crate::ports::{is_tcp_listening, jig_proxy_http_pid, local_lan_ip_for_ipv4_listener};
use crate::state::{StateStore, now_ms, pid_is_alive};
use crate::types::{Route, RouteMode};

pub use crate::types::{
    AppKind, AppRunSpec, CommandSpec, DevRequest, ProxyAliasRequest, ProxyCertRequest,
    ProxyListRequest, ProxyPruneRequest, ProxyRunRequest, ProxyServiceRequest, ProxySettings,
    ProxyStartRequest, ProxyStopRequest,
};

pub fn dev(request: DevRequest) -> Result<Value> {
    let mut specs = request.apps;
    if request.discover_workspace {
        specs.extend(workspace::discover(
            &request.root,
            &request.repo_name,
            &request.settings.tld,
            &request.package_manager,
        )?);
    }
    if !request.selected_apps.is_empty() {
        let available_apps: Vec<_> = specs.iter().map(|spec| spec.name.clone()).collect();
        specs.retain(|spec| request.selected_apps.iter().any(|name| name == &spec.name));
        if specs.is_empty() {
            bail!(
                "No development apps matched --app filter '{}'. Available apps: {}",
                request.selected_apps.join(", "),
                if available_apps.is_empty() {
                    "<none>".into()
                } else {
                    available_apps.join(", ")
                }
            );
        }
    }
    if request.no_proxy {
        for spec in &mut specs {
            spec.proxy = false;
        }
    }
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    ensure_unique_specs(&specs)?;
    let current_exe = current_exe()?;
    processes::run_apps(specs, &request.settings, &current_exe)
}

pub fn proxy_start(request: ProxyStartRequest) -> Result<Value> {
    let current_exe = current_exe()?;
    if request.foreground {
        // Foreground mode owns the process until shutdown. This return value is
        // only reached if the listener loop exits cleanly.
        server::run_foreground(request.settings, current_exe)?;
        return Ok(json!({ "ok": true, "foreground": true }));
    }
    processes::ensure_proxy_running(&request.settings, &current_exe)?;
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    Ok(json!({
        "ok": true,
        "foreground": false,
        "http_port": store.read_http_port()?,
        "https_port": store.read_https_port()?,
        "bind_host": if request.settings.lan { "0.0.0.0" } else { "127.0.0.1" },
        "lan_host": if request.settings.lan { local_lan_ip_for_ipv4_listener().map(|ip| ip.to_string()) } else { None },
        "state_dir": store.root(),
    }))
}

pub fn proxy_stop(request: ProxyStopRequest) -> Result<Value> {
    if let Some(path) = missing_state_dir(&request.settings)? {
        return Ok(json!({
            "ok": true,
            "stopped": false,
            "runtime_files_cleared": false,
            "pid": null,
            "pid_alive": false,
            "probed_http_port": null,
            "health_pid": null,
            "handshake_ok": false,
            "pid_matches_proxy": false,
            "warning": null,
            "state_dir": path,
        }));
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let pid = store.read_pid()?;
    let pid_alive = pid.is_some_and(pid_is_alive);
    let probed_http_port = store.read_http_port()?;
    let health_token = store.read_health_token()?;
    let health_pid = probed_http_port
        .and_then(|port| jig_proxy_http_pid("127.0.0.1", port, health_token.as_deref()));
    let handshake_ok = health_pid.is_some();
    let pid_matches_proxy = pid
        .zip(health_pid)
        .is_some_and(|(pid, health_pid)| pid == health_pid);
    let mut stopped = false;
    if let Some(pid) = pid {
        if pid_alive && pid_matches_proxy {
            stopped = terminate_proxy_pid(pid);
        }
    }
    let should_clear_runtime_files = stopped || pid.is_none() || !pid_alive;
    let runtime_files_cleared = if should_clear_runtime_files {
        match store.try_clear_runtime_files() {
            Ok(()) => true,
            Err(error) => {
                eprintln!(
                    "jig proxy could not clear runtime files in {}: {error}",
                    store.root().display()
                );
                false
            }
        }
    } else {
        false
    };
    // ok=false is used for identity-preserving stop refusals: callers should
    // treat the JSON warning as the actionable result instead of assuming a
    // proxy process was stopped.
    let ok = runtime_files_cleared;
    Ok(json!({
        "ok": ok,
        "stopped": stopped,
        "runtime_files_cleared": runtime_files_cleared,
        "pid": pid,
        "pid_alive": pid_alive,
        "probed_http_port": probed_http_port,
        "health_pid": health_pid,
        "handshake_ok": handshake_ok,
        "pid_matches_proxy": pid_matches_proxy,
        "warning": stop_warning(pid, health_pid, pid_alive, handshake_ok, pid_matches_proxy, stopped),
        "state_dir": store.root(),
    }))
}

fn stop_warning(
    pid: Option<u32>,
    health_pid: Option<u32>,
    pid_alive: bool,
    handshake_ok: bool,
    pid_matches_proxy: bool,
    stopped: bool,
) -> Option<String> {
    if stopped {
        return None;
    }
    let pid = pid?;
    if pid_alive && !handshake_ok {
        return Some(format!(
            "PID file points at process {}, but it did not answer the Jig proxy health check. Runtime files were kept to avoid hiding or terminating an unrelated process; inspect the PID or remove the state dir after confirming it is stale.",
            pid
        ));
    }
    if pid_alive && handshake_ok && !pid_matches_proxy {
        return Some(format!(
            "A Jig proxy answered on the stored port as PID {}, but the PID file points at {}. Runtime files were kept to avoid terminating an unrelated process; use the matching JIG_PROXY_STATE_DIR or stop the other proxy explicitly.",
            health_pid.unwrap_or_default(),
            pid
        ));
    }
    if pid_alive && handshake_ok {
        return Some(format!(
            "Jig proxy process {} answered the health check but did not exit after stop signals. Runtime files were kept because the process may still own its ports.",
            pid
        ));
    }
    if !pid_alive {
        return Some(format!(
            "Stale Jig proxy PID file for process {} was found and cleared.",
            pid
        ));
    }
    None
}

#[cfg(unix)]
fn terminate_proxy_pid(pid: u32) -> bool {
    let Some(unix_pid) = unix_pid(pid) else {
        return false;
    };
    signal_proxy_pid(unix_pid, libc::SIGTERM);
    if wait_for_pid_exit(pid, Duration::from_secs(2)) {
        return true;
    }
    signal_proxy_pid(unix_pid, libc::SIGKILL);
    wait_for_pid_exit(pid, Duration::from_secs(1))
}

#[cfg(unix)]
fn signal_proxy_pid(pid: i32, signal: i32) {
    unsafe {
        // SAFETY: pid was range-checked by unix_pid and signal is one of the
        // libc constants used for process termination.
        let _ = libc::kill(pid, signal);
    }
}

#[cfg(unix)]
fn unix_pid(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok()
}

#[cfg(not(any(unix, windows)))]
fn terminate_proxy_pid(_pid: u32) -> bool {
    false
}

#[cfg(windows)]
fn terminate_proxy_pid(pid: u32) -> bool {
    let Ok(status) = std::process::Command::new(windows_system32_tool("taskkill.exe"))
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
    else {
        return false;
    };
    if status.success() && wait_for_pid_exit(pid, Duration::from_secs(2)) {
        return true;
    }
    let Ok(status) = std::process::Command::new(windows_system32_tool("taskkill.exe"))
        .env_clear()
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
    else {
        return false;
    };
    status.success() && wait_for_pid_exit(pid, Duration::from_secs(1))
}

#[cfg(windows)]
fn windows_system32_tool(name: &str) -> std::path::PathBuf {
    // Use the canonical system directory instead of a mutable environment
    // variable so cleanup keeps using the OS taskkill binary.
    std::path::PathBuf::from(r"C:\Windows\System32").join(name)
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !pid_is_alive(pid)
}

pub fn proxy_list(request: ProxyListRequest) -> Result<Value> {
    if let Some(path) = missing_state_dir(&request.settings)? {
        return Ok(json!({
            "ok": true,
            "state_dir": path,
            "http_port": null,
            "https_port": null,
            "proxy_exe": null,
            "proxy_exe_note": null,
            "proxy_exe_warning": null,
            "raw": request.raw,
            "routes": [],
        }));
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let routes = store.read_routes(!request.raw)?;
    let proxy_exe = store.read_proxy_exe_status()?;
    let proxy_exe_note = if proxy_exe.path.is_some() || proxy_exe.warning.is_some() {
        Some(proxy_exe_note())
    } else {
        None
    };
    Ok(json!({
        "ok": true,
        "state_dir": store.root(),
        "http_port": store.read_http_port()?,
        "https_port": store.read_https_port()?,
        "proxy_exe": proxy_exe.path,
        "proxy_exe_note": proxy_exe_note,
        "proxy_exe_warning": proxy_exe.warning,
        "raw": request.raw,
        "routes": routes,
    }))
}

fn proxy_exe_note() -> &'static str {
    "If this path came from JIG_DEV_BIN, restart the long-running proxy after rebuilding or replacing that binary."
}

pub fn proxy_prune(request: ProxyPruneRequest) -> Result<Value> {
    if missing_state_dir(&request.settings)?.is_some() {
        return Ok(json!({ "ok": true, "routes": [] }));
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let routes = store.prune()?;
    Ok(json!({ "ok": true, "routes": routes }))
}

pub fn proxy_run_foreground(request: ProxyRunRequest) -> Result<Value> {
    let current_exe = current_exe()?;
    processes::run_app(request.spec, &request.settings, &current_exe)
}

pub fn proxy_alias(request: ProxyAliasRequest) -> Result<Value> {
    if request.target_port == 0 {
        bail!("Proxy alias target port must be greater than 0");
    }
    let target_ip = validate_alias_target_host(&request.target_host)?;
    if request.settings.lan && !host::ip_is_loopback(target_ip) {
        bail!(
            "LAN proxy aliases may only target loopback IP literals. Refusing to expose '{}' through the LAN listener.",
            request.target_host
        );
    }
    if !host::ip_is_loopback(target_ip) && !request.accept_non_loopback_target {
        bail!(
            "Proxy alias target host '{}' is not loopback. Pass --accept-non-loopback-target to acknowledge that local browser requests can be proxied to that address.",
            request.target_host
        );
    }
    let store = StateStore::resolve(request.settings.state_dir.clone())?;
    let hostname = host::route_hostname(&request.name, &request.repo_name, &request.settings.tld)?;
    let https_active = proxy_https_listener_is_current_jig_proxy(&store)?;
    if https_active {
        // This is a freshness hint, not an atomic claim about the listener.
        // Route publication is independent, and a restarted proxy will pick up
        // cert state through the normal cache invalidation path.
        certs::ensure_for_hosts(&request.settings, std::slice::from_ref(&hostname))?;
    }
    store.add_alias_route(Route {
        hostname: RouteHostname::new(&hostname)?,
        target_host: TargetHost::ip_literal(&request.target_host)?,
        target_port: request.target_port,
        owner_pid: None,
        owner_start_token: None,
        mode: RouteMode::Alias,
        created_at_ms: now_ms(),
    })?;
    Ok(json!({
        "ok": true,
        "hostname": hostname,
        "target_port": request.target_port,
        "state_dir": store.root(),
    }))
}

fn proxy_https_listener_is_current_jig_proxy(store: &StateStore) -> Result<bool> {
    let Some(https_port) = store.read_https_port()? else {
        return Ok(false);
    };
    if !is_tcp_listening("127.0.0.1", https_port) {
        return Ok(false);
    }
    let Some(http_port) = store.read_http_port()? else {
        return Ok(false);
    };
    let Some(health_token) = store.read_health_token()? else {
        return Ok(false);
    };
    let Some(health_pid) = jig_proxy_http_pid("127.0.0.1", http_port, Some(&health_token)) else {
        return Ok(false);
    };
    Ok(store.read_pid()? == Some(health_pid))
}

pub fn proxy_cert(request: ProxyCertRequest) -> Result<Value> {
    match request {
        ProxyCertRequest::Generate { settings, force } => certs::generate(&settings, force),
        ProxyCertRequest::Status { settings } => certs::status(&settings),
        ProxyCertRequest::Trust {
            settings,
            accept_trust_scope,
        } => certs::trust(&settings, accept_trust_scope),
        ProxyCertRequest::Untrust {
            settings,
            accept_trust_scope,
        } => certs::untrust(&settings, accept_trust_scope),
    }
}

pub fn proxy_service(request: ProxyServiceRequest) -> Result<Value> {
    match request {
        ProxyServiceRequest::Install {
            settings,
            current_exe,
            repo_root,
            accept_service_scope,
        } => service::install(&settings, current_exe, repo_root, accept_service_scope),
        ProxyServiceRequest::Uninstall { settings } => service::uninstall(&settings),
        ProxyServiceRequest::Status { settings } => service::status(&settings),
    }
}

pub fn current_exe() -> Result<std::path::PathBuf> {
    let path = std::env::current_exe()?;
    path.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize current executable {}",
            path.display()
        )
    })
}

pub fn resolve_state_dir(explicit: Option<std::path::PathBuf>) -> Result<std::path::PathBuf> {
    if let Some(path) = explicit {
        Ok(path)
    } else if let Ok(path) = std::env::var("JIG_PROXY_STATE_DIR") {
        Ok(std::path::PathBuf::from(path))
    } else {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory for Jig proxy state")?
            .join(".jig/proxy"))
    }
}

fn missing_state_dir(settings: &ProxySettings) -> Result<Option<std::path::PathBuf>> {
    let path = resolve_state_dir(settings.state_dir.clone())?;
    Ok((!path.exists()).then_some(path))
}

pub fn app_hostname(name: &str, repo_name: &str, tld: &str) -> Result<String> {
    host::route_hostname(name, repo_name, tld)
}

pub fn dns_label(value: &str) -> Result<String> {
    host::sanitize_label(value)
}

pub fn validate_tld(tld: &str) -> Result<()> {
    host::validate_tld(tld)
}

pub fn ip_is_loopback(ip: IpAddr) -> bool {
    host::ip_is_loopback(ip)
}

fn ensure_unique_specs(specs: &[AppRunSpec]) -> Result<()> {
    let mut names = HashMap::new();
    let mut hostnames = HashMap::new();
    for spec in specs {
        if let Some(previous_dir) = names.insert(spec.name.as_str(), spec.dir.as_path()) {
            bail!(
                "Duplicate development app name '{}' in {} and {}",
                spec.name,
                previous_dir.display(),
                spec.dir.display()
            );
        }
        if let Some(previous_dir) = hostnames.insert(spec.hostname.as_str(), spec.dir.as_path()) {
            bail!(
                "Duplicate development app hostname '{}' in {} and {}",
                spec.hostname,
                previous_dir.display(),
                spec.dir.display()
            );
        }
    }
    Ok(())
}

fn validate_alias_target_host(host: &str) -> Result<IpAddr> {
    parse_ip_literal(host)
        .with_context(|| format!("Proxy alias target host '{host}' must be an IP literal"))
}

pub fn parse_ip_literal(host: &str) -> Result<IpAddr> {
    host.parse::<IpAddr>()
        .with_context(|| format!("target host '{host}' must be an IP literal"))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn proxy_stop_keeps_runtime_files_for_live_unverified_pid() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        store.write_pid(std::process::id()).unwrap();
        store.write_http_port(9).unwrap();

        let output = proxy_stop(ProxyStopRequest { settings }).unwrap();

        assert_eq!(output["ok"].as_bool(), Some(false));
        assert_eq!(output["stopped"].as_bool(), Some(false));
        assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
        assert!(
            output["warning"]
                .as_str()
                .unwrap()
                .contains("did not answer")
        );
        assert!(store.pid_path().exists());
        assert!(store.http_port_path().exists());
    }

    #[test]
    fn proxy_stop_does_not_kill_when_health_pid_differs() {
        let temp = tempdir().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 512];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: 1\r\ncontent-length: 11\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
        });

        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        store.write_pid(std::process::id()).unwrap();
        store.write_http_port(port).unwrap();

        let output = proxy_stop(ProxyStopRequest { settings }).unwrap();
        handle.join().unwrap();

        assert_eq!(output["ok"].as_bool(), Some(false));
        assert_eq!(output["stopped"].as_bool(), Some(false));
        assert_eq!(output["handshake_ok"].as_bool(), Some(true));
        assert_eq!(output["pid_matches_proxy"].as_bool(), Some(false));
        assert_eq!(output["runtime_files_cleared"].as_bool(), Some(false));
        assert!(
            output["warning"]
                .as_str()
                .unwrap()
                .contains("PID file points")
        );
        assert!(store.pid_path().exists());
        assert!(store.http_port_path().exists());
    }

    #[test]
    fn proxy_alias_rejects_zero_port() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "127.0.0.1".into(),
            target_port: 0,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("must be greater than 0"));
    }

    #[test]
    fn proxy_alias_rejects_invalid_target_host() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "bad host".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("must be an IP literal"));
    }

    #[test]
    fn proxy_alias_rejects_hostname_target_host() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "example.com".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("must be an IP literal"));
    }

    #[test]
    fn proxy_alias_lan_rejects_non_loopback_target_host() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            lan: true,
            ..ProxySettings::default()
        };

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "10.0.0.5".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("loopback"));
    }

    #[test]
    fn proxy_alias_requires_ack_for_non_loopback_target_host() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "10.0.0.5".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("--accept-non-loopback-target"));
    }

    #[test]
    fn proxy_alias_allows_acknowledged_non_loopback_target_host() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };

        let output = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "10.0.0.5".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: true,
            settings,
        })
        .unwrap();

        assert_eq!(output["ok"].as_bool(), Some(true));
    }

    #[test]
    fn proxy_alias_rejects_live_process_route_replacement() {
        if !crate::state::process_start_tokens_supported() {
            return;
        }
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        store
            .add_route(Route {
                hostname: "api.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                target_port: 4000,
                owner_pid: Some(std::process::id()),
                owner_start_token: crate::state::process_start_token(std::process::id()),
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            })
            .unwrap();

        let error = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "127.0.0.1".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("would replace a live process route"));
    }

    #[cfg(unix)]
    #[test]
    fn proxy_alias_registers_route_and_refreshes_https_certificate() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };
        let https_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let health_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let token = store.ensure_health_token().unwrap();
        store.write_pid(std::process::id()).unwrap();
        store
            .write_http_port(health_listener.local_addr().unwrap().port())
            .unwrap();
        store
            .write_https_port(https_listener.local_addr().unwrap().port())
            .unwrap();
        let health = thread::spawn(move || {
            let (mut stream, _) = health_listener.accept().unwrap();
            let mut request = [0u8; 512];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.contains(&format!("x-jig-proxy-health-token: {token}\r\n")));
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nx-jig-proxy: 1\r\nx-jig-proxy-pid: {}\r\ncontent-length: 0\r\n\r\n",
                std::process::id()
            )
            .unwrap();
        });

        let output = proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "127.0.0.1".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings: settings.clone(),
        })
        .unwrap();
        health.join().unwrap();

        assert_eq!(output["ok"].as_bool(), Some(true));
        assert_eq!(output["hostname"].as_str(), Some("api.demo.localhost"));
        let routes = store.read_routes(false).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].hostname, "api.demo.localhost");
        assert_eq!(routes[0].target_port, 5000);
        assert_eq!(routes[0].mode, RouteMode::Alias);
        assert!(store.leaf_path().exists());
        let leaf_hosts = std::fs::read_to_string(store.leaf_hosts_path()).unwrap();
        assert!(leaf_hosts.contains("api.demo.localhost"));
    }

    #[cfg(unix)]
    #[test]
    fn proxy_alias_defers_https_certificate_without_running_https_proxy() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().to_path_buf()),
            https: true,
            ..ProxySettings::default()
        };

        proxy_alias(ProxyAliasRequest {
            name: "api".into(),
            target_host: "127.0.0.1".into(),
            target_port: 5000,
            repo_name: "demo".into(),
            accept_non_loopback_target: false,
            settings: settings.clone(),
        })
        .unwrap();

        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        assert!(!store.leaf_path().exists());
    }

    #[test]
    fn proxy_stop_list_and_prune_noop_when_state_dir_is_missing() {
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing-state");
        let settings = ProxySettings {
            state_dir: Some(missing.clone()),
            ..ProxySettings::default()
        };

        let stop = proxy_stop(ProxyStopRequest {
            settings: settings.clone(),
        })
        .unwrap();
        let list = proxy_list(ProxyListRequest {
            settings: settings.clone(),
            raw: false,
        })
        .unwrap();
        let prune = proxy_prune(ProxyPruneRequest { settings }).unwrap();

        assert_eq!(stop["ok"].as_bool(), Some(true));
        assert_eq!(stop["stopped"].as_bool(), Some(false));
        assert!(list["routes"].as_array().unwrap().is_empty());
        assert!(prune["routes"].as_array().unwrap().is_empty());
        assert!(!missing.exists());
    }

    #[test]
    fn dev_reports_unknown_selected_app_names() {
        let temp = tempdir().unwrap();
        let error = dev(DevRequest {
            repo_name: "demo".into(),
            root: temp.path().to_path_buf(),
            package_manager: "npm".into(),
            settings: ProxySettings {
                state_dir: Some(temp.path().to_path_buf()),
                ..ProxySettings::default()
            },
            apps: vec![AppRunSpec {
                name: "web".into(),
                dir: temp.path().to_path_buf(),
                command: CommandSpec::Argv(vec!["unused".into()]),
                kind: AppKind::EnvPort,
                hostname: "web.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                explicit_port: None,
                proxy: false,
            }],
            selected_apps: vec!["api".into()],
            discover_workspace: false,
            no_proxy: false,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("No development apps matched"));
        assert!(error.contains("Available apps: web"));
    }

    #[test]
    fn dev_reports_empty_app_configuration_before_launch() {
        let temp = tempdir().unwrap();
        let error = dev(DevRequest {
            repo_name: "demo".into(),
            root: temp.path().to_path_buf(),
            package_manager: "npm".into(),
            settings: ProxySettings {
                state_dir: Some(temp.path().to_path_buf()),
                ..ProxySettings::default()
            },
            apps: Vec::new(),
            selected_apps: Vec::new(),
            discover_workspace: false,
            no_proxy: false,
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("No development apps were configured or discovered"));
    }

    #[test]
    fn duplicate_hostname_error_includes_source_dirs() {
        let temp = tempdir().unwrap();
        let web_dir = temp.path().join("web");
        let api_dir = temp.path().join("api");
        let specs = vec![
            AppRunSpec {
                name: "web".into(),
                dir: web_dir.clone(),
                command: CommandSpec::Argv(vec!["unused".into()]),
                kind: AppKind::EnvPort,
                hostname: "app.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                explicit_port: None,
                proxy: true,
            },
            AppRunSpec {
                name: "api".into(),
                dir: api_dir.clone(),
                command: CommandSpec::Argv(vec!["unused".into()]),
                kind: AppKind::EnvPort,
                hostname: "app.demo.localhost".into(),
                target_host: "127.0.0.1".into(),
                explicit_port: None,
                proxy: true,
            },
        ];

        let error = ensure_unique_specs(&specs).unwrap_err().to_string();

        assert!(error.contains("Duplicate development app hostname"));
        assert!(error.contains(&web_dir.display().to_string()));
        assert!(error.contains(&api_dir.display().to_string()));
    }
}
