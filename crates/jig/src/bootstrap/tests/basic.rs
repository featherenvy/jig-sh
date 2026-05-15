use super::*;

#[test]
fn parses_frontend_app_flag() {
    let app = parse_frontend_app("frontend:web:40").unwrap();
    assert_eq!(
        app,
        FrontendApp {
            name: "frontend".into(),
            dir: "web".into(),
            coverage_threshold: 40,
        }
    );
}

#[test]
fn seed_answers_only_serializes_provided_values() {
    let toml = seed_answers_toml(
        &AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            rust_crate_roots: vec!["crates".into()],
            frontend_apps: vec![FrontendApp {
                name: "frontend".into(),
                dir: "web".into(),
                coverage_threshold: 40,
            }],
            ..AnswerOpts::default()
        },
        &PrivateAnswerOverrides::default(),
    );

    let mapping = toml.as_table().unwrap();
    assert_eq!(
        mapping.get("repo_name").unwrap(),
        &TomlValue::String("demo".into())
    );
    assert_eq!(
        mapping.get("sqlx_enabled").unwrap(),
        &TomlValue::Boolean(false)
    );
    assert!(mapping.contains_key("rust_crate_roots"));
    assert!(!mapping.contains_key("default_branch"));
}

fn write_answers_fixture(dir: &Path, sqlx_enabled: Option<bool>) {
    let mut body = String::from("default_branch = \"main\"\n");
    if let Some(sqlx_enabled) = sqlx_enabled {
        body.push_str(&format!(
            "sqlx_enabled = {}\n",
            if sqlx_enabled { "true" } else { "false" }
        ));
    }
    fs::write(dir.join(".jig.toml"), body).unwrap();
}

fn with_test_build_template_pin_policy<T>(
    policy: BuildTemplatePinPolicy,
    run: impl FnOnce() -> T,
) -> T {
    struct Guard(Option<BuildTemplatePinPolicy>);

    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| slot.set(self.0));
        }
    }

    let previous = TEST_BUILD_TEMPLATE_PIN_POLICY.with(|slot| {
        let previous = slot.get();
        slot.set(Some(policy));
        previous
    });
    let _guard = Guard(previous);
    run()
}

#[test]
fn rendered_conflicts_detects_generated_paths() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(destination.path().join("scripts/jig"), "existing").unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[test]
fn rendered_conflicts_marks_task_mutated_outputs() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::write(rendered.path().join("agent-map.md"), "placeholder").unwrap();
    fs::write(destination.path().join("agent-map.md"), "existing").unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["agent-map.md"]);
}

#[test]
fn rendered_conflicts_marks_sqlx_pruned_task_outputs() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(false));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(
        rendered.path().join("scripts/add-migration.sh"),
        "templated",
    )
    .unwrap();
    fs::write(
        destination.path().join("scripts/add-migration.sh"),
        "existing",
    )
    .unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["scripts/add-migration.sh"]);
}

#[test]
fn rendered_conflicts_ignores_identical_files() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/jig"), "same").unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert!(conflicts.is_empty());
}

