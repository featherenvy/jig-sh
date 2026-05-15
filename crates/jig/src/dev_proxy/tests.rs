use std::fs;

use serde_json::json;
use tempfile::tempdir;

use super::*;
use crate::context::DevConfig;

fn write_contract(root: &std::path::Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["contract-check"],
            "optional_make_targets": [],
            "tools": [],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_config(root: &std::path::Path, extra: &str) {
    write_contract(root);
    fs::write(
        root.join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
{extra}
"#
        ),
    )
    .unwrap();
}

#[test]
fn dev_config_defaults_match_proxy_settings_defaults() {
    let dev = DevConfig::default();
    let proxy = jig_dev_proxy::ProxySettings::default();

    assert_eq!(dev.proxy_port, proxy.http_port);
    assert_eq!(dev.https_port, proxy.https_port);
    assert_eq!(dev.tld, proxy.tld);
}

#[test]
fn dev_apps_cannot_be_combined_with_legacy_frontend_apps() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
dir = "apps/web"
argv = ["bun", "run", "dev"]
"#,
    )
    .unwrap();

    let error = RepoContext::load_from(temp.path()).unwrap_err().to_string();

    assert!(error.contains("cannot both be configured"));
}

#[test]
fn legacy_frontend_apps_are_used_when_dev_apps_are_absent() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "pnpm"

[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80

[dev]
proxy_port = 1555
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();

    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "web");
    assert_eq!(apps[0].kind, jig_dev_proxy::AppKind::Vite);
    assert!(matches!(
        &apps[0].command,
        jig_dev_proxy::CommandSpec::Argv(argv)
            if argv == &vec!["pnpm".to_string(), "run".to_string(), "dev".to_string()]
    ));
}

#[test]
fn unknown_dev_app_kind_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vit"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("Unsupported dev app kind"));
}

#[test]
fn dev_app_host_must_be_ip_literal() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
host = "api.example.test"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must be an IP literal"));
}

#[test]
fn proxied_dev_app_host_must_be_loopback() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[[dev.apps]]
name = "web"
host = "192.0.2.10"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must target a loopback IP literal"));
}

#[test]
fn non_proxied_dev_app_may_use_non_loopback_direct_host() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[[dev.apps]]
name = "web"
host = "192.0.2.10"
proxy = false
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let apps = configured_apps(&ctx, &settings).unwrap();

    assert_eq!(apps[0].target_host, "192.0.2.10");
    assert!(!apps[0].proxy);
}

#[test]
fn dev_app_name_rejects_surrounding_whitespace() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[[dev.apps]]
name = " web "
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must not contain leading or trailing whitespace"));
}

#[test]
fn dev_app_dirs_must_stay_under_repo_root() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    let outside = tempdir().unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
dir = "{}"
command = "bun run dev"
"#,
            outside.path().display()
        ),
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("resolves outside repo root"));
}

#[test]
fn settings_do_not_require_configured_app_dirs_to_exist() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[dev.apps]]
name = "web"
dir = "missing-app-dir"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("dev app dir"));
    assert!(error.contains("must exist"));
}

#[test]
fn vite_dev_app_requires_argv() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
web_package_manager = "bun"

[dev]
proxy_port = 1555

[[dev.apps]]
name = "web"
kind = "vite"
command = "bun run dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let settings = settings(&ctx, &ProxyRuntimeOpts::default()).unwrap();
    let error = configured_apps(&ctx, &settings).unwrap_err().to_string();

    assert!(error.contains("must set argv"));
}

#[test]
fn invalid_dev_tld_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
tld = "bad,tld"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOpts::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("invalid hostname"));
}

#[test]
fn public_dev_tld_is_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
tld = "dev"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOpts::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("is not allowed"));
}

#[test]
fn zero_proxy_ports_are_rejected() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
proxy_port = 0
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOpts::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("proxy HTTP port must be greater than 0"));
}

