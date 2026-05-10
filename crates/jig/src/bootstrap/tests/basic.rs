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
    let yaml = seed_answers_yaml(
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

    let mapping = yaml.as_mapping().unwrap();
    assert_eq!(
        mapping.get(YamlValue::String("repo_name".into())).unwrap(),
        &YamlValue::String("demo".into())
    );
    assert_eq!(
        mapping
            .get(YamlValue::String("sqlx_enabled".into()))
            .unwrap(),
        &YamlValue::Bool(false)
    );
    assert!(mapping.contains_key(YamlValue::String("rust_crate_roots".into())));
    assert!(!mapping.contains_key(YamlValue::String("default_branch".into())));
}

fn write_answers_fixture(dir: &Path, sqlx_enabled: Option<bool>) {
    let mut body = String::from("default_branch: main\n");
    if let Some(sqlx_enabled) = sqlx_enabled {
        body.push_str(&format!(
            "sqlx_enabled: {}\n",
            if sqlx_enabled { "true" } else { "false" }
        ));
    }
    fs::write(dir.join(".jig.yml"), body).unwrap();
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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
        &rendered.path().join(".jig.yml"),
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

    unsafe {
        env::set_var(GIT_BIN_ENV, &git_path);
    }

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination.clone(),
        template: template.path().display().to_string(),
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

    unsafe {
        env::remove_var(GIT_BIN_ENV);
    }

    assert_eq!(output["git_initialized"], true);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("git init -b main"));
    assert!(destination.exists());
    assert!(destination.join(".jig.yml").exists());
    assert!(destination.join("scripts/jig").exists());
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

    unsafe {
        env::set_var(GIT_BIN_ENV, &git_path);
    }

    let template = materialize_template_worktree();
    let destination = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: destination,
        template: template.path().display().to_string(),
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

    unsafe {
        env::remove_var(GIT_BIN_ENV);
    }

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

    unsafe {
        env::set_var(GIT_BIN_ENV, &git_path);
    }

    let template = materialize_template_worktree();
    let error = run_init(InitOpts {
        path: temp.path().join("repo"),
        template: template.path().display().to_string(),
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

    unsafe {
        env::remove_var(GIT_BIN_ENV);
    }

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
        template: template.path().display().to_string(),
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
    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("sqlx_enabled: false"));
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
        template: template.path().display().to_string(),
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
        template: template.path().display().to_string(),
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
        template: template.path().display().to_string(),
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
        template: template.path().display().to_string(),
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
        template: template.path().display().to_string(),
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
    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("sqlx_enabled: true"));
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
        template: template.path().display().to_string(),
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
}