#[cfg(unix)]
#[test]
fn rendered_conflicts_detects_executable_bit_changes() {
    use std::os::unix::fs::PermissionsExt;

    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/jig"), "same").unwrap();
    fs::set_permissions(
        rendered.path().join("scripts/jig"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::set_permissions(
        destination.path().join("scripts/jig"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[cfg(unix)]
#[test]
fn rendered_conflicts_detects_file_replacing_symlink() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::create_dir_all(destination.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "same").unwrap();
    fs::write(destination.path().join("scripts/target"), "same").unwrap();
    create_symlink(Path::new("target"), &destination.path().join("scripts/jig")).unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["scripts/jig"]);
}

#[test]
fn rendered_conflicts_detects_blocking_ancestor_file() {
    let rendered = tempdir().unwrap();
    let destination = tempdir().unwrap();
    write_answers_fixture(rendered.path(), Some(true));
    fs::create_dir_all(rendered.path().join("scripts")).unwrap();
    fs::write(rendered.path().join("scripts/jig"), "rendered").unwrap();
    fs::write(destination.path().join("scripts"), "blocking file").unwrap();

    let conflicts = rendered_conflicts(
        rendered.path(),
        &rendered.path().join(".jig.toml"),
        destination.path(),
    )
    .unwrap();
    assert_eq!(conflicts, vec!["scripts"]);
}

#[test]
fn preview_workspace_only_copies_agent_guides() {
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    fs::create_dir_all(source.path().join("crates/api")).unwrap();
    fs::create_dir_all(source.path().join("target/debug")).unwrap();
    fs::write(source.path().join("AGENTS.md"), "root").unwrap();
    fs::write(source.path().join("crates/api/AGENTS.md"), "nested").unwrap();
    fs::write(source.path().join("target/debug/build.log"), "noise").unwrap();

    seed_preview_workspace(source.path(), destination.path()).unwrap();

    assert!(destination.path().join("AGENTS.md").exists());
    assert!(destination.path().join("crates/api/AGENTS.md").exists());
    assert!(!destination.path().join("target/debug/build.log").exists());
}

#[test]
fn run_init_uses_native_renderer_and_git() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.toml").exists());
    assert!(destination.join("scripts/jig").exists());
}

#[test]
fn adopt_without_template_uses_official_template_release_tag_and_records_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();
    let fake_commit = "0123456789abcdef0123456789abcdef01234567";

    let log_path = temp.path().join("commands.log");
    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            r#"#!/bin/sh
printf 'git %s\n' "$*" >> "{log_path}"
if [ "$1" = "clone" ]; then
  mkdir -p "$4"
  cp -R "{template}/." "$4"
  exit 0
fi
if [ "$1" = "rev-parse" ]; then
  printf '{fake_commit}\n'
  exit 0
fi
exit 0
"#,
            log_path = log_path.display(),
            template = template.path().display(),
            fake_commit = fake_commit,
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let commands = fs::read_to_string(log_path).unwrap();
    assert!(commands.contains("git clone --quiet https://github.com/bpcakes/jig-sh.git"));
    assert!(commands.contains(&format!(
        "git checkout --quiet v{}",
        env!("CARGO_PKG_VERSION")
    )));

    let answers = read_answers_toml(&repo.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(OFFICIAL_TEMPLATE_SOURCE)
    );
    assert_eq!(
        answers.get("_commit").and_then(TomlValue::as_str),
        Some(fake_commit)
    );
}

#[test]
fn omitted_template_preserves_explicit_vcs_ref() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request(None, &vcs_ref).unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("main"));
    assert!(request.used_default);
}

#[test]
fn explicit_official_template_url_still_uses_release_pin() {
    let template = Some(OFFICIAL_TEMPLATE_SOURCE.to_string());
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        template.as_deref(),
        &no_ref,
        BuildTemplatePinPolicy::Released,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(
        request.vcs_ref.as_deref(),
        Some(official_template_ref().as_str())
    );
    assert!(request.used_default);

    assert!(is_official_template_source(
        "https://github.com/bpcakes/jig-sh"
    ));
    assert!(!is_official_template_source(
        "https://github.com/bpcakes/jig-sh.git.git"
    ));
}

#[test]
fn unreleased_build_rejects_implicit_official_release_pin() {
    let no_ref = None;
    let error = resolve_initial_template_request_with_policy(
        None,
        &no_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("unreleased or dirty local source"));
    assert!(error.contains(&official_template_ref()));
    assert!(error.contains("--template /path/to/jig-sh --template-mode committed"));
    assert!(error.contains("--vcs-ref <ref>"));
}

#[test]
fn run_adopt_rejects_default_template_for_unreleased_build_policy() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    let error = with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_adopt(AdoptOpts {
            path: repo,
            template: None,
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some("demo".into()),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap_err()
        .to_string()
    });

    assert!(error.contains("unreleased or dirty local source"));
    assert!(error.contains(&official_template_ref()));
}

#[test]
fn unreleased_build_rejects_canonical_official_url_without_ref() {
    for template in [
        "https://github.com/bpcakes/jig-sh",
        "https://github.com/bpcakes/jig-sh.git",
    ] {
        let no_ref = None;
        let error = resolve_initial_template_request_with_policy(
            Some(template),
            &no_ref,
            BuildTemplatePinPolicy::Unreleased,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("unreleased or dirty local source"));
        assert!(error.contains(&official_template_ref()));
    }
}

