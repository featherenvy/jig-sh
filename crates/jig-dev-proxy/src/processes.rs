use std::collections::HashSet;
#[cfg(test)]
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::certs;
use crate::host::{RouteHostname, TargetHost, target_host_is_loopback};
use crate::ports::{find_free_app_port_excluding, local_lan_ip_for_ipv4_listener, port_is_free};
use crate::state::{StateStore, now_ms, process_start_tokens_supported};
#[cfg(test)]
use crate::types::CommandSpec;
use crate::types::{AppKind, AppRunSpec, ProxySettings, Route, RouteMode};
mod cleanup;
mod frameworks;
mod listener_owner;
mod proxy;

use self::cleanup::*;
use self::frameworks::*;
use self::listener_owner::*;
pub(crate) use self::proxy::ensure_proxy_running;
#[cfg(test)]
use self::proxy::{MAX_PROXY_LOG_BYTES, ensure_requested_https, open_proxy_log};
use self::proxy::{proxy_health_failed, proxy_ready};

const PROXY_HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);
static CTRL_C_REQUESTED: AtomicBool = AtomicBool::new(false);
static CTRL_C_HANDLER: OnceLock<()> = OnceLock::new();

pub(crate) fn run_app(
    spec: AppRunSpec,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<Value> {
    start_ctrlc_cleanup_session();
    let route_parts = if spec.proxy {
        ensure_process_routes_supported()?;
        let route_parts = process_route_parts(settings, &spec)?;
        prepare_certs_for_hosts(settings, std::slice::from_ref(&spec.hostname))?;
        ensure_proxy_running(settings, current_exe)?;
        Some(route_parts)
    } else {
        None
    };
    let store = StateStore::resolve(settings.state_dir.clone())?;

    let port = choose_app_port(spec.explicit_port, &spec.target_host, &mut HashSet::new())?;
    let argv = command_argv(&spec.command, &spec.kind, port)?;
    if argv.is_empty() {
        bail!("No command configured for app '{}'", spec.name);
    }

    let mut child = spawn_child(&spec, &argv, port, settings)?;
    let pid = child.id();
    let owner_start_token = if spec.proxy {
        match wait_for_app_ready(&spec, port, &mut child) {
            Ok(token) => token,
            Err(error) => {
                terminate_child(&mut child);
                let _ = child.wait();
                return Err(error);
            }
        }
    } else {
        None
    };
    if spec.proxy {
        let Some(owner_start_token) = owner_start_token else {
            terminate_child(&mut child);
            let _ = child.wait();
            bail!(
                "Could not verify start identity for child process {pid}; refusing to publish process route"
            );
        };
        let Some((hostname, target_host)) = route_parts else {
            terminate_child(&mut child);
            let _ = child.wait();
            bail!(
                "Could not prepare process route for child process {pid}; refusing to publish route"
            );
        };
        let route = Route {
            hostname,
            target_host,
            target_port: port,
            owner_pid: Some(pid),
            owner_start_token: Some(owner_start_token.clone()),
            mode: RouteMode::Process,
            created_at_ms: now_ms(),
        };
        if let Err(error) = store.add_verified_route(route, || {
            verify_process_route_owner(
                &spec.name,
                &spec.target_host,
                port,
                pid,
                Some(&owner_start_token),
            )
        }) {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err(error);
        }
    }

    if let Err(error) = print_app_url(&spec, settings, port, &store) {
        terminate_child(&mut child);
        let _ = child.wait();
        if spec.proxy {
            let _ = store.remove_route(&spec.hostname);
        }
        return Err(error);
    }
    let status = loop {
        if ctrl_c_requested() {
            terminate_child(&mut child);
            let _ = child.wait();
            if spec.proxy {
                let _ = store.remove_route(&spec.hostname);
            }
            bail!("Interrupted");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                if spec.proxy {
                    let _ = store.remove_route(&spec.hostname);
                }
                terminate_child(&mut child);
                let _ = child.wait();
                return Err(error.into());
            }
        }
    };
    if spec.proxy {
        store.remove_route(&spec.hostname)?;
    }

    let exit_status = status.code().unwrap_or(1);
    Ok(json!({
        "ok": status.success(),
        "app": spec.name,
        "hostname": spec.hostname,
        "port": port,
        "exit_status": exit_status,
    }))
}

