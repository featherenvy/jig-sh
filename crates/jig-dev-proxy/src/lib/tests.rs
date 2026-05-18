use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use tempfile::tempdir;

use super::*;

#[test]
fn proxy_runtime_status_reports_missing_runtime_state() {
    let temp = tempdir().unwrap();
    let store = StateStore::resolve(Some(temp.path().to_path_buf())).unwrap();

    let status = proxy_runtime_status(&store).unwrap();

    assert_eq!(status.pid, None);
    assert!(!status.pid_alive);
    assert_eq!(status.http_port, None);
    assert_eq!(status.health_pid, None);
    assert!(!status.handshake_ok);
    assert!(!status.pid_matches_proxy);
}

#[cfg(unix)]
#[test]
fn signal_esrch_is_treated_as_already_stopped() {
    assert_eq!(
        classify_signal_error(Some(libc::ESRCH)),
        SignalResult::NotFound
    );
}

#[test]
fn proxy_stop_keeps_runtime_files_for_live_unverified_pid() {
    let temp = tempdir().unwrap();
    let settings = ProxySettings {
        state_dir: Some(temp.path().to_path_buf()),
        ..ProxySettings::default()
    };
    let store = StateStore::resolve(settings.state_dir.clone()).unwrap();
    store.write_pid(std::process::id()).unwrap();
    store.write_http_port(closed_loopback_port()).unwrap();

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

fn closed_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
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