#[test]
fn unreleased_build_allows_explicit_official_ref() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request_with_policy(
        None,
        &vcs_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("main"));
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_explicit_official_release_tag() {
    let vcs_ref = Some("v0.1.0".to_string());
    let request = resolve_initial_template_request_with_policy(
        None,
        &vcs_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("v0.1.0"));
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_explicit_official_ref_for_canonical_urls() {
    for template in [
        "https://github.com/bpcakes/jig-sh",
        "https://github.com/bpcakes/jig-sh.git",
    ] {
        let vcs_ref = Some("main".to_string());
        let request = resolve_initial_template_request_with_policy(
            Some(template),
            &vcs_ref,
            BuildTemplatePinPolicy::Unreleased,
        )
        .unwrap();

        assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
        assert_eq!(request.vcs_ref.as_deref(), Some("main"));
        assert!(request.used_default);
    }
}

#[test]
fn build_template_pin_policy_env_parser_handles_all_values() {
    assert_eq!(
        build_template_pin_policy_from_env(Some("released")),
        BuildTemplatePinPolicy::Released
    );
    assert_eq!(
        build_template_pin_policy_from_env(Some("unreleased")),
        BuildTemplatePinPolicy::Unreleased
    );
    assert_eq!(
        build_template_pin_policy_from_env(Some("unknown")),
        BuildTemplatePinPolicy::Unknown
    );
    assert_eq!(
        build_template_pin_policy_from_env(None),
        BuildTemplatePinPolicy::Unknown
    );
}

#[test]
fn unknown_build_uses_release_pin_for_packaged_installs() {
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        None,
        &no_ref,
        BuildTemplatePinPolicy::Unknown,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(
        request.vcs_ref.as_deref(),
        Some(official_template_ref().as_str())
    );
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_non_official_template_source() {
    let template = Some("/path/to/jig-sh".to_string());
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        template.as_deref(),
        &no_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, "/path/to/jig-sh");
    assert_eq!(request.vcs_ref.as_deref(), None);
    assert!(!request.used_default);
}

#[test]
fn omitted_template_uses_release_tag_for_package_version() {
    assert_eq!(official_template_ref_for_version("1.2.3"), "v1.2.3");
    assert_eq!(
        official_template_ref_for_version("1.2.3-rc.1"),
        "v1.2.3-rc.1"
    );
}

#[test]
fn default_template_mode_rejects_local_only_mode_before_clone() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    // The default template is remote, so this must fail before any git clone can start.
    let error = run_adopt(AdoptOpts {
        path: repo,
        template: None,
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err();

    let error_chain = format!("{:#}", error);
    assert!(error_chain.contains("--template-mode only applies to local git template paths."));
    assert!(error_chain.contains("Omit --template-mode for remote templates"));
}

#[test]
fn default_template_resolution_errors_explain_offline_and_ref_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();

    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            r#"#!/bin/sh
if [ "$1" = "clone" ]; then
  mkdir -p "$4"
  cp -R "{template}/." "$4"
  exit 0
fi
if [ "$1" = "checkout" ]; then
  echo "missing release tag" >&2
  exit 1
fi
exit 0
"#,
            template = template.path().display(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to resolve the official Jig template"));
    assert!(error.contains("requires network access"));
    assert!(error.contains("prerelease or development version"));
    assert!(error.contains("--template <local-path>"));
    assert!(error.contains("--vcs-ref <ref>"));
}

#[test]
fn default_template_clone_errors_get_official_template_context() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        r#"#!/bin/sh
if [ "$1" = "clone" ]; then
  echo "network unavailable" >&2
  exit 1
fi
exit 0
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to resolve the official Jig template"));
    assert!(error.contains(OFFICIAL_TEMPLATE_SOURCE));
    assert!(error.contains(&official_template_ref()));
    assert!(error.contains("requires network access"));
}

#[test]
fn default_template_resolution_error_for_explicit_ref_does_not_blame_release_tag() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request(None, &vcs_ref).unwrap();
    let error = default_template_failure_context(&request);

    assert!(error.contains("at main"));
    assert!(error.contains("selected ref must exist"));
    assert!(!error.contains("matching release tag"));
    assert!(!error.contains("prerelease or development version"));
}

#[test]
fn run_init_renders_empty_agent_tooling_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[agent_tooling.codex]
marketplaces = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("marketplaces = []"));
    let ctx = crate::context::RepoContext::load_from(&destination).unwrap();
    assert!(ctx.codex_marketplaces().is_empty());
}

