use super::*;

#[test]
fn init_rejects_unsafe_frontend_app_values() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let bad_name = run_init(InitOpts {
        path: temp.path().join("bad-name"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web;rm".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(bad_name.contains("Invalid frontend app name"));

    let bad_dir = run_init(InitOpts {
        path: temp.path().join("bad-dir"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "../web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(bad_dir.contains("must not contain '..'"));

    let dot_dir = run_init(InitOpts {
        path: temp.path().join("dot-dir"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/./web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(dot_dir.contains("must not contain '.' path components"));

    let empty_segment_dir = run_init(InitOpts {
        path: temp.path().join("empty-segment-dir"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps//web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(empty_segment_dir.contains("must not contain empty path components"));

    let absolute_dir = run_init(InitOpts {
        path: temp.path().join("absolute-dir"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "/tmp/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(absolute_dir.contains("must be relative"));

    let unsupported_dir = run_init(InitOpts {
        path: temp.path().join("unsupported-dir"),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web:dev".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();
    assert!(unsupported_dir.contains("contains unsupported characters"));
}

#[test]
fn init_reports_and_preserves_legacy_dev_command_answer() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    let repo = temp.path().join("repo");
    let output = run_init(InitOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            dev_command: Some("npm run dev".into()),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["notes"].as_array().unwrap().iter().any(|note| {
        note.as_str()
            .unwrap()
            .contains("Preserved deprecated dev_command")
    }));
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("dev_command = \"npm run dev\""));
    assert!(answers.contains("Deprecated and ignored by generated commands"));
}

#[test]
fn adopt_accepts_npm_frontend_app_and_renders_current_web_and_dev_config() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api/src")).unwrap();
    fs::write(
        repo.join("crates/api/Cargo.toml"),
        "[package]\nname = \"api\"\n",
    )
    .unwrap();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(repo.join("package.json"), r#"{"private":true}"#).unwrap();
    fs::write(repo.join("package-lock.json"), "{}").unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage",
    "dev": "vite"
  }
}
"#,
    )
    .unwrap();

    let output = run_adopt(AdoptOpts {
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
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    assert!(output["next_steps"].as_array().unwrap().iter().any(|step| {
        step.as_str()
            .unwrap()
            .contains("scripts/jig check agent-guides")
    }));
    assert!(repo.join("crates/api/AGENTS.md").exists());
    let crate_guide = fs::read_to_string(repo.join("crates/api/AGENTS.md")).unwrap();
    assert!(crate_guide.contains("## Purpose"));
    assert!(crate_guide.contains("`src/lib.rs`"));

    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(answers.contains("web_package_manager = \"npm\""));
    assert!(answers.contains("[[frontend_apps]]"));
    assert!(answers.contains("[[dev.apps]]"));
    assert!(answers.contains("argv = [\"npm\", \"run\", \"dev\"]"));
    assert!(!answers.contains("dev_command"));

    let makefile = fs::read_to_string(repo.join("Makefile")).unwrap();
    assert!(makefile.contains("WEB_INSTALL_COMMAND ?= npm ci"));
    assert!(makefile.contains("if [ -f package.json ] && [ -f package-lock.json ]"));
    assert!(
        makefile.contains(
            "$(NODE) scripts/check-webapp-scripts.mjs apps/web lint typecheck build:bundle test:coverage"
        )
    );
    assert!(makefile.contains("scripts/jig dev"));
    assert!(repo.join("scripts/check-webapp-scripts.mjs").is_file());
    let script_helper = fs::read_to_string(repo.join("scripts/check-webapp-scripts.mjs")).unwrap();
    assert!(script_helper.contains("typeof command !== \"string\""));
    assert!(script_helper.contains("command.trim().length === 0"));

    let web_workflow =
        fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
    assert!(web_workflow.contains("actions/setup-node@v5"));
    assert!(web_workflow.contains("cache: npm"));
    assert!(web_workflow.contains("npm ci"));
    assert!(web_workflow.contains("node scripts/check-webapp-scripts.mjs"));
    assert!(!web_workflow.contains("oven-sh/setup-bun"));

    let rust_workflow = fs::read_to_string(repo.join(".github/workflows/rust-tests.yml")).unwrap();
    assert!(rust_workflow.contains("scripts/jig check fmt"));
    assert!(!rust_workflow.contains("scripts/jig fmt-check"));

    let agent_map_workflow =
        fs::read_to_string(repo.join(".github/workflows/agent-map-check.yml")).unwrap();
    assert!(agent_map_workflow.contains("scripts/jig check agent-map"));
    assert!(!agent_map_workflow.contains("scripts/jig agent-map check"));
}

#[test]
fn init_renders_web_commands_for_all_supported_package_managers() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let cases = [
        ("bun", "bun install --frozen-lockfile", "bun run"),
        ("npm", "npm ci", "npm run"),
        ("pnpm", "pnpm install --frozen-lockfile", "pnpm run"),
        ("yarn", "yarn install --frozen-lockfile", "yarn run"),
    ];

    for (package_manager, install_command, run_command) in cases {
        let repo = temp.path().join(package_manager);
        run_init(InitOpts {
            path: repo.clone(),
            template: Some(template.path().display().to_string()),
            template_mode: None,
            vcs_ref: None,
            force: false,
            defaults: true,
            no_input: true,
            answers: AnswerOpts {
                repo_name: Some(format!("demo-{package_manager}")),
                sqlx_enabled: Some(false),
                web_package_manager: Some(package_manager.into()),
                frontend_apps: vec![FrontendApp {
                    name: "web".into(),
                    dir: "apps/web".into(),
                    coverage_threshold: 80,
                    kind: "vite".into(),
                }],
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let makefile = fs::read_to_string(repo.join("Makefile")).unwrap();
        assert!(
            makefile.contains(&format!("WEB_INSTALL_COMMAND ?= {install_command}")),
            "missing install command for {package_manager}"
        );
        assert!(
            makefile.contains(&format!("WEB_RUN_COMMAND ?= {run_command}")),
            "missing run command for {package_manager}"
        );
        let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
        assert!(
            answers.contains(&format!("argv = [\"{package_manager}\", \"run\", \"dev\"]")),
            "missing dev app argv for {package_manager}"
        );

        let workflow =
            fs::read_to_string(repo.join(".github/workflows/webapp-checks.yml")).unwrap();
        if package_manager == "bun" {
            assert!(workflow.contains("oven-sh/setup-bun@v2"));
            assert!(workflow.contains("node-version: 22"));
        } else {
            assert!(workflow.contains(&format!("cache: {package_manager}")));
        }
        if matches!(package_manager, "pnpm" | "yarn") {
            assert!(workflow.contains("corepack enable"));
            let corepack = workflow.find("corepack enable").unwrap();
            let cache = workflow.find(&format!("cache: {package_manager}")).unwrap();
            assert!(
                corepack < cache,
                "corepack must be enabled before {package_manager} cache setup"
            );
        }
    }
}

#[test]
fn adopt_rejects_frontend_app_missing_required_ci_scripts() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": null,
    "typecheck": "tsc --noEmit"
  }
}
"#,
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
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("missing package.json scripts required by generated web CI"));
    assert!(error.contains("lint, build:bundle, test:coverage"));
    assert!(error.contains("remove the entry from frontend_apps"));
}

#[test]
fn adopt_rejects_frontend_app_without_repo_or_app_lockfile() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("apps/web")).unwrap();
    fs::write(
        repo.join("apps/web/package.json"),
        r#"{
  "name": "web",
  "scripts": {
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}
"#,
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
            web_package_manager: Some("npm".into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("does not have a lockfile for npm"));
    assert!(error.contains("repo root or app directory"));
    assert!(error.contains("remove the entry from frontend_apps"));
}