#[test]
fn explicit_read_only_state_dir_must_exist() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
"#,
    )
    .unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let opts = ProxyRuntimeOpts {
        state_dir: Some(temp.path().join("missing-state")),
        ..ProxyRuntimeOpts::default()
    };

    let error = settings_existing_state_dir(&ctx, &opts)
        .unwrap_err()
        .to_string();

    assert!(error.contains("does not exist"));
}

#[test]
fn settings_does_not_create_missing_state_dir() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
"#,
    )
    .unwrap();
    let missing = temp.path().join("missing-state");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let opts = ProxyRuntimeOpts {
        state_dir: Some(missing.clone()),
        ..ProxyRuntimeOpts::default()
    };

    let settings = settings(&ctx, &opts).unwrap();

    assert_eq!(settings.state_dir.as_deref(), Some(missing.as_path()));
    assert!(!missing.exists());
}

#[test]
fn no_proxy_rejects_proxy_runtime_flags() {
    let opts = ProxyRuntimeOpts {
        https: true,
        tld: Some("localhost".into()),
        ..ProxyRuntimeOpts::default()
    };

    let error = reject_no_proxy_runtime_flags(true, &opts)
        .unwrap_err()
        .to_string();

    assert!(error.contains("--no-proxy cannot be combined"));
    assert!(error.contains("--https"));
    assert!(error.contains("--tld"));
}

#[test]
fn no_proxy_allows_state_dir_for_other_proxy_commands() {
    let opts = ProxyRuntimeOpts {
        state_dir: Some(PathBuf::from("/tmp/jig-proxy-state")),
        ..ProxyRuntimeOpts::default()
    };

    reject_no_proxy_runtime_flags(true, &opts).unwrap();
}