#[test]
fn run_init_renders_empty_agent_tooling_plugin_lists_as_toml_arrays() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"
plugins = []
"#,
    )
    .unwrap();
    let destination = temp.path().join("repo");

    run_init(InitOpts {
        path: destination.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        answers: AnswerOpts {
            answers_file: Some(answers_file),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let rendered = fs::read_to_string(destination.join(".jig.toml")).unwrap();
    assert!(rendered.contains("plugins = []"));
    let ctx = crate::context::RepoContext::load_from(&destination).unwrap();
    assert_eq!(ctx.codex_marketplaces().len(), 1);
    assert!(ctx.codex_marketplaces()[0].plugins.is_empty());
}

#[test]
fn run_init_falls_back_only_for_unsupported_git_branch_flag() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'error: unknown switch `b`\\n' >&2\n  exit 129\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            default_branch: Some("trunk".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b trunk"));
    assert!(log.contains("git init"));
    assert!(log.contains("git symbolic-ref HEAD refs/heads/trunk"));
}

#[test]
fn run_init_surfaces_git_branch_init_failures() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let log_path = temp.path().join("commands.log");
    let git_path = bin_dir.join("git-stub.sh");
    fs::write(
            &git_path,
            format!(
                "#!/bin/sh\nprintf 'git %s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"init\" ] && [ \"$2\" = \"-b\" ]; then\n  printf 'fatal: repository storage is broken\\n' >&2\n  exit 1\nfi\nexit 0\n",
                log_path.display()
            ),
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let template = materialize_template_worktree();
    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: false,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("git init -b main failed"));
    assert!(error.contains("repository storage is broken"));
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b main"));
    assert!(!log.contains("git symbolic-ref HEAD refs/heads/main"));
}

#[test]
fn adopt_with_real_template_runs_destination_tasks() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
    assert!(!repo.join("scripts/add-migration.sh").exists());
    assert!(
        !repo
            .join("scripts/check-migration-immutability.sh")
            .exists()
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = false"));
}

#[test]
fn adopt_skips_makefile_by_default_when_destination_already_has_one() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(repo.join("Makefile"), "project-owned:\n\t@true\n").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("Makefile")).unwrap(),
        "project-owned:\n\t@true\n"
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("makefile_enabled = false"));
    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(contract.contains(r#""contract_version": 2"#));
    assert!(contract.contains(r#""kind": "command""#));
    assert!(!contract.contains("jig.run_target"));
}

#[test]
fn adopt_can_be_told_to_manage_makefile_and_reports_conflict() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(repo.join("Makefile"), "project-owned:\n\t@true\n").unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            makefile_enabled: Some(true),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Adopt would overwrite template-managed paths"));
    assert!(error.contains("Makefile"));
}

#[test]
fn adopt_appends_jig_block_to_existing_root_agents() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.starts_with("# Existing Agent Guide"));
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
    assert!(root_guide.contains("Use `scripts/jig` for the typed repo contract"));
    assert_eq!(
        root_guide
            .matches("<!-- BEGIN JIG MANAGED BLOCK -->")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn adopt_refuses_to_replace_symlinked_root_agents_without_force() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.shared.md"),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n",
    )
    .unwrap();
    create_symlink(Path::new("AGENTS.shared.md"), &repo.join("AGENTS.md")).unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Adopt would overwrite template-managed paths"));
    assert!(error.contains("AGENTS.md"));
    assert!(
        fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(repo.join("AGENTS.shared.md")).unwrap(),
        "# Existing Agent Guide\n\nKeep this repo-specific guidance.\n"
    );

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: true,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(
        !fs::symlink_metadata(repo.join("AGENTS.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(root_guide.contains("Keep this repo-specific guidance."));
    assert!(root_guide.contains("<!-- BEGIN JIG MANAGED BLOCK -->"));
}

#[test]
fn adopt_rejects_malformed_existing_root_agents_jig_block() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);
    fs::write(
        repo.join("AGENTS.md"),
        "# Existing Agent Guide\n\n<!-- BEGIN JIG MANAGED BLOCK -->\nmissing end\n",
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Malformed Jig managed block"));
}

#[test]
fn adopt_with_real_template_keeps_sqlx_files_when_enabled() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(true),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            migration_add_command: Some("scripts/add-migration.sh".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let agent_map = fs::read_to_string(repo.join("agent-map.md")).unwrap();
    assert!(agent_map.contains("[crates/api](./crates/api/AGENTS.md)"));
    assert!(repo.join("scripts/add-migration.sh").exists());
    assert!(
        repo.join("scripts/check-migration-immutability.sh")
            .exists()
    );
    assert!(
        repo.join("scripts/check-sqlx-unchecked-non-test.sh")
            .exists()
    );
    assert!(
        repo.join("scripts/generate-sqlx-unchecked-queries-todo.sh")
            .exists()
    );
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("sqlx_enabled = true"));
}

#[test]
fn adopt_with_sqlx_and_schema_dumps_disabled_hides_schema_dump_target() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(true),
            schema_dump_enabled: Some(false),
            rust_migration_dir: Some("migrations".into()),
            rust_sqlx_metadata_dir: Some(".sqlx".into()),
            migration_add_command: Some("scripts/add-migration.sh".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    let makefile = fs::read_to_string(repo.join("Makefile")).unwrap();
    assert!(!makefile.contains("schema-dump: ##"));
    assert!(!makefile.contains(" schema-dump "));

    let contract = fs::read_to_string(repo.join(".agent/jig-contract.json")).unwrap();
    assert!(!contract.contains("\"schema-dump\""));
    assert!(!contract.contains("jig.schema_dump"));
    assert!(!contract.contains("\"schema_check_command\""));
    assert!(!contract.contains("jig.schema_check"));

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(!answers.contains("schema_dump_command"));
    assert!(!answers.contains("schema_check_command"));
    assert!(!answers.contains("tool = \"jig.schema_check\""));
}