pub(crate) fn run_apps(
    specs: Vec<AppRunSpec>,
    settings: &ProxySettings,
    current_exe: &Path,
) -> Result<Value> {
    start_ctrlc_cleanup_session();
    if specs.is_empty() {
        bail!("No development apps were configured or discovered.");
    }
    validate_explicit_ports(&specs)?;
    let uses_proxy = specs.iter().any(|spec| spec.proxy);
    if uses_proxy {
        ensure_process_routes_supported()?;
        validate_process_routes(settings, &specs)?;
        let hostnames: Vec<String> = specs
            .iter()
            .filter(|spec| spec.proxy)
            .map(|spec| spec.hostname.clone())
            .collect();
        prepare_certs_for_hosts(settings, &hostnames)?;
        ensure_proxy_running(settings, current_exe)?;
    }
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let mut children = Vec::new();
    let mut routes = Vec::new();
    let mut assigned_ports = HashSet::new();

    for spec in specs {
        let route_parts = if spec.proxy {
            Some(process_route_parts(settings, &spec)?)
        } else {
            None
        };
        let port = match choose_app_port(spec.explicit_port, &spec.target_host, &mut assigned_ports)
        {
            Ok(port) => port,
            Err(error) => {
                cleanup_children(&mut children);
                return Err(error);
            }
        };
        let argv = match command_argv(&spec.command, &spec.kind, port) {
            Ok(argv) if !argv.is_empty() => argv,
            Ok(_) => {
                cleanup_children(&mut children);
                bail!("No command configured for app '{}'", spec.name);
            }
            Err(error) => {
                cleanup_children(&mut children);
                return Err(error);
            }
        };
        let mut child = match spawn_child(&spec, &argv, port, settings) {
            Ok(child) => child,
            Err(error) => {
                cleanup_children(&mut children);
                return Err(error);
            }
        };
        let child_pid = child.id();
        let owner_start_token = if spec.proxy {
            match wait_for_app_ready(&spec, port, &mut child) {
                Ok(token) => token,
                Err(error) => {
                    terminate_child(&mut child);
                    let _ = child.wait();
                    cleanup_children(&mut children);
                    return Err(error);
                }
            }
        } else {
            None
        };
        if spec.proxy && owner_start_token.is_none() {
            terminate_child(&mut child);
            let _ = child.wait();
            cleanup_children(&mut children);
            bail!(
                "Could not verify start identity for child process {child_pid}; refusing to publish process route"
            );
        }
        if spec.proxy {
            let Some((hostname, target_host)) = route_parts else {
                terminate_child(&mut child);
                let _ = child.wait();
                cleanup_children(&mut children);
                bail!(
                    "Could not prepare process route for child process {child_pid}; refusing to publish route"
                );
            };
            let route = Route {
                hostname,
                target_host,
                target_port: port,
                owner_pid: Some(child_pid),
                owner_start_token,
                mode: RouteMode::Process,
                created_at_ms: now_ms(),
            };
            if let Err(error) = store.add_verified_route(route.clone(), || {
                verify_process_route_owner(
                    &spec.name,
                    &spec.target_host,
                    port,
                    child_pid,
                    route.owner_start_token.as_deref(),
                )
            }) {
                terminate_child(&mut child);
                let _ = child.wait();
                cleanup_children(&mut children);
                return Err(error);
            }
            routes.push(route);
        }
        if let Err(error) = print_app_url(&spec, settings, port, &store) {
            terminate_child(&mut child);
            let _ = child.wait();
            if spec.proxy {
                let _ = store.remove_route(&spec.hostname);
            }
            cleanup_children(&mut children);
            return Err(error);
        }
        children.push(RunningChild {
            name: spec.name,
            hostname: spec.hostname,
            proxied: spec.proxy,
            store: store.clone(),
            child,
            cleanup_armed: true,
        });
    }

    let mut first_exit = None;
    let mut proxy_stopped = false;
    let mut interrupted = false;
    let mut proxy_health_misses = 0u8;
    let mut next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
    while first_exit.is_none() {
        if ctrl_c_requested() {
            first_exit = Some(("interrupt".to_string(), 130));
            interrupted = true;
            break;
        }
        for running in &mut children {
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    first_exit = Some((running.name.clone(), status.code().unwrap_or(1)));
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    cleanup_children(&mut children);
                    return Err(error.into());
                }
            }
        }
        if first_exit.is_none() && uses_proxy && Instant::now() >= next_proxy_health_check {
            next_proxy_health_check = Instant::now() + PROXY_HEALTH_CHECK_INTERVAL;
            if proxy_health_failed(&mut proxy_health_misses, proxy_ready(&store, settings)?) {
                first_exit = Some(("jig proxy".to_string(), 1));
                proxy_stopped = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if proxy_stopped {
        eprintln!("Jig proxy stopped responding; shutting down development session");
    } else if interrupted {
        eprintln!("Interrupted; stopping development session");
    } else if let Some((name, code)) = &first_exit {
        eprintln!("{name} exited with status {code}; stopping development session");
    }

    cleanup_children(&mut children);
    if interrupted {
        bail!("Interrupted");
    }

    Ok(json!({
        "ok": first_exit.as_ref().map(|(_, code)| *code == 0).unwrap_or(false),
        "first_exit": first_exit.map(|(name, code)| json!({ "app": name, "exit_status": code })),
        "proxy_failed": proxy_stopped,
        "routes": routes,
    }))
}

fn prepare_certs_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    certs::ensure_for_hosts(settings, hostnames)?;
    Ok(())
}