#[test]
fn contextless_proxy_commands_are_limited_to_host_cleanup_and_status() {
    assert!(commands::can_run_without_context(&ProxyCommand::Stop(
        ProxyStopOpts::default()
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Service(
        ProxyServiceCommand::Status(ProxyServiceRuntimeOpts::default())
    )));
    assert!(commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartOpts {
            foreground: true,
            proxy: ProxyRuntimeOpts::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Start(
        ProxyStartOpts {
            foreground: false,
            proxy: ProxyRuntimeOpts::default(),
        }
    )));
    assert!(!commands::can_run_without_context(&ProxyCommand::Cert(
        ProxyCertCommand::Generate(ProxyCertGenerateOpts::default())
    )));
}

#[test]
fn contextless_proxy_allowlist_is_exhaustive() {
    let commands = proxy_command_cases();
    let allowed = commands
        .iter()
        .filter_map(|command| {
            commands::can_run_without_context(command).then_some(proxy_command_case_name(command))
        })
        .collect::<Vec<_>>();

    assert_eq!(
        allowed,
        vec![
            "start:foreground",
            "stop",
            "list",
            "prune",
            "cert:status",
            "cert:trust",
            "cert:untrust",
            "service:uninstall",
            "service:status",
        ]
    );
}

fn proxy_command_cases() -> Vec<ProxyCommand> {
    vec![
        ProxyCommand::Start(ProxyStartOpts {
            foreground: true,
            proxy: ProxyRuntimeOpts::default(),
        }),
        ProxyCommand::Start(ProxyStartOpts {
            foreground: false,
            proxy: ProxyRuntimeOpts::default(),
        }),
        ProxyCommand::Stop(ProxyStopOpts::default()),
        ProxyCommand::List(ProxyListOpts::default()),
        ProxyCommand::Prune(ProxyPruneOpts::default()),
        ProxyCommand::Run(ProxyRunOpts {
            name: "web".into(),
            kind: None,
            dir: None,
            port: Some(3000),
            no_proxy: false,
            proxy: ProxyRuntimeOpts::default(),
            command: vec!["npm".into(), "run".into(), "dev".into()],
        }),
        ProxyCommand::Alias(ProxyAliasOpts {
            name: "web".into(),
            port: 3000,
            host: "127.0.0.1".into(),
            accept_non_loopback_target: false,
            proxy: ProxyRuntimeOpts::default(),
        }),
        ProxyCommand::Cert(ProxyCertCommand::Generate(ProxyCertGenerateOpts::default())),
        ProxyCommand::Cert(ProxyCertCommand::Status(ProxyCertRuntimeOpts::default())),
        ProxyCommand::Cert(ProxyCertCommand::Trust(ProxyCertTrustOpts {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOpts::default(),
        })),
        ProxyCommand::Cert(ProxyCertCommand::Untrust(ProxyCertUntrustOpts {
            accept_trust_scope: true,
            proxy: ProxyRuntimeOpts::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Install(ProxyServiceInstallOpts {
            accept_service_scope: true,
            proxy: ProxyRuntimeOpts::default(),
        })),
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(
            ProxyServiceRuntimeOpts::default(),
        )),
        ProxyCommand::Service(ProxyServiceCommand::Status(
            ProxyServiceRuntimeOpts::default(),
        )),
    ]
}

fn proxy_command_case_name(command: &ProxyCommand) -> &'static str {
    match command {
        ProxyCommand::Start(opts) if opts.foreground => "start:foreground",
        ProxyCommand::Start(_) => "start:background",
        ProxyCommand::Stop(_) => "stop",
        ProxyCommand::List(_) => "list",
        ProxyCommand::Prune(_) => "prune",
        ProxyCommand::Run(_) => "run",
        ProxyCommand::Alias(_) => "alias",
        ProxyCommand::Cert(ProxyCertCommand::Generate(_)) => "cert:generate",
        ProxyCommand::Cert(ProxyCertCommand::Status(_)) => "cert:status",
        ProxyCommand::Cert(ProxyCertCommand::Trust(_)) => "cert:trust",
        ProxyCommand::Cert(ProxyCertCommand::Untrust(_)) => "cert:untrust",
        ProxyCommand::Service(ProxyServiceCommand::Install(_)) => "service:install",
        ProxyCommand::Service(ProxyServiceCommand::Uninstall(_)) => "service:uninstall",
        ProxyCommand::Service(ProxyServiceCommand::Status(_)) => "service:status",
    }
}

#[test]
fn contextless_proxy_settings_use_runtime_flags() {
    let temp = tempdir().unwrap();
    let settings = settings_without_context(&ProxyRuntimeOpts {
        state_dir: Some(temp.path().to_path_buf()),
        http_port: Some(1555),
        https_port: Some(1556),
        https: true,
        no_https: false,
        http2: false,
        no_http2: true,
        lan: true,
        no_lan: false,
        tld: Some("Test".into()),
    })
    .unwrap();

    assert_eq!(settings.state_dir, Some(temp.path().to_path_buf()));
    assert_eq!(settings.http_port, 1555);
    assert_eq!(settings.https_port, Some(1556));
    assert!(settings.https);
    assert!(!settings.http2);
    assert!(settings.lan);
    assert_eq!(settings.tld, "test");
    assert!(settings.additional_dns_names.is_empty());
}

#[test]
fn proxy_runtime_flags_can_disable_configured_https_and_lan() {
    let temp = tempdir().unwrap();
    write_config(
        temp.path(),
        r#"
[dev]
https = true
lan = true
"#,
    );
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let settings = settings(
        &ctx,
        &ProxyRuntimeOpts {
            no_https: true,
            no_lan: true,
            ..ProxyRuntimeOpts::default()
        },
    )
    .unwrap();

    assert!(!settings.https);
    assert!(!settings.lan);
}

#[test]
fn proxy_http_and_https_ports_must_differ() {
    let temp = tempdir().unwrap();
    write_contract(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[dev]
proxy_port = 1555
https_port = 1555
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = settings(&ctx, &ProxyRuntimeOpts::default())
        .unwrap_err()
        .to_string();

    assert!(error.contains("must be different"));
}
