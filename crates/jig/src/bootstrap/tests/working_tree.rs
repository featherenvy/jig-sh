use super::*;

#[test]
fn adopt_requires_template_mode_for_local_git_templates() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: template.path().display().to_string(),
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

    assert!(error.contains("require --template-mode"));
}

#[test]
fn adopt_committed_mode_rejects_dirty_local_template() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

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

    assert!(error.contains("clean git working tree"));
}

#[test]
fn adopt_working_tree_mode_renders_uncommitted_template_changes() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Working Tree Marker\n",
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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
    assert!(root_guide.contains("Working Tree Marker"));
    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("_template_mode: working-tree"));
    assert!(answers.contains("_template_local_path:"));
    assert!(repo.join(".agent/.cache/template-source/.git").exists());
}

#[test]
fn update_working_tree_mode_refreshes_template_snapshot() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Updated Working Tree Marker\n",
    )
    .unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Updated Working Tree Marker"));
    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("_template_mode: working-tree"));
}

#[test]
fn update_working_tree_mode_rejects_vcs_ref_override() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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

    let error = run_update(UpdateOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
        template_mode: None,
        recopy: false,
        vcs_ref: Some("HEAD".into()),
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("--vcs-ref is not supported with --template-mode working-tree"));
}

#[test]
fn update_working_tree_mode_rejects_switching_local_template_checkout() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    let other_template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    run_adopt(AdoptOpts {
        path: repo,
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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

    let error = run_update(UpdateOpts {
        path: temp.path().join("repo"),
        template: Some(other_template.path().display().to_string()),
        template_mode: Some(TemplateMode::WorkingTree),
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot switch template source paths in-place"));
}

#[test]
fn update_can_relink_working_tree_repo_to_committed_template_mode() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Working Tree Marker\n",
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();
    fs::write(repo.join("AGENTS.md"), "# Repo Marker\n").unwrap();
    git(&repo, ["add", "AGENTS.md"]).unwrap();
    git(&repo, ["commit", "-m", "repo change"]).unwrap();

    git(
        template.path(),
        ["checkout", "--", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(!root_guide.contains("Working Tree Marker"));
    assert!(root_guide.contains("Repo Marker"));

    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("_template_mode: committed"));
    assert!(answers.contains(&template.path().display().to_string()));
    assert!(!answers.contains(TEMPLATE_CACHE_RELATIVE_PATH));
}

#[test]
fn update_relink_to_committed_mode_honors_requested_vcs_ref() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Older Marker\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "older template"]).unwrap();
    let old_ref = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();

    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Newer Marker\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "newer template"]).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        vcs_ref: Some(old_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Older Marker"));
    assert!(!root_guide.contains("Newer Marker"));

    let answers_path = repo.join(".jig.yml");
    assert_eq!(
        read_optional_answer_string(&answers_path, "_commit")
            .unwrap()
            .as_deref(),
        Some(old_ref.as_str())
    );
}

#[test]
fn update_can_relink_working_tree_repo_to_committed_mode_without_template_override() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();
    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Working Tree Marker\n",
    )
    .unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: template.path().display().to_string(),
        template_mode: Some(TemplateMode::WorkingTree),
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
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();
    fs::write(repo.join("AGENTS.md"), "# Repo Marker\n").unwrap();
    git(&repo, ["add", "AGENTS.md"]).unwrap();
    git(&repo, ["commit", "-m", "repo change"]).unwrap();

    git(
        template.path(),
        ["checkout", "--", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(!root_guide.contains("Working Tree Marker"));
    assert!(root_guide.contains("Repo Marker"));

    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("_template_mode: committed"));
    assert!(
        answers.contains(
            &absolute_path(template.path())
                .unwrap()
                .display()
                .to_string()
        )
    );
    assert!(!answers.contains(TEMPLATE_CACHE_RELATIVE_PATH));
}

#[test]
fn update_committed_mode_with_vcs_ref_only_updates_metadata() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::create_dir_all(repo.join("crates/api")).unwrap();
    fs::write(repo.join("crates/api/AGENTS.md"), "crate guide").unwrap();

    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Older Marker\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "older template"]).unwrap();

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
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    fs::write(
        template.path().join("templates/project/AGENTS.md.jinja"),
        "# Newer Marker\n",
    )
    .unwrap();
    git(
        template.path(),
        ["add", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "newer template"]).unwrap();
    let new_ref = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        vcs_ref: Some(new_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Newer Marker"));
    assert!(!root_guide.contains("Older Marker"));

    let answers_path = repo.join(".jig.yml");
    assert_eq!(
        read_optional_answer_string(&answers_path, "_commit")
            .unwrap()
            .as_deref(),
        Some(new_ref.as_str())
    );
}
