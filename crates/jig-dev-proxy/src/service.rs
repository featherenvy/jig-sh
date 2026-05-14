use std::fs::{self, File};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::file_ops;
use crate::host::validate_tld;
use crate::state::StateStore;
use crate::types::ProxySettings;

pub(crate) fn install(
    settings: &ProxySettings,
    current_exe: PathBuf,
    repo_root: PathBuf,
    accept_service_scope: bool,
) -> Result<Value> {
    if !accept_service_scope {
        bail!(
            "Refusing to install the Jig proxy service without --accept-service-scope. This command writes and loads a per-user service for the local development proxy."
        );
    }
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let service_path = service_path()?;
    write_and_load_service(
        settings,
        &store,
        &current_exe,
        &repo_root,
        &service_path,
        load_service,
    )
}

fn write_and_load_service(
    settings: &ProxySettings,
    store: &StateStore,
    current_exe: &Path,
    repo_root: &Path,
    service_path: &Path,
    load: impl FnOnce(&Path) -> Value,
) -> Result<Value> {
    if let Some(parent) = service_path.parent() {
        ensure_service_parent_is_not_symlink(parent)?;
        fs::create_dir_all(parent)?;
        ensure_service_parent_is_not_symlink(parent)?;
    }
    let body = service_body(settings, store, current_exe, repo_root)?;
    let file_written = write_service_file_if_safe(service_path, &body)?;
    let load = load(service_path);
    let loaded = load["ok"].as_bool().unwrap_or(false);
    let file_present = service_path.exists();
    Ok(json!({
        "ok": loaded,
        "installed": file_present && loaded,
        "file_present": file_present,
        "file_written": file_written,
        "load": load,
        "path": service_path,
        "state_dir": store.root(),
        "repo_root": repo_root,
        "log_path": store.log_path(),
        "note": service_reload_hint(),
        "privileged_port_note": privileged_port_note(settings),
    }))
}

pub(crate) fn uninstall(settings: &ProxySettings) -> Result<Value> {
    let _ = settings;
    let path = service_path()?;
    unload_and_remove_service(&path, unload_service, reload_after_remove_service)
}

pub(crate) fn status(settings: &ProxySettings) -> Result<Value> {
    let store = StateStore::resolve(settings.state_dir.clone())?;
    let path = service_path()?;
    Ok(service_status_value(
        settings,
        &store,
        &path,
        service_manager_status,
    ))
}

fn service_status_value(
    settings: &ProxySettings,
    store: &StateStore,
    path: &Path,
    manager_status: impl FnOnce(&Path) -> Value,
) -> Value {
    let file_present = path.exists();
    let service = if file_present {
        manager_status(path)
    } else {
        json!({
            "ok": true,
            "skipped_no_file": true,
            "loaded": false,
            "enabled": false,
            "running": false,
        })
    };
    let loaded = service["loaded"].as_bool().unwrap_or(false);
    let enabled = service["enabled"].as_bool().unwrap_or(loaded);
    json!({
        "ok": service["ok"].as_bool().unwrap_or(false),
        "installed": file_present && loaded && enabled,
        "file_present": file_present,
        "path": path,
        "state_dir": store.root(),
        "platform": std::env::consts::OS,
        "service": service,
        "privileged_port_note": privileged_port_note(settings),
    })
}

