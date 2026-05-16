use super::*;
use clap::CommandFactory;

mod help_helpers {
    use super::*;

    pub(super) fn rendered_help(path: &[&str]) -> String {
        let mut command = Cli::command();
        let mut current = &mut command;
        for (index, name) in path.iter().enumerate() {
            current = current.find_subcommand_mut(name).unwrap_or_else(|| {
                panic!("missing subcommand {name:?} at index {index} in path {path:?}")
            });
        }
        current.render_help().to_string()
    }

    pub(super) fn assert_help_contains(help: &str, expected: &str) {
        assert!(
            help.contains(expected),
            "expected rendered help to contain {expected:?}\n\n{help}"
        );
    }
}

use help_helpers::{assert_help_contains, rendered_help};

#[test]
fn template_errors_get_hint() {
    let missing_template_value =
        Cli::try_parse_from(["jig", "adopt", ".", "--template"]).unwrap_err();
    assert_eq!(
        missing_template_value.kind(),
        clap::error::ErrorKind::InvalidValue
    );
    assert!(should_add_template_hint(&missing_template_value));

    let unrelated = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();
    assert!(!should_add_template_hint(&unrelated));
}

#[test]
fn top_level_help_describes_common_commands() {
    let help = Cli::command().render_help().to_string();

    assert_help_contains(&help, "init");
    assert_help_contains(&help, "Create a new repository");
    assert_help_contains(&help, "Manage structured work plans");
    assert_help_contains(&help, "Inspect or bootstrap local agent tooling");
}

#[test]
fn nested_help_describes_work_and_agent_commands() {
    let work_help = Cli::command()
        .find_subcommand_mut("work")
        .unwrap()
        .render_help()
        .to_string();
    assert_help_contains(&work_help, "start");
    assert_help_contains(&work_help, "Start a structured work plan");
    assert_help_contains(&work_help, "gates");
    assert_help_contains(&work_help, "Show required gate status");

    let agent_help = Cli::command()
        .find_subcommand_mut("agent")
        .unwrap()
        .render_help()
        .to_string();
    assert_help_contains(&agent_help, "doctor");
    assert_help_contains(&agent_help, "Report local Codex marketplace readiness");
    assert_help_contains(&agent_help, "bootstrap");
    assert_help_contains(
        &agent_help,
        "Register the configured Codex skills marketplace",
    );
}

#[test]
fn work_start_help_includes_examples() {
    let work_start_help = rendered_help(&["work", "start"]);
    assert_help_contains(&work_start_help, "jig work start --title \"Add auth\"");
    assert_help_contains(&work_start_help, "--body-file .agent/notes/signup-plan.md");
}

#[test]
fn work_check_help_includes_examples() {
    let work_check_help = rendered_help(&["work", "check"]);
    assert_help_contains(&work_check_help, "jig work check --plan-id plan_abc123");
    assert_help_contains(&work_check_help, "--tool jig.test");
}

#[test]
fn work_finish_help_includes_examples() {
    let work_finish_help = rendered_help(&["work", "finish"]);
    assert_help_contains(&work_finish_help, "jig work finish --plan-id plan_abc123");
    assert_help_contains(&work_finish_help, "--outcome success");
}

#[test]
fn agent_help_includes_examples() {
    let agent_help = rendered_help(&["agent"]);
    assert_help_contains(&agent_help, "jig agent doctor");
    assert_help_contains(&agent_help, "jig agent bootstrap");
}

#[test]
fn agent_bootstrap_help_includes_examples() {
    let agent_bootstrap_help = rendered_help(&["agent", "bootstrap"]);
    assert_help_contains(&agent_bootstrap_help, "GitHub owner/repo skill marketplace");
    assert_help_contains(
        &agent_bootstrap_help,
        "jig agent bootstrap --marketplace owner/skills-repo",
    );
}

#[test]
fn human_summary_flags_are_discoverable() {
    let agent_doctor_help = rendered_help(&["agent", "doctor"]);
    assert_help_contains(&agent_doctor_help, "--summary");
    assert_help_contains(&agent_doctor_help, "human-readable readiness summary");

    let work_status_help = rendered_help(&["work", "status"]);
    assert_help_contains(&work_status_help, "--summary");
    assert_help_contains(&work_status_help, "human-readable work summary");
}

#[test]
fn proxy_run_help_includes_launcher_context_and_examples() {
    let proxy_run_help = rendered_help(&["proxy", "run"]);
    assert_help_contains(&proxy_run_help, "The app command must come after --");
    assert_help_contains(&proxy_run_help, "[[dev.apps]].host");
    assert_help_contains(&proxy_run_help, "jig proxy run web -- npm run dev");
    assert_help_contains(&proxy_run_help, "jig proxy run web -- vite --open");
    assert_help_contains(
        &proxy_run_help,
        "jig proxy run api --port 3000 -- cargo run",
    );
    assert_help_contains(
        &proxy_run_help,
        "jig proxy run web --no-proxy -- npm run dev",
    );
}