fn validate_explicit_ports(specs: &[AppRunSpec]) -> Result<()> {
    let mut explicit_ports = HashSet::new();
    for spec in specs {
        let Some(port) = spec.explicit_port else {
            continue;
        };
        if port == 0 {
            bail!("Explicit development app ports must be greater than 0");
        }
        if !explicit_ports.insert(port) {
            bail!("Multiple development apps requested port {port}");
        }
    }
    Ok(())
}

fn ensure_process_routes_supported() -> Result<()> {
    if process_start_tokens_supported() {
        return Ok(());
    }
    bail!(
        "Process routes require process start-token verification on this platform. Use `scripts/jig proxy alias` for an already-running app, or run with --no-proxy."
    )
}

fn validate_process_routes(settings: &ProxySettings, specs: &[AppRunSpec]) -> Result<()> {
    for spec in specs {
        if spec.proxy {
            process_route_parts(settings, spec)?;
        }
    }
    Ok(())
}

fn process_route_parts(
    settings: &ProxySettings,
    spec: &AppRunSpec,
) -> Result<(RouteHostname, TargetHost)> {
    let hostname = RouteHostname::new(&spec.hostname)?;
    let target_host = TargetHost::ip_literal(&spec.target_host).map_err(|_| {
        anyhow!(
            "Process route '{}' target host '{}' must be an IP literal",
            spec.name,
            spec.target_host
        )
    })?;
    if settings.lan && !target_host_is_loopback(&spec.target_host) {
        bail!(
            "LAN process route '{}' may only target loopback IP literals. Refusing to expose '{}' through the LAN listener.",
            spec.name,
            spec.target_host
        );
    }
    Ok((hostname, target_host))
}

fn spawn_child(
    spec: &AppRunSpec,
    argv: &[String],
    port: u16,
    settings: &ProxySettings,
) -> Result<Child> {
    // App commands are trusted repo-configured dev processes and intentionally
    // inherit the caller's environment; only the background proxy clears env.
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&spec.dir)
        .env("PORT", port.to_string())
        .env("HOST", &spec.target_host);
    configure_app_child_process_group(&mut command);
    if spec.kind == AppKind::Vite || command_looks_like_vite(argv) {
        // Vite validates the browser-facing Host header even though Jig binds
        // the app to loopback. Vite's internal allowed-hosts escape hatch keeps
        // routed dev hostnames working while still injecting --host 127.0.0.1;
        // keep this isolated because Vite can rename the variable.
        command.env(
            "__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS",
            vite_allowed_hosts(spec, settings)?,
        );
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            anyhow!(
                "Failed to run command '{}': executable was not found in PATH",
                argv[0]
            )
        } else {
            anyhow!("Failed to run command '{}': {error}", argv[0])
        }
    })
}

#[cfg(unix)]
fn configure_app_child_process_group(command: &mut Command) {
    unsafe {
        // SAFETY: pre_exec runs in the child after fork and before exec. The
        // closure only calls setsid and reads errno for its return value.
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

#[cfg(windows)]
fn configure_app_child_process_group(command: &mut Command) {
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_app_child_process_group(_command: &mut Command) {}

fn choose_app_port(
    explicit: Option<u16>,
    target_host: &str,
    assigned_ports: &mut HashSet<u16>,
) -> Result<u16> {
    let port = if let Some(port) = explicit {
        if port == 0 {
            bail!("Explicit development app ports must be greater than 0");
        }
        if assigned_ports.contains(&port) {
            bail!("Multiple development apps requested port {port}");
        }
        if !port_is_free(target_host, port)? {
            bail!("Requested development app port {port} is already in use on {target_host}");
        }
        port
    } else {
        find_free_app_port_excluding(target_host, assigned_ports)?
    };
    if !assigned_ports.insert(port) {
        bail!("Multiple development apps requested port {port}");
    }
    Ok(port)
}

fn print_app_url(
    spec: &AppRunSpec,
    settings: &ProxySettings,
    port: u16,
    store: &StateStore,
) -> Result<()> {
    if spec.proxy {
        let (scheme, proxy_port) = if settings.https {
            store
                .read_https_port()?
                .map(|port| ("https", port))
                .unwrap_or((
                    "http",
                    store.read_http_port()?.unwrap_or(settings.http_port),
                ))
        } else {
            (
                "http",
                store.read_http_port()?.unwrap_or(settings.http_port),
            )
        };
        eprintln!("{} -> {scheme}://{}:{proxy_port}", spec.name, spec.hostname);
        if settings.lan {
            if let Some(ip) = local_lan_ip_for_ipv4_listener() {
                eprintln!(
                    "{} LAN -> {scheme}://{}:{} with Host header {} or a local DNS/hosts entry",
                    spec.name, ip, proxy_port, spec.hostname
                );
            } else {
                eprintln!(
                    "{} LAN -> no non-loopback IPv4 LAN address detected for the IPv4 listener; configure DNS/hosts once an address is available",
                    spec.name
                );
            }
        }
        return Ok(());
    }
    eprintln!("{} -> http://{}:{port}", spec.name, spec.target_host);
    Ok(())
}

#[cfg(test)]
mod tests;