fn service_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory")?
            .join("Library/LaunchAgents/sh.jig.proxy.plist"))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(dirs::home_dir()
            .context("Could not resolve home directory")?
            .join(".config/systemd/user/jig-proxy.service"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Jig proxy user services are not supported on this platform.");
}

fn service_body(
    settings: &ProxySettings,
    store: &StateStore,
    current_exe: &Path,
    repo_root: &Path,
) -> Result<String> {
    if settings.http_port == 0 {
        bail!("proxy HTTP port must be greater than 0 for service files");
    }
    if settings.https_port == Some(0) {
        bail!("proxy HTTPS port must be greater than 0 for service files");
    }
    if settings.https && settings.https_port == Some(settings.http_port) {
        bail!("proxy HTTP and HTTPS ports must be different for service files");
    }
    validate_tld(&settings.tld)?;
    let current_exe = service_path_text(current_exe, "current executable")?;
    let repo_root = service_path_text(repo_root, "repo root")?;
    let state_dir = service_path_text(store.root(), "proxy state dir")?;
    let log_path = service_path_text(&store.log_path(), "proxy log path")?;

    #[cfg(target_os = "macos")]
    {
        let mut args = vec![
            current_exe.clone(),
            "proxy".to_string(),
            "start".to_string(),
            "--foreground".to_string(),
            "--http-port".to_string(),
            settings.http_port.to_string(),
            "--tld".to_string(),
            settings.tld.clone(),
        ];
        if settings.https {
            args.push("--https".to_string());
            args.push("--https-port".to_string());
            args.push(settings.https_port.unwrap_or(1443).to_string());
        }
        if !settings.http2 {
            args.push("--no-http2".to_string());
        }
        if settings.lan {
            args.push("--lan".to_string());
        }
        let program_args = plist_string_array_entries(&args);
        Ok(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>{label}
  <key>ProgramArguments</key>
  <array>
{program_args}
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>JIG_PROXY_STATE_DIR</key>{state_dir}
    <key>JIG_REPO_ROOT</key>{repo_root}
  </dict>
  <key>WorkingDirectory</key>{repo_root}
  <key>StandardOutPath</key>{log_path}
  <key>StandardErrorPath</key>{log_path}
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
</dict>
</plist>
"#,
            label = plist_string("sh.jig.proxy"),
            program_args = program_args,
            state_dir = plist_string(&state_dir),
            repo_root = plist_string(&repo_root),
            log_path = plist_string(&log_path),
        ))
    }

    #[cfg(target_os = "linux")]
    {
        let exe = systemd_exec_quote(&current_exe)?;
        let tld = systemd_exec_quote(&settings.tld)?;
        let state_dir_env = systemd_quote(&format!("JIG_PROXY_STATE_DIR={state_dir}"))?;
        let repo_root_env = systemd_quote(&format!("JIG_REPO_ROOT={repo_root}"))?;
        let repo_root = systemd_quote(&repo_root)?;
        let log_output = systemd_quote(&format!("append:{log_path}"))?;
        Ok(format!(
            r#"[Unit]
Description=Jig local development proxy

[Service]
ExecStart={exe} proxy start --foreground --http-port {http_port} --tld {tld}{https_args}{http2_args}{lan_args}
Environment={state_dir_env}
Environment={repo_root_env}
WorkingDirectory={repo_root}
StandardOutput={log_output}
StandardError={log_output}
Restart=on-failure
RestartSec=5s

[Install]
WantedBy=default.target
"#,
            exe = exe,
            http_port = settings.http_port,
            tld = tld,
            https_args = if settings.https {
                format!(
                    " --https --https-port {}",
                    settings.https_port.unwrap_or(1443)
                )
            } else {
                String::new()
            },
            http2_args = if settings.http2 { "" } else { " --no-http2" },
            lan_args = if settings.lan { " --lan" } else { "" },
            state_dir_env = state_dir_env,
            repo_root_env = repo_root_env,
            repo_root = repo_root,
            log_output = log_output,
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    anyhow::bail!("Jig proxy service install is not supported on this platform.");
}

fn service_path_text(path: &Path, label: &str) -> Result<String> {
    if !path.is_absolute() {
        anyhow::bail!("{label} path must be absolute for service files");
    }
    let text = path.to_string_lossy().into_owned();
    if text.chars().any(|ch| ch.is_control()) {
        anyhow::bail!("{label} path cannot contain control characters for service files");
    }
    Ok(text)
}

#[cfg(any(target_os = "macos", test))]
fn plist_string(input: &str) -> String {
    format!("<string>{}</string>", xml_escape(input))
}

#[cfg(target_os = "macos")]
fn plist_string_array_entries(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("    {}", plist_string(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn service_reload_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Jig attempts to load the LaunchAgent with launchctl. If that fails, run launchctl bootstrap gui/$UID ~/Library/LaunchAgents/sh.jig.proxy.plist."
    }
    #[cfg(target_os = "linux")]
    {
        "Jig attempts systemctl --user daemon-reload and enable --now. If that fails, run systemctl --user daemon-reload && systemctl --user enable --now jig-proxy.service."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "Service management is unsupported on this platform."
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_status_json(program: &str, args: &[String]) -> Value {
    let mut command = match service_command(program) {
        Ok(command) => command,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    match command.args(args).status() {
        Ok(status) => json!({
            "ok": status.success(),
            "status": status.code(),
        }),
        Err(error) => json!({
            "ok": false,
            "error": error.to_string(),
        }),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn command_output_json(program: &str, args: &[String]) -> Value {
    let mut command = match service_command(program) {
        Ok(command) => command,
        Err(error) => {
            return json!({
                "ok": false,
                "error": error.to_string(),
            });
        }
    };
    match command.args(args).output() {
        Ok(output) => json!({
            "ok": output.status.success(),
            "status": output.status.code(),
            "stdout": String::from_utf8_lossy(&output.stdout).trim(),
            "stderr": String::from_utf8_lossy(&output.stderr).trim(),
        }),
        Err(error) => json!({
            "ok": false,
            "error": error.to_string(),
        }),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn service_command(program: &str) -> Result<Command> {
    let path = service_tool_path(program)?;
    let mut command = Command::new(path);
    command.env_clear();
    preserve_service_command_env(&mut command);
    Ok(command)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn preserve_service_command_env(command: &mut Command) {
    for key in [
        "HOME",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

#[cfg(target_os = "macos")]
fn service_tool_path(program: &str) -> Result<PathBuf> {
    match program {
        "launchctl" => Ok(PathBuf::from("/bin/launchctl")),
        other => anyhow::bail!("Unsupported Jig proxy service manager command: {other}"),
    }
}

#[cfg(target_os = "linux")]
fn service_tool_path(program: &str) -> Result<PathBuf> {
    match program {
        "systemctl" => fixed_system_tool_path("systemctl"),
        other => anyhow::bail!("Unsupported Jig proxy service manager command: {other}"),
    }
}

#[cfg(target_os = "linux")]
fn fixed_system_tool_path(program: &str) -> Result<PathBuf> {
    let candidates = fixed_system_tool_candidates(program);
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if executable_regular_file(&path) {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "Could not find {program} at fixed system tool paths: {}",
        candidates.join(", ")
    )
}

#[cfg(target_os = "linux")]
fn fixed_system_tool_candidates(program: &str) -> &'static [&'static str] {
    match program {
        "systemctl" => &["/usr/bin/systemctl", "/bin/systemctl"],
        _ => &[],
    }
}

#[cfg(target_os = "linux")]
fn executable_regular_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn command_spawned(output: &Value) -> bool {
    output.get("error").is_none()
}

#[cfg(target_os = "macos")]
fn launchctl_domain() -> Result<String> {
    let uid = unsafe {
        // SAFETY: geteuid takes no pointers and has no preconditions.
        libc::geteuid()
    };
    if uid == 0 {
        bail!("Jig proxy user services must be managed as the login user, not with sudo/root.");
    }
    Ok(format!("gui/{uid}"))
}

fn load_service(path: &Path) -> Value {
    #[cfg(target_os = "macos")]
    {
        let domain = match launchctl_domain() {
            Ok(domain) => domain,
            Err(error) => {
                return json!({
                    "ok": false,
                    "error": error.to_string(),
                });
            }
        };
        let bootout_args = vec![
            "bootout".to_string(),
            domain.clone(),
            path.to_string_lossy().into_owned(),
        ];
        let bootout = launchctl_bootout_json(&bootout_args);
        let bootstrap_args = vec![
            "bootstrap".to_string(),
            domain,
            path.to_string_lossy().into_owned(),
        ];
        let bootstrap = command_status_json("launchctl", &bootstrap_args);
        json!({
            "ok": bootstrap["ok"].as_bool().unwrap_or(false),
            "bootout": bootout,
            "bootstrap": bootstrap,
        })
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        let reload_args = vec!["--user".to_string(), "daemon-reload".to_string()];
        let reload = command_status_json("systemctl", &reload_args);
        let enable_args = vec![
            "--user".to_string(),
            "enable".to_string(),
            "--now".to_string(),
            "jig-proxy.service".to_string(),
        ];
        let enable = command_status_json("systemctl", &enable_args);
        json!({
            "ok": reload["ok"].as_bool().unwrap_or(false)
                && enable["ok"].as_bool().unwrap_or(false),
            "daemon_reload": reload,
            "enable_now": enable,
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        json!({ "ok": false, "unsupported": true })
    }
}

fn service_manager_status(path: &Path) -> Value {
    if !path.exists() {
        return json!({
            "ok": true,
            "skipped_no_file": true,
            "loaded": false,
            "enabled": false,
            "running": false,
        });
    }

    #[cfg(target_os = "macos")]
    {
        let domain = match launchctl_domain() {
            Ok(domain) => domain,
            Err(error) => {
                return json!({
                    "ok": false,
                    "error": error.to_string(),
                    "loaded": false,
                    "enabled": false,
                    "running": false,
                });
            }
        };
        let print_args = vec!["print".to_string(), format!("{domain}/sh.jig.proxy")];
        let print = command_output_json("launchctl", &print_args);
        let loaded = print["ok"].as_bool().unwrap_or(false);
        let running = loaded
            && print["stdout"]
                .as_str()
                .is_some_and(launchctl_print_state_is_running);
        json!({
            "ok": command_spawned(&print),
            "loaded": loaded,
            "enabled": loaded,
            "running": running,
            "print": print,
        })
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        let show_args = vec![
            "--user".to_string(),
            "show".to_string(),
            "jig-proxy.service".to_string(),
            "--property=LoadState".to_string(),
            "--value".to_string(),
        ];
        let show = command_output_json("systemctl", &show_args);
        let enabled_args = vec![
            "--user".to_string(),
            "is-enabled".to_string(),
            "jig-proxy.service".to_string(),
        ];
        let enabled = command_output_json("systemctl", &enabled_args);
        let active_args = vec![
            "--user".to_string(),
            "is-active".to_string(),
            "jig-proxy.service".to_string(),
        ];
        let active = command_output_json("systemctl", &active_args);
        let loaded = show["stdout"].as_str().unwrap_or_default() == "loaded";
        let is_enabled = enabled["stdout"].as_str().unwrap_or_default() == "enabled";
        let running = active["stdout"].as_str().unwrap_or_default() == "active";
        json!({
            "ok": command_spawned(&show) && command_spawned(&enabled) && command_spawned(&active),
            "loaded": loaded,
            "enabled": is_enabled,
            "running": running,
            "show": show,
            "is_enabled": enabled,
            "is_active": active,
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        json!({
            "ok": false,
            "unsupported": true,
            "loaded": false,
            "enabled": false,
            "running": false,
        })
    }
}

fn unload_service(path: &Path) -> Value {
    #[cfg(target_os = "macos")]
    {
        let domain = match launchctl_domain() {
            Ok(domain) => domain,
            Err(error) => {
                return json!({
                    "ok": false,
                    "error": error.to_string(),
                });
            }
        };
        let bootout_args = vec![
            "bootout".to_string(),
            domain,
            path.to_string_lossy().into_owned(),
        ];
        launchctl_bootout_json(&bootout_args)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = path;
        let disable_args = vec![
            "--user".to_string(),
            "disable".to_string(),
            "--now".to_string(),
            "jig-proxy.service".to_string(),
        ];
        let disable = command_status_json("systemctl", &disable_args);
        json!({
            "ok": disable["ok"].as_bool().unwrap_or(false),
            "disable_now": disable,
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        json!({ "ok": false, "unsupported": true })
    }
}

fn reload_after_remove_service(_path: &Path) -> Value {
    #[cfg(target_os = "linux")]
    {
        let reload_args = vec!["--user".to_string(), "daemon-reload".to_string()];
        let reload = command_status_json("systemctl", &reload_args);
        json!({
            "ok": reload["ok"].as_bool().unwrap_or(false),
            "daemon_reload": reload,
        })
    }

    #[cfg(target_os = "macos")]
    {
        json!({ "ok": true, "skipped": true })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        json!({ "ok": false, "unsupported": true })
    }
}

#[cfg(target_os = "macos")]
fn launchctl_bootout_json(args: &[String]) -> Value {
    let output = command_output_json("launchctl", args);
    if output["ok"].as_bool().unwrap_or(false) || !launchctl_output_means_not_loaded(&output) {
        return output;
    }
    json!({
        "ok": true,
        "status": output["status"].clone(),
        "skipped_not_loaded": true,
        "stdout": output["stdout"].clone(),
        "stderr": output["stderr"].clone(),
    })
}

#[cfg(any(target_os = "macos", test))]
fn launchctl_output_means_not_loaded(output: &Value) -> bool {
    let text = format!(
        "{}\n{}",
        output["stdout"].as_str().unwrap_or_default(),
        output["stderr"].as_str().unwrap_or_default()
    )
    .to_ascii_lowercase();
    text.contains("no such process")
        || text.contains("not loaded")
        || text.contains("could not find service")
        || text.contains("service is not loaded")
}

#[cfg(any(target_os = "macos", test))]
fn launchctl_print_state_is_running(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("state = running"))
}

fn privileged_port_note(settings: &ProxySettings) -> Option<&'static str> {
    let uses_privileged =
        settings.http_port < 1024 || settings.https_port.is_some_and(|port| port < 1024);
    if !uses_privileged {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        Some(
            "Ports below 1024 require root or cap_net_bind_service. For a user service, grant the installed jig binary with: sudo setcap 'cap_net_bind_service=+ep' <path-to-jig>.",
        )
    }
    #[cfg(target_os = "macos")]
    {
        Some(
            "Ports below 1024 require a root-owned LaunchDaemon or a local port-forward from 80/443 to an unprivileged Jig proxy port.",
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Some("Ports below 1024 may require elevated privileges on this platform.")
    }
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_service_file(path: &Path, body: &str) -> Result<()> {
    let tmp = file_ops::temp_path(path, "jig-proxy-service");
    let mut file = create_service_file(&tmp)?;
    file.write_all(body.as_bytes())?;
    file.sync_data()?;
    drop(file);
    file_ops::replace_file(&tmp, path, "jig-proxy-service")
}

fn ensure_service_parent_is_not_symlink(parent: &Path) -> Result<()> {
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!(
                "Refusing to write Jig proxy service file under symlinked directory {}",
                parent.display()
            );
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| {
            format!(
                "Failed to inspect Jig proxy service directory {}",
                parent.display()
            )
        }),
    }
}

fn create_service_file(path: &Path) -> Result<File> {
    file_ops::create_new_file(path, 0o644)
}

fn write_service_file_if_safe(path: &Path, body: &str) -> Result<bool> {
    if let Some(mut file) = open_existing_service_file(path)? {
        let mut existing = String::new();
        file.read_to_string(&mut existing)?;
        if existing != body {
            anyhow::bail!(
                "Refusing to overwrite existing Jig proxy service file {} because its contents differ. Run `scripts/jig proxy service uninstall` first or remove the file manually.",
                path.display()
            );
        }
        ensure_existing_service_file_permissions(path, &file)?;
        return Ok(false);
    }
    write_service_file(path, body)?;
    Ok(true)
}

fn open_existing_service_file(path: &Path) -> Result<Option<File>> {
    match file_ops::open_read_no_follow(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        #[cfg(unix)]
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            anyhow::bail!(
                "Refusing to reuse existing Jig proxy service file {} because it is a symlink.",
                path.display()
            )
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Refusing to reuse existing Jig proxy service file {}",
                path.display()
            )
        }),
    }
}

fn ensure_existing_service_file_permissions(path: &Path, file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o777;
        if mode & 0o022 != 0 {
            anyhow::bail!(
                "Refusing to reuse existing Jig proxy service file {} with permissions {:o}; remove group/world write bits first.",
                path.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    let _ = (path, file);
    Ok(())
}

fn unload_and_remove_service(
    path: &Path,
    unload: impl FnOnce(&Path) -> Value,
    reload_after_remove: impl FnOnce(&Path) -> Value,
) -> Result<Value> {
    let existed = path.exists();
    let unload = if existed {
        unload(path)
    } else {
        json!({ "ok": true, "skipped": true })
    };
    let unload_ok = unload["ok"].as_bool().unwrap_or(false);
    if existed && !unload_ok {
        return Ok(json!({
            "ok": false,
            "installed": true,
            "removed": false,
            "unload": unload,
            "path": path,
            "note": service_reload_hint(),
        }));
    }
    if existed {
        fs::remove_file(path)?;
    }
    let reload = if existed {
        reload_after_remove(path)
    } else {
        json!({ "ok": true, "skipped": true })
    };
    let reload_ok = reload["ok"].as_bool().unwrap_or(false);
    Ok(json!({
        "ok": reload_ok,
        "installed": false,
        "removed": existed,
        "unload": unload,
        "reload": reload,
        "path": path,
        "note": service_reload_hint(),
    }))
}

#[cfg(test)]
fn temp_service_path(path: &Path) -> PathBuf {
    file_ops::temp_path(path, "jig-proxy-service")
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote(input: &str) -> Result<String> {
    if input.contains('\r') || input.contains('\n') {
        anyhow::bail!("systemd unit value cannot contain CR or LF characters");
    }
    Ok(format!(
        "\"{}\"",
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('#', "\\x23")
            .replace('%', "%%")
    ))
}

#[cfg(any(target_os = "linux", test))]
fn systemd_exec_quote(input: &str) -> Result<String> {
    Ok(systemd_quote(input)?.replace('$', "$$"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn install_requires_accept_service_scope() {
        let temp = tempdir().unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };

        let error = install(
            &settings,
            PathBuf::from("/tmp/jig"),
            temp.path().join("repo"),
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("--accept-service-scope"));
        assert!(!settings.state_dir.as_ref().unwrap().exists());
    }

    #[test]
    fn launchctl_print_state_parser_requires_running_state() {
        assert!(launchctl_print_state_is_running(
            "domain = gui/501\nstate = running\n"
        ));
        assert!(!launchctl_print_state_is_running(
            "domain = gui/501\nstate = waiting\n"
        ));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn service_body_rejects_zero_ports() {
        let temp = tempdir().unwrap();
        let mut settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        settings.http_port = 0;
        let error = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTP port must be greater than 0"));

        settings.http_port = 1355;
        settings.https_port = Some(0);
        let error = service_body(&settings, &store, Path::new("/tmp/jig"), temp.path())
            .unwrap_err()
            .to_string();
        assert!(error.contains("HTTPS port must be greater than 0"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn service_body_sets_repo_root_environment() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo root");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();

        assert!(body.contains("JIG_PROXY_STATE_DIR"));
        assert!(body.contains("JIG_REPO_ROOT"));
        assert!(body.contains("WorkingDirectory"));
        assert!(body.contains("proxy.log"));
        assert!(body.contains(&repo.to_string_lossy().to_string()));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn service_body_preserves_http2_runtime_setting() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let mut settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
        assert!(!body.contains("--no-http2"));

        settings.http2 = false;
        let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
        assert!(body.contains("--no-http2"));
    }

    #[test]
    fn service_temp_paths_are_unique_within_process() {
        let temp = tempdir().unwrap();
        let service_path = temp.path().join("jig-proxy.service");

        let first = temp_service_path(&service_path);
        let second = temp_service_path(&service_path);

        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".tmp")
        );
        assert!(
            second
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".tmp")
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn install_response_reports_load_failure_but_written_file() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let service_path = temp.path().join("jig-proxy.service");

        let output = write_and_load_service(
            &settings,
            &store,
            Path::new("/tmp/jig"),
            &repo,
            &service_path,
            |_| json!({ "ok": false, "error": "load failed" }),
        )
        .unwrap();

        assert_eq!(output["ok"].as_bool(), Some(false));
        assert_eq!(output["installed"].as_bool(), Some(false));
        assert_eq!(output["file_written"].as_bool(), Some(true));
        assert!(service_path.exists());
        assert_eq!(output["load"]["error"].as_str(), Some("load failed"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn install_refuses_to_overwrite_different_service_file() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        fs::write(&service_path, "custom service").unwrap();

        let error = write_and_load_service(
            &settings,
            &store,
            Path::new("/tmp/jig"),
            &repo,
            &service_path,
            |_| json!({ "ok": true }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("Refusing to overwrite existing Jig proxy service file"));
        assert_eq!(fs::read_to_string(service_path).unwrap(), "custom service");
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn install_refuses_to_reuse_group_writable_service_file() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();
        fs::write(&service_path, body).unwrap();
        fs::set_permissions(&service_path, fs::Permissions::from_mode(0o664)).unwrap();

        let error = write_and_load_service(
            &settings,
            &store,
            Path::new("/tmp/jig"),
            &repo,
            &service_path,
            |_| json!({ "ok": true }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("group/world write bits"));
    }

    #[cfg(unix)]
    #[test]
    fn install_refuses_to_reuse_symlinked_service_file() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        let target = temp.path().join("target.service");
        fs::write(&target, "service").unwrap();
        std::os::unix::fs::symlink(&target, &service_path).unwrap();

        let error = write_and_load_service(
            &settings,
            &store,
            Path::new("/tmp/jig"),
            &repo,
            &service_path,
            |_| json!({ "ok": true }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("because it is a symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn install_refuses_symlinked_service_parent() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
        let real_parent = temp.path().join("real-services");
        let linked_parent = temp.path().join("linked-services");
        fs::create_dir_all(&real_parent).unwrap();
        std::os::unix::fs::symlink(&real_parent, &linked_parent).unwrap();
        let service_path = linked_parent.join("jig-proxy.service");

        let error = write_and_load_service(
            &settings,
            &store,
            Path::new("/tmp/jig"),
            &repo,
            &service_path,
            |_| json!({ "ok": true }),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("symlinked directory"));
        assert!(!real_parent.join("jig-proxy.service").exists());
    }

    #[test]
    fn uninstall_keeps_service_file_when_unload_fails() {
        let temp = tempdir().unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        fs::write(&service_path, "service").unwrap();

        let output = unload_and_remove_service(
            &service_path,
            |_| json!({ "ok": false, "error": "unload failed" }),
            |_| json!({ "ok": true }),
        )
        .unwrap();

        assert_eq!(output["ok"].as_bool(), Some(false));
        assert_eq!(output["removed"].as_bool(), Some(false));
        assert!(service_path.exists());
    }

    #[test]
    fn uninstall_removes_file_only_after_successful_unload_then_reloads() {
        let temp = tempdir().unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        fs::write(&service_path, "service").unwrap();

        let output = unload_and_remove_service(
            &service_path,
            |_| json!({ "ok": true }),
            |_| json!({ "ok": true, "daemon_reload": { "ok": true } }),
        )
        .unwrap();

        assert_eq!(output["ok"].as_bool(), Some(true));
        assert_eq!(output["removed"].as_bool(), Some(true));
        assert_eq!(output["reload"]["ok"].as_bool(), Some(true));
        assert!(!service_path.exists());
    }

    #[test]
    fn uninstall_reports_reload_failure_after_file_removal() {
        let temp = tempdir().unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        fs::write(&service_path, "service").unwrap();

        let output = unload_and_remove_service(
            &service_path,
            |_| json!({ "ok": true }),
            |_| json!({ "ok": false, "error": "reload failed" }),
        )
        .unwrap();

        assert_eq!(output["ok"].as_bool(), Some(false));
        assert_eq!(output["removed"].as_bool(), Some(true));
        assert_eq!(output["installed"].as_bool(), Some(false));
        assert!(!service_path.exists());
    }

    #[test]
    fn service_status_requires_file_and_loaded_enabled_manager_state() {
        let temp = tempdir().unwrap();
        let service_path = temp.path().join("jig-proxy.service");
        fs::write(&service_path, "service").unwrap();
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        let output = service_status_value(&settings, &store, &service_path, |_| {
            json!({
                "ok": true,
                "loaded": true,
                "enabled": false,
                "running": false,
            })
        });

        assert_eq!(output["ok"].as_bool(), Some(true));
        assert_eq!(output["file_present"].as_bool(), Some(true));
        assert_eq!(output["installed"].as_bool(), Some(false));
    }

    #[test]
    fn service_path_text_rejects_line_breaks() {
        let error = service_path_text(Path::new("/tmp/jig\nbin"), "current executable")
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot contain control characters"));
    }

    #[test]
    fn service_path_text_rejects_nul() {
        let error = service_path_text(Path::new("/tmp/jig\0bin"), "current executable")
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot contain control characters"));
    }

    #[test]
    fn service_path_text_rejects_relative_paths() {
        let error = service_path_text(Path::new("target/debug/jig"), "current executable")
            .unwrap_err()
            .to_string();

        assert!(error.contains("must be absolute"));
    }

    #[test]
    fn launchctl_not_loaded_output_is_not_uninstall_failure() {
        let output = json!({
            "ok": false,
            "status": 5,
            "stdout": "",
            "stderr": "Bootstrap failed: 5: Input/output error\nservice is not loaded"
        });

        assert!(launchctl_output_means_not_loaded(&output));
    }

    #[test]
    fn xml_escape_covers_apostrophes() {
        assert_eq!(
            xml_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }

    #[test]
    fn plist_string_escapes_body_text() {
        assert_eq!(
            plist_string("a&b<c>d\"e'f"),
            "<string>a&amp;b&lt;c&gt;d&quot;e&apos;f</string>"
        );
    }

    #[test]
    fn systemd_quote_escapes_comment_markers() {
        assert_eq!(
            systemd_quote("JIG_REPO_ROOT=/tmp/repo#1%$").unwrap(),
            "\"JIG_REPO_ROOT=/tmp/repo\\x231%%$\""
        );
    }

    #[test]
    fn systemd_exec_quote_escapes_command_dollars() {
        assert_eq!(
            systemd_exec_quote("/tmp/repo$1/bin/jig").unwrap(),
            "\"/tmp/repo$$1/bin/jig\""
        );
    }

    #[test]
    fn systemd_quote_handles_quotes_and_backslashes() {
        assert_eq!(
            systemd_quote(r#"JIG_REPO_ROOT=/tmp/repo "one" \ user's"#).unwrap(),
            r#""JIG_REPO_ROOT=/tmp/repo \"one\" \\ user's""#
        );
    }

    #[test]
    fn systemd_quote_rejects_line_breaks() {
        let error = systemd_quote("JIG_REPO_ROOT=/tmp/repo\nbad")
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot contain CR or LF"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn service_body_quotes_systemd_paths_with_spaces() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo root");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state dir")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        let body = service_body(&settings, &store, Path::new("/tmp/jig bin/jig"), &repo).unwrap();

        assert!(body.contains("ExecStart=\"/tmp/jig bin/jig\" proxy start"));
        assert!(body.contains("Environment=\"JIG_REPO_ROOT="));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn service_body_systemd_lines_start_at_column_zero() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let settings = ProxySettings {
            state_dir: Some(temp.path().join("state")),
            ..ProxySettings::default()
        };
        let store = StateStore::resolve(settings.state_dir.clone()).unwrap();

        let body = service_body(&settings, &store, Path::new("/tmp/jig"), &repo).unwrap();

        for line in body.lines().filter(|line| !line.is_empty()) {
            assert!(
                !line.chars().next().is_some_and(|ch| ch.is_whitespace()),
                "systemd unit line must start at column zero: {line:?}"
            );
            if line.starts_with('[') {
                assert!(
                    line.ends_with(']'),
                    "systemd section header must close on the same line: {line:?}"
                );
            } else {
                assert!(
                    line.contains('='),
                    "systemd directive must contain '=': {line:?}"
                );
            }
        }
    }
}