#[test]
fn migration_help_includes_examples() {
    let migration_help = rendered_help(&["migration-add"]);
    assert_help_contains(&migration_help, "open structured work plan");
    assert_help_contains(&migration_help, "jig migration-add create_users");
    assert_help_contains(&migration_help, "--plan-id plan_abc123");
}

#[test]
fn adopt_and_init_default_to_official_template() {
    let adopt = Cli::try_parse_from(["jig", "adopt", ".", "--repo-name", "demo"]).unwrap();
    match adopt.command {
        CommandKind::Adopt(bootstrap::AdoptOpts { template, .. }) => {
            assert_eq!(template, None);
        }
        other => panic!("expected adopt command, got {other:?}"),
    }

    let init = Cli::try_parse_from(["jig", "init", "/tmp/demo", "--repo-name", "demo"]).unwrap();
    match init.command {
        CommandKind::Init(bootstrap::InitOpts { template, .. }) => {
            assert_eq!(template, None);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn parses_init_command_with_repeatable_flags() {
    let cli = Cli::try_parse_from([
        "jig",
        "init",
        "/tmp/demo",
        "--template",
        "/tmp/template",
        "--template-mode",
        "committed",
        "--repo-name",
        "demo",
        "--rust-migration-dir",
        "migrations",
        "--rust-crate-root",
        "crates",
        "--rust-crate-root",
        "libs",
        "--frontend-app",
        "frontend:web:40",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Init(bootstrap::InitOpts {
            template_mode,
            answers,
            ..
        }) => {
            assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
            assert_eq!(answers.rust_crate_roots, vec!["crates", "libs"]);
            assert_eq!(answers.frontend_apps.len(), 1);
        }
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn rejects_working_tree_template_mode() {
    let error = Cli::try_parse_from([
        "jig",
        "init",
        "/tmp/demo",
        "--template",
        "/tmp/template",
        "--template-mode",
        "working-tree",
    ])
    .unwrap_err()
    .to_string();

    assert!(error.contains("invalid value 'working-tree'"));
    assert!(error.contains("committed"));
}

#[test]
fn parses_update_recopy_flag() {
    let cli = Cli::try_parse_from([
        "jig",
        "update",
        "--recopy",
        "--force",
        "--template",
        "/tmp/template",
        "--template-mode",
        "committed",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Update(bootstrap::UpdateOpts {
            recopy,
            force,
            template,
            template_mode,
            ..
        }) => {
            assert!(recopy);
            assert!(force);
            assert_eq!(template.as_deref(), Some("/tmp/template"));
            assert_eq!(template_mode, Some(bootstrap::TemplateMode::Committed));
        }
        other => panic!("expected update command, got {other:?}"),
    }
}

#[test]
fn parses_work_receipts_filters() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "receipts",
        "--session-id",
        "session_1",
        "--plan-id",
        "plan_1",
        "--tool-name",
        tool::TEST,
        "--failed-only",
        "--limit",
        "5",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Receipts(opts)) => {
            assert_eq!(opts.session_id.as_deref(), Some("session_1"));
            assert_eq!(opts.plan_id.as_deref(), Some("plan_1"));
            assert_eq!(opts.tool_name.as_deref(), Some(tool::TEST));
            assert!(opts.failed_only);
            assert_eq!(opts.limit, 5);
        }
        other => panic!("expected work receipts command, got {other:?}"),
    }
}

#[test]
fn parses_work_goal() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "goal",
        "--objective",
        "Migrate the API",
        "--success",
        "all handlers use the new type",
        "--validation",
        "make test",
        "--validation",
        "make clippy",
        "--constraint",
        "do not change public routes",
        "--checkpoint",
        "baseline current tests",
        "--title",
        "API migration",
        "--notes",
        "Keep changes small.",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Goal(opts)) => {
            assert_eq!(opts.objective, "Migrate the API");
            assert_eq!(opts.success, "all handlers use the new type");
            assert_eq!(opts.validations, vec!["make test", "make clippy"]);
            assert_eq!(opts.constraints, vec!["do not change public routes"]);
            assert_eq!(opts.checkpoints, vec!["baseline current tests"]);
            assert_eq!(opts.title.as_deref(), Some("API migration"));
            assert_eq!(opts.notes.as_deref(), Some("Keep changes small."));
        }
        other => panic!("expected work goal command, got {other:?}"),
    }
}

#[test]
fn parses_agent_doctor_command() {
    let cli = Cli::try_parse_from(["jig", "agent", "doctor"]).unwrap();

    match cli.command {
        CommandKind::Agent(AgentCommand::Doctor(opts)) => {
            assert!(!opts.summary);
        }
        other => panic!("expected agent doctor command, got {other:?}"),
    }

    let summary = Cli::try_parse_from(["jig", "agent", "doctor", "--summary"]).unwrap();
    match summary.command {
        CommandKind::Agent(AgentCommand::Doctor(opts)) => {
            assert!(opts.summary);
        }
        other => panic!("expected agent doctor summary command, got {other:?}"),
    }
}

#[test]
fn parses_agent_bootstrap_marketplace() {
    let cli = Cli::try_parse_from([
        "jig",
        "agent",
        "bootstrap",
        "--marketplace",
        "../jig-skills",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Agent(AgentCommand::Bootstrap(opts)) => {
            assert_eq!(opts.marketplace.as_deref(), Some("../jig-skills"));
        }
        other => panic!("expected agent bootstrap command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_run_command() {
    let cli = Cli::try_parse_from([
        "jig",
        "proxy",
        "run",
        "web",
        "--kind",
        "vite",
        "--http-port",
        "1555",
        "--",
        "vite",
        "--open",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Run(opts)) => {
            assert_eq!(opts.name, "web");
            assert_eq!(opts.kind.as_deref(), Some("vite"));
            assert_eq!(opts.proxy.http_port, Some(1555));
            assert!(!opts.no_proxy);
            assert_eq!(opts.command, vec!["vite", "--open"]);
        }
        other => panic!("expected proxy run command, got {other:?}"),
    }
}

#[test]
fn proxy_run_requires_separator_before_command() {
    let error = Cli::try_parse_from(["jig", "proxy", "run", "web", "vite"]).unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
}

#[test]
fn parses_proxy_run_no_proxy() {
    let cli = Cli::try_parse_from([
        "jig",
        "proxy",
        "run",
        "web",
        "--no-proxy",
        "--",
        "cargo",
        "run",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Run(opts)) => {
            assert!(opts.no_proxy);
            assert_eq!(opts.command, vec!["cargo", "run"]);
        }
        other => panic!("expected proxy run command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_state_dir() {
    let cli = Cli::try_parse_from(["jig", "proxy", "list", "--state-dir", "/tmp/jig-proxy-test"])
        .unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::List(opts)) => {
            assert_eq!(
                opts.proxy.state_dir,
                Some(PathBuf::from("/tmp/jig-proxy-test"))
            );
        }
        other => panic!("expected proxy list command, got {other:?}"),
    }
}

#[test]
fn parses_proxy_alias_port_flag() {
    let cli = Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "8080"]).unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Alias(opts)) => {
            assert_eq!(opts.name, "api");
            assert_eq!(opts.port, 8080);
        }
        other => panic!("expected proxy alias command, got {other:?}"),
    }
}

