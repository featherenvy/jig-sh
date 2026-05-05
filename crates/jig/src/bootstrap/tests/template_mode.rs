use super::*;

#[test]
fn adopt_local_git_template_defaults_to_committed_mode() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    run_adopt(AdoptOpts {
        path: repo.clone(),
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
    .unwrap();

    let answers = fs::read_to_string(repo.join(".jig.yml")).unwrap();
    assert!(answers.contains("_template_mode: committed"));
    assert!(answers.contains("_template_local_path:"));
    assert!(
        answers.contains(
            &absolute_path(template.path())
                .unwrap()
                .display()
                .to_string()
        )
    );
}

#[test]
fn adopt_local_git_template_rejects_dirty_committed_source() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();
    write_test_crate_guide(&repo);

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

    assert!(error.contains("clean git working tree"));
    assert!(error.contains("Commit or stash template changes"));
}

#[test]
fn update_rejects_legacy_working_tree_template_state() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    let answers_path = repo.join(".jig.yml");
    let mut answers = read_answers_yaml(&answers_path).unwrap();
    answers.insert(
        YamlValue::String(TEMPLATE_MODE_KEY.into()),
        YamlValue::String("working-tree".into()),
    );
    write_answers_yaml(&answers_path, &answers).unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unsupported legacy template mode 'working-tree'"));
    assert!(error.contains("committed template source"));
}

#[test]
fn update_committed_mode_rejects_switching_local_template_checkout() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    let other_template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let error = run_update(UpdateOpts {
        path: repo,
        template: Some(other_template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
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
fn update_default_committed_mode_uses_clean_local_template_head() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    commit_template_root_guide(
        template.path(),
        "# Default Update Marker\n",
        "template update",
    );

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
    assert!(root_guide.contains("Default Update Marker"));
}

#[test]
fn update_default_committed_mode_rejects_dirty_local_template() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    fs::write(template.path().join("DIRTY.txt"), "dirty").unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("clean git working tree"));
}

#[test]
fn update_committed_mode_with_vcs_ref_only_updates_metadata() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    commit_template_root_guide(template.path(), "# Older Marker\n", "older template");

    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);
    init_git_repo_for_test(&repo);
    git(&repo, ["add", "."]).unwrap();
    git(&repo, ["commit", "-m", "adopt"]).unwrap();

    let new_ref = commit_template_root_guide(template.path(), "# Newer Marker\n", "newer template");

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
