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