#[test]
fn proxy_alias_host_rejects_non_ip_literals_at_parse_time() {
    let error = Cli::try_parse_from([
        "jig",
        "proxy",
        "alias",
        "api",
        "--port",
        "8080",
        "--host",
        "localhost",
    ])
    .unwrap_err();

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn proxy_ports_reject_zero_at_parse_time() {
    let alias_error =
        Cli::try_parse_from(["jig", "proxy", "alias", "api", "--port", "0"]).unwrap_err();
    assert_eq!(alias_error.kind(), clap::error::ErrorKind::ValueValidation);

    let run_error =
        Cli::try_parse_from(["jig", "proxy", "run", "web", "--port", "0", "--", "vite"])
            .unwrap_err();
    assert_eq!(run_error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn proxy_cert_trust_requires_scope_acknowledgement_at_parse_time() {
    for command in ["trust", "untrust"] {
        let error = Cli::try_parse_from(["jig", "proxy", "cert", command]).unwrap_err();

        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
}

#[test]
fn proxy_service_install_requires_scope_acknowledgement_at_parse_time() {
    let error = Cli::try_parse_from(["jig", "proxy", "service", "install"]).unwrap_err();

    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn proxy_json_ok_false_is_cli_failure() {
    let error = require_json_ok(true, &serde_json::json!({ "ok": false }))
        .unwrap_err()
        .to_string();

    assert!(error.contains("ok=false"));
    require_json_ok(false, &serde_json::json!({ "ok": false })).unwrap();
    assert!(command_reports_failure_with_ok(&CommandKind::Dev(
        DevOpts {
            apps: Vec::new(),
            discover_workspace: false,
            no_proxy: false,
            proxy: ProxyRuntimeOpts::default(),
        }
    )));
    assert!(command_reports_failure_with_ok(&CommandKind::Agent(
        AgentCommand::Doctor(AgentDoctorOpts::default())
    )));
}

#[test]
fn parses_proxy_runtime_flags_on_prune_cert_and_service_commands() {
    let prune =
        Cli::try_parse_from(["jig", "proxy", "prune", "--state-dir", "/tmp/proxy"]).unwrap();
    match prune.command {
        CommandKind::Proxy(ProxyCommand::Prune(opts)) => {
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy prune command, got {other:?}"),
    }

    let cert = Cli::try_parse_from(["jig", "proxy", "cert", "status", "--tld", "test"]).unwrap();
    match cert.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Status(opts))) => {
            assert_eq!(opts.proxy.tld.as_deref(), Some("test"));
        }
        other => panic!("expected proxy cert status command, got {other:?}"),
    }

    let cert_trust = Cli::try_parse_from([
        "jig",
        "proxy",
        "cert",
        "trust",
        "--accept-trust-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match cert_trust.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Trust(opts))) => {
            assert!(opts.accept_trust_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy cert trust command, got {other:?}"),
    }

    let cert_untrust = Cli::try_parse_from([
        "jig",
        "proxy",
        "cert",
        "untrust",
        "--accept-trust-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match cert_untrust.command {
        CommandKind::Proxy(ProxyCommand::Cert(ProxyCertCommand::Untrust(opts))) => {
            assert!(opts.accept_trust_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy cert untrust command, got {other:?}"),
    }

    let service = Cli::try_parse_from([
        "jig",
        "proxy",
        "service",
        "status",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match service.command {
        CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Status(opts))) => {
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy service status command, got {other:?}"),
    }

    let service_install = Cli::try_parse_from([
        "jig",
        "proxy",
        "service",
        "install",
        "--accept-service-scope",
        "--state-dir",
        "/tmp/proxy",
    ])
    .unwrap();
    match service_install.command {
        CommandKind::Proxy(ProxyCommand::Service(ProxyServiceCommand::Install(opts))) => {
            assert!(opts.accept_service_scope);
            assert_eq!(opts.proxy.state_dir, Some(PathBuf::from("/tmp/proxy")));
        }
        other => panic!("expected proxy service install command, got {other:?}"),
    }
}

#[test]
fn parses_dev_command_with_selected_apps() {
    let cli = Cli::try_parse_from([
        "jig", "dev", "--app", "web", "--app", "api", "--https", "--lan",
    ])
    .unwrap();

    match cli.command {
        CommandKind::Dev(opts) => {
            assert_eq!(opts.apps, vec!["web", "api"]);
            assert!(opts.proxy.https);
            assert!(opts.proxy.lan);
        }
        other => panic!("expected dev command, got {other:?}"),
    }
}

#[test]
fn parses_hidden_proxy_no_http2_runtime_flag() {
    let cli = Cli::try_parse_from(["jig", "proxy", "start", "--foreground", "--no-http2"]).unwrap();

    match cli.command {
        CommandKind::Proxy(ProxyCommand::Start(opts)) => {
            assert!(opts.foreground);
            assert!(opts.proxy.no_http2);
        }
        other => panic!("expected proxy start command, got {other:?}"),
    }
}

#[test]
fn parses_work_status_command() {
    let cli = Cli::try_parse_from(["jig", "work", "status"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Status(opts)) => {
            assert!(!opts.summary);
        }
        other => panic!("expected work status command, got {other:?}"),
    }

    let summary = Cli::try_parse_from(["jig", "work", "status", "--summary"]).unwrap();
    match summary.command {
        CommandKind::Work(WorkCommand::Status(opts)) => {
            assert!(opts.summary);
        }
        other => panic!("expected work status summary command, got {other:?}"),
    }
}

#[test]
fn parses_work_check_tools() {
    let cli = Cli::try_parse_from([
        "jig",
        "work",
        "check",
        "--plan-id",
        "plan_1",
        "--tool",
        tool::CONTRACT_CHECK,
        "--tool",
        tool::TEST,
    ])
    .unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Check(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
            assert_eq!(opts.tools, vec![tool::CONTRACT_CHECK, tool::TEST]);
        }
        other => panic!("expected work check command, got {other:?}"),
    }
}

#[test]
fn parses_work_gates_command() {
    let cli = Cli::try_parse_from(["jig", "work", "gates", "--plan-id", "plan_1"]).unwrap();

    match cli.command {
        CommandKind::Work(WorkCommand::Gates(opts)) => {
            assert_eq!(opts.plan_id, "plan_1");
        }
        other => panic!("expected work gates command, got {other:?}"),
    }
}
