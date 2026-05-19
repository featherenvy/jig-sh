use std::path::Path;

use serde_json::{Value as JsonValue, json};

use super::answers::AnswerInputShape;
use super::{AnswerOpts, FrontendApp};

mod frontend;
mod github;
mod package_manager;
mod repo;
mod rust_sqlx;
mod scan;

use self::frontend::infer_frontend_apps;
use self::github::infer_ci_github_runner;
use self::package_manager::infer_package_manager;
use self::repo::{infer_default_branch, infer_repo_name};
use self::rust_sqlx::{infer_rust_crate_roots, infer_sqlx};
use self::scan::{RepoScan, push_scan_warning};

#[cfg(test)]
use self::frontend::segment_matches;
#[cfg(test)]
use self::github::select_github_runner;
#[cfg(test)]
use self::repo::{repo_name_from_remote_url, safe_repo_name};
#[cfg(test)]
use self::rust_sqlx::crate_root_from_workspace_member;
#[cfg(test)]
use self::scan::MAX_SCAN_FILE_BYTES;

#[derive(Clone, Debug, Default)]
pub(super) struct AdoptInference {
    repo_name: Option<String>,
    default_branch: Option<String>,
    rust_crate_roots: Vec<String>,
    sqlx_enabled: Option<bool>,
    rust_migration_dir: Option<String>,
    rust_migration_dirs: Vec<String>,
    rust_sqlx_metadata_dir: Option<String>,
    sqlx_check_command: Option<String>,
    web_package_manager: Option<String>,
    frontend_apps: Vec<FrontendApp>,
    ci_github_runner: Option<String>,
    signals: Vec<String>,
    warnings: Vec<String>,
}

pub(super) fn infer_adopt_answers(root: &Path) -> AdoptInference {
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(root, &mut warnings);
    let repo_name = infer_repo_name(root);
    let default_branch = infer_default_branch(root, &mut warnings);
    let mut inference = AdoptInference {
        repo_name: repo_name.clone(),
        default_branch,
        rust_crate_roots: infer_rust_crate_roots(root, &mut warnings),
        web_package_manager: infer_package_manager(root, &scan, &mut warnings),
        frontend_apps: infer_frontend_apps(root, repo_name.as_deref(), &mut warnings),
        ci_github_runner: infer_ci_github_runner(root, &scan, &mut warnings),
        warnings,
        ..AdoptInference::default()
    };

    let sqlx = infer_sqlx(root, &scan, &mut inference.warnings);
    inference.sqlx_enabled = Some(sqlx.enabled);
    inference.rust_migration_dir = sqlx.migration_dir;
    inference.rust_migration_dirs = sqlx.migration_dirs;
    inference.rust_sqlx_metadata_dir = sqlx.metadata_dir;
    inference.sqlx_check_command = sqlx.check_command;
    inference.signals.extend(sqlx.signals);
    if inference.sqlx_enabled == Some(true)
        && inference
            .ci_github_runner
            .as_deref()
            .is_some_and(|runner| runner.starts_with("windows-"))
    {
        push_scan_warning(
            &mut inference.warnings,
            root,
            "SQLx check command inference uses POSIX shell syntax but the inferred GitHub runner is Windows; pass --sqlx-check-command if needed",
        );
    }

    if !inference.rust_crate_roots.is_empty() {
        inference.signals.push(format!(
            "Rust crate roots: {}",
            inference.rust_crate_roots.join(", ")
        ));
    }
    if !inference.frontend_apps.is_empty() {
        inference.signals.push(format!(
            "frontend apps: {}",
            inference
                .frontend_apps
                .iter()
                .map(|app| format!("{} at {}", app.name, app.dir))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(package_manager) = inference.web_package_manager.as_deref() {
        inference
            .signals
            .push(format!("package manager: {package_manager}"));
    }
    if let Some(runner) = inference.ci_github_runner.as_deref() {
        inference.signals.push(format!("GitHub runner: {runner}"));
    }

    inference
}

impl AdoptInference {
    pub(super) fn apply_to_answers(
        &self,
        answers: &mut AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) {
        fill_string(
            &mut answers.repo_name,
            self.repo_name.as_deref(),
            answer_shape,
            "repo_name",
        );
        fill_string(
            &mut answers.default_branch,
            self.default_branch.as_deref(),
            answer_shape,
            "default_branch",
        );
        fill_string(
            &mut answers.ci_github_runner,
            self.ci_github_runner.as_deref(),
            answer_shape,
            "ci_github_runner",
        );
        fill_string(
            &mut answers.web_package_manager,
            self.web_package_manager.as_deref(),
            answer_shape,
            "web_package_manager",
        );
        fill_vec(
            &mut answers.rust_crate_roots,
            &self.rust_crate_roots,
            answer_shape,
            "rust_crate_roots",
        );
        fill_frontend_apps(
            &mut answers.frontend_apps,
            &self.frontend_apps,
            answer_shape,
        );

        let explicit_sqlx_enabled = answer_shape.explicit_sqlx_enabled(answers);
        if answer_shape.should_apply_inferred_sqlx_enabled(answers) {
            answers.sqlx_enabled = self.sqlx_enabled;
        }
        if self.sqlx_enabled == Some(true) && explicit_sqlx_enabled != Some(false) {
            fill_string(
                &mut answers.rust_migration_dir,
                self.rust_migration_dir.as_deref(),
                answer_shape,
                "rust_migration_dir",
            );
            fill_string(
                &mut answers.rust_sqlx_metadata_dir,
                self.rust_sqlx_metadata_dir.as_deref(),
                answer_shape,
                "rust_sqlx_metadata_dir",
            );
            fill_string(
                &mut answers.sqlx_check_command,
                self.sqlx_check_command.as_deref(),
                answer_shape,
                "sqlx_check_command",
            );
        }
    }

    pub(super) fn summary(&self) -> String {
        let rust = if self.rust_crate_roots.is_empty() {
            "no Rust workspace".to_string()
        } else {
            format!("Rust workspace ({})", self.rust_crate_roots.join(", "))
        };
        let sqlx = if self.sqlx_enabled == Some(true) {
            match self.rust_migration_dir.as_deref() {
                Some(dir) => format!("SQLx migrations at {dir}"),
                None => "SQLx".into(),
            }
        } else {
            "no SQLx".into()
        };
        let frontend = match self.frontend_apps.as_slice() {
            [] => "no frontend apps".to_string(),
            [app] => format!("one {} app at {}", app.kind, app.dir),
            apps => format!("{} frontend apps", apps.len()),
        };
        let package_manager = self
            .web_package_manager
            .as_deref()
            .map(|value| format!("{value} lockfile"))
            .unwrap_or_else(|| "no web lockfile".into());
        format!("{rust}, {sqlx}, {frontend}, {package_manager}")
    }

    pub(super) fn report(&self) -> JsonValue {
        json!({
            "summary": self.summary(),
            "scope": "inferred values before CLI and answers-file precedence is applied",
            "repo_name": self.repo_name,
            "default_branch": self.default_branch,
            "rust_crate_roots": self.rust_crate_roots,
            "sqlx_enabled": self.sqlx_enabled,
            "rust_migration_dir": self.rust_migration_dir,
            "rust_migration_dirs": self.rust_migration_dirs,
            "rust_sqlx_metadata_dir": self.rust_sqlx_metadata_dir,
            "web_package_manager": self.web_package_manager,
            "frontend_apps": self.frontend_apps,
            "ci_github_runner": self.ci_github_runner,
            "signals": self.signals,
            "warnings": self.warnings,
        })
    }

    pub(super) fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

fn fill_string(
    target: &mut Option<String>,
    value: Option<&str>,
    answer_shape: &AnswerInputShape,
    key: &str,
) {
    if target.is_none() && !answer_shape.contains_key(key) {
        *target = value.map(str::to_string);
    }
}

fn fill_vec(
    target: &mut Vec<String>,
    value: &[String],
    answer_shape: &AnswerInputShape,
    key: &str,
) {
    if target.is_empty() && !value.is_empty() && !answer_shape.contains_key(key) {
        target.extend(value.iter().cloned());
    }
}

fn fill_frontend_apps(
    target: &mut Vec<FrontendApp>,
    value: &[FrontendApp],
    answer_shape: &AnswerInputShape,
) {
    if target.is_empty() && !value.is_empty() && !answer_shape.contains_key("frontend_apps") {
        target.extend(value.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use super::super::git::git;
    use super::*;
    use crate::bootstrap::adopt_infer::scan::MAX_SCAN_WARNINGS;

    fn infer_sqlx(root: &Path, warnings: &mut Vec<String>) -> super::rust_sqlx::SqlxInference {
        let scan = RepoScan::collect(root, warnings);
        super::rust_sqlx::infer_sqlx(root, &scan, warnings)
    }

    fn infer_package_manager(root: &Path, warnings: &mut Vec<String>) -> Option<String> {
        let scan = RepoScan::collect(root, warnings);
        super::package_manager::infer_package_manager(root, &scan, warnings)
    }

    #[test]
    fn parses_remote_repo_names() {
        assert_eq!(
            repo_name_from_remote_url("git@github.com:owner/demo.git").as_deref(),
            Some("demo")
        );
        assert_eq!(
            repo_name_from_remote_url("https://github.com/owner/demo").as_deref(),
            Some("demo")
        );
        assert_eq!(
            repo_name_from_remote_url("ssh://git@example.com:2222/owner/demo.git").as_deref(),
            Some("demo")
        );
        assert_eq!(
            repo_name_from_remote_url("git@github.com:owner/my.app.git").as_deref(),
            Some("my.app")
        );
    }

    #[test]
    fn remote_repo_name_preserves_dots() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), ["init"]).unwrap();
        git(
            temp.path(),
            ["remote", "add", "origin", "git@github.com:owner/my.app.git"],
        )
        .unwrap();

        assert_eq!(infer_repo_name(temp.path()).as_deref(), Some("my.app"));
    }

    #[test]
    fn fallback_repo_name_is_sanitized() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("Demo App.v2");
        fs::create_dir(&repo).unwrap();

        assert_eq!(infer_repo_name(&repo).as_deref(), Some("Demo-App-v2"));
        assert_eq!(safe_repo_name("@@@"), "repo");
    }

    #[test]
    fn inferred_sqlx_enabled_predicate_respects_explicit_shapes() {
        let mut answers = AnswerOpts::default();
        let empty_shape = AnswerInputShape::default();
        assert!(empty_shape.should_apply_inferred_sqlx_enabled(&answers));

        answers.rust_migration_dir = Some("migrations".into());
        assert!(!empty_shape.should_apply_inferred_sqlx_enabled(&answers));
        answers.rust_migration_dir = None;

        let shape = answer_shape_from_keys(["sqlx_check_command"]);
        assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
        let shape = answer_shape_from_keys(["schema_dump_command"]);
        assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
        answers.migration_add_command = Some("scripts/new-migration.sh".into());
        assert!(!empty_shape.should_apply_inferred_sqlx_enabled(&answers));
        answers.migration_add_command = None;

        let shape = answer_shape_from_key_values([("schema_dump_enabled", true)]);
        assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
        let shape = answer_shape_from_key_values([("schema_dump_enabled", false)]);
        assert!(shape.should_apply_inferred_sqlx_enabled(&answers));
    }

    fn answer_shape_from_keys(keys: impl IntoIterator<Item = &'static str>) -> AnswerInputShape {
        let table = keys
            .into_iter()
            .map(|key| (key.to_string(), toml::Value::String(String::new())))
            .collect();
        AnswerInputShape::from_table(&table)
    }

    fn answer_shape_from_key_values(
        pairs: impl IntoIterator<Item = (&'static str, bool)>,
    ) -> AnswerInputShape {
        let table = pairs
            .into_iter()
            .map(|(key, value)| (key.to_string(), toml::Value::Boolean(value)))
            .collect();
        AnswerInputShape::from_table(&table)
    }

    #[test]
    fn scan_warnings_are_capped_with_omission_notice() {
        let temp = tempfile::tempdir().unwrap();
        let mut warnings = Vec::new();

        for _ in 0..(MAX_SCAN_WARNINGS + 5) {
            push_scan_warning(&mut warnings, temp.path(), "synthetic warning");
        }

        assert_eq!(warnings.len(), MAX_SCAN_WARNINGS);
        assert_eq!(
            warnings.last().map(String::as_str),
            Some("additional inference scan warnings omitted")
        );
    }

    #[test]
    fn crate_roots_follow_workspace_member_parents() {
        assert_eq!(
            crate_root_from_workspace_member("crates/*").as_deref(),
            Some("crates")
        );
        assert_eq!(
            crate_root_from_workspace_member("apps/api").as_deref(),
            Some("apps")
        );
        assert_eq!(crate_root_from_workspace_member(".").as_deref(), Some("."));
    }

    #[test]
    fn single_crate_root_is_inferred_as_repo_root() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
        )
        .unwrap();
        let mut warnings = Vec::new();

        assert_eq!(
            infer_rust_crate_roots(temp.path(), &mut warnings),
            vec!["."]
        );
    }

    #[test]
    fn workspace_glob_segment_match_supports_multiple_stars() {
        assert!(segment_matches("*-app-*", "demo-app-web"));
        assert!(segment_matches("app-*-web", "app-demo-web"));
        assert!(!segment_matches("app-*-web", "app-demo-api"));
    }

    #[test]
    fn sqlx_detection_includes_cargo_sqlx_commands() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/sqlx.yml"),
            "steps:\n  - run: cargo sqlx prepare --check\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);

        assert!(sqlx.enabled);
        assert_eq!(sqlx.migration_dir.as_deref(), Some("migrations"));
        assert_eq!(
            sqlx.check_command.as_deref(),
            Some(
                "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check -- --all-targets"
            )
        );
        assert!(
            sqlx.signals
                .iter()
                .any(|signal| signal == "cargo sqlx command")
        );
    }

    #[test]
    fn sqlx_check_command_uses_workspace_flag_for_cargo_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
sqlx = "0.8"
"#,
        )
        .unwrap();

        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);

        assert_eq!(
            sqlx.check_command.as_deref(),
            Some(
                "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check --workspace -- --all-targets"
            )
        );
    }

    #[test]
    fn sqlx_detection_ignores_benign_cargo_sqlx_mentions() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "/// Example: sqlx::migrate!();\n// sqlx::migrate!();\n/* sqlx::migrate!(); */\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("notes.toml"),
            "# run cargo sqlx prepare manually if needed\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("script.sh"),
            "# cargo sqlx prepare --check\nnpm test # cargo sqlx prepare --check\n",
        )
        .unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "steps:\n  - run: npm test # cargo sqlx prepare --check\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);

        assert!(!sqlx.enabled);
        assert!(
            sqlx.signals
                .iter()
                .any(|signal| { signal.contains("no SQLx signals detected") })
        );
    }

    #[test]
    fn root_named_like_skipped_dir_is_still_scanned() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("target");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(&root);

        assert_eq!(inference.rust_crate_roots, vec!["."]);
    }

    #[test]
    fn nested_package_manager_conflicts_are_reported_as_warnings() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        fs::create_dir_all(temp.path().join("packages/api")).unwrap();
        fs::write(temp.path().join("apps/web/package-lock.json"), "{}").unwrap();
        fs::write(temp.path().join("packages/api/pnpm-lock.yaml"), "").unwrap();

        let mut warnings = Vec::new();
        let manager = infer_package_manager(temp.path(), &mut warnings);

        assert_eq!(manager.as_deref(), Some("npm"));
        assert!(
            warnings
                .iter()
                .any(|warning| { warning.contains("multiple package manager lockfiles detected") })
        );
    }

    #[test]
    fn root_package_manager_conflicts_are_reported_as_warnings() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("bun.lock"), "").unwrap();
        fs::write(temp.path().join("package-lock.json"), "{}").unwrap();

        let mut warnings = Vec::new();
        let manager = infer_package_manager(temp.path(), &mut warnings);

        assert_eq!(manager.as_deref(), Some("bun"));
        assert!(warnings.iter().any(|warning| {
            warning.contains("multiple root package manager lockfiles detected")
        }));
    }

    #[test]
    fn default_branch_prefers_known_origin_refs_over_current_branch() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        let global_config = temp.path().join("global-gitconfig");
        fs::write(&global_config, "").unwrap();
        let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
        let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
        git(temp.path(), ["init", "-b", "feature"]).unwrap();
        git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
        git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
        fs::write(temp.path().join("README.md"), "demo\n").unwrap();
        git(temp.path(), ["add", "README.md"]).unwrap();
        git(temp.path(), ["commit", "-m", "init"]).unwrap();
        git(
            temp.path(),
            ["update-ref", "refs/remotes/origin/main", "HEAD"],
        )
        .unwrap();

        let mut warnings = Vec::new();
        assert_eq!(
            infer_default_branch(temp.path(), &mut warnings).as_deref(),
            Some("main")
        );
    }

    #[test]
    fn default_branch_warns_when_multiple_origin_candidates_exist() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        let global_config = temp.path().join("global-gitconfig");
        fs::write(&global_config, "").unwrap();
        let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
        let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
        git(temp.path(), ["init", "-b", "feature"]).unwrap();
        git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
        git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
        fs::write(temp.path().join("README.md"), "demo\n").unwrap();
        git(temp.path(), ["add", "README.md"]).unwrap();
        git(temp.path(), ["commit", "-m", "init"]).unwrap();
        git(
            temp.path(),
            ["update-ref", "refs/remotes/origin/main", "HEAD"],
        )
        .unwrap();
        git(
            temp.path(),
            ["update-ref", "refs/remotes/origin/master", "HEAD"],
        )
        .unwrap();

        let mut warnings = Vec::new();
        assert_eq!(
            infer_default_branch(temp.path(), &mut warnings).as_deref(),
            Some("main")
        );
        assert!(warnings.iter().any(|warning| {
            warning.contains("multiple origin default branch candidates detected")
        }));
    }

    #[test]
    fn default_branch_does_not_infer_unknown_current_branch() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        let global_config = temp.path().join("global-gitconfig");
        fs::write(&global_config, "").unwrap();
        let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
        let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
        git(temp.path(), ["init", "-b", "feature"]).unwrap();
        git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
        git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
        fs::write(temp.path().join("README.md"), "demo\n").unwrap();
        git(temp.path(), ["add", "README.md"]).unwrap();
        git(temp.path(), ["commit", "-m", "init"]).unwrap();

        let mut warnings = Vec::new();
        assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
        assert!(warnings.iter().any(|warning| {
            warning.contains("current branch feature is not a known default branch name")
        }));
    }

    #[test]
    fn default_branch_infers_known_local_head_without_origin() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        let global_config = temp.path().join("global-gitconfig");
        fs::write(&global_config, "").unwrap();
        let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
        let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
        git(temp.path(), ["init", "-b", "main"]).unwrap();
        git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
        git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
        fs::write(temp.path().join("README.md"), "demo\n").unwrap();
        git(temp.path(), ["add", "README.md"]).unwrap();
        git(temp.path(), ["commit", "-m", "init"]).unwrap();

        let mut warnings = Vec::new();
        assert_eq!(
            infer_default_branch(temp.path(), &mut warnings).as_deref(),
            Some("main")
        );
    }

    #[test]
    fn default_branch_ignores_malformed_origin_head() {
        let _guard = crate::test_env::lock_env();
        let temp = tempfile::tempdir().unwrap();
        let global_config = temp.path().join("global-gitconfig");
        fs::write(&global_config, "").unwrap();
        let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
        let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
        git(temp.path(), ["init", "-b", "feature"]).unwrap();
        git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
        git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
        fs::write(temp.path().join("README.md"), "demo\n").unwrap();
        git(temp.path(), ["add", "README.md"]).unwrap();
        git(temp.path(), ["commit", "-m", "init"]).unwrap();
        git(
            temp.path(),
            [
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/heads/feature",
            ],
        )
        .unwrap();

        let mut warnings = Vec::new();
        assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
    }

    #[test]
    fn sqlx_detection_reports_nested_and_multiple_migration_dirs() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("crates/api/migrations/20240101_init")).unwrap();
        fs::create_dir_all(temp.path().join("services/billing/migrations")).unwrap();
        fs::write(
            temp.path()
                .join("crates/api/migrations/20240101_init/up.sql"),
            "select 1;",
        )
        .unwrap();
        fs::write(
            temp.path().join("services/billing/migrations/0001.sql"),
            "select 1;",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);

        assert!(sqlx.enabled);
        assert_eq!(
            sqlx.migration_dirs,
            vec![
                "crates/api/migrations".to_string(),
                "services/billing/migrations".to_string(),
            ]
        );
        assert_eq!(sqlx.migration_dir.as_deref(), Some("crates/api/migrations"));
        assert!(sqlx.signals.iter().any(|signal| {
            signal
                == "migration directories detected: crates/api/migrations, services/billing/migrations"
        }));
        assert!(warnings.iter().any(|warning| {
            warning.contains("multiple migration directories detected")
                && warning.contains("crates/api/migrations")
        }));
    }

    #[test]
    fn migration_dir_ignores_non_migration_sql_snippets() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("migrations/archive")).unwrap();
        fs::write(
            temp.path().join("migrations/archive/old_dump.sql"),
            "select 1;",
        )
        .unwrap();
        fs::write(temp.path().join("migrations/README.sql"), "notes").unwrap();

        let mut warnings = Vec::new();
        let sqlx = infer_sqlx(temp.path(), &mut warnings);

        assert!(!sqlx.enabled);
        assert!(sqlx.migration_dirs.is_empty());
    }

    #[test]
    fn missing_workspace_glob_is_a_warning_not_a_failure() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"workspaces":["missing/*"]}"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.frontend_apps.is_empty());
        assert!(
            inference
                .warnings
                .iter()
                .any(|warning| warning.contains("could not read directory")),
            "expected scan warning, got {:?}",
            inference.warnings
        );
    }

    #[test]
    fn empty_pnpm_workspace_is_reported_as_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("pnpm-workspace.yaml"), "packages:\n").unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("pnpm-workspace.yaml did not declare supported packages globs")
        }));
    }

    #[test]
    fn pnpm_workspace_flow_style_globs_are_supported() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
        fs::write(
            temp.path().join("pnpm-workspace.yaml"),
            "packages: [\"apps/*\"]\n",
        )
        .unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
        fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "apps/web");
    }

    #[test]
    fn frontend_packages_missing_ci_scripts_are_reported_as_warnings() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("web")).unwrap();
        fs::write(
            temp.path().join("web/package.json"),
            r#"{"scripts":{"dev":"vite","lint":"eslint ."}}"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.frontend_apps.is_empty());
        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("missing required CI scripts")
                && warning.contains("typecheck")
                && warning.contains("build:bundle")
                && warning.contains("test:coverage")
        }));
    }

    #[test]
    fn fallback_frontend_scan_ignores_non_conventional_package_dirs() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("examples/demo")).unwrap();
        fs::write(
            temp.path().join("examples/demo/package.json"),
            r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.frontend_apps.is_empty());
    }

    #[test]
    fn declared_workspaces_limit_frontend_app_candidates() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"private":true,"workspaces":["apps/*"]}"#,
        )
        .unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
        fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "apps/web");
    }

    #[test]
    fn workspace_exclusion_globs_remove_frontend_candidates() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("packages/web")).unwrap();
        fs::create_dir_all(temp.path().join("packages/private")).unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"private":true,"workspaces":["packages/*","!packages/private"]}"#,
        )
        .unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
        fs::write(
            temp.path().join("packages/private/package.json"),
            app_package,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "packages/web");
    }

    #[test]
    fn pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("packages/web")).unwrap();
        fs::create_dir_all(temp.path().join("packages/private")).unwrap();
        fs::write(
            temp.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n  - !packages/private\n",
        )
        .unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
        fs::write(
            temp.path().join("packages/private/package.json"),
            app_package,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "packages/web");
    }

    #[test]
    fn quoted_pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("packages/web")).unwrap();
        fs::create_dir_all(temp.path().join("packages/private")).unwrap();
        fs::write(
            temp.path().join("pnpm-workspace.yaml"),
            "packages: [\"packages/*\", \"!packages/private\"]\n",
        )
        .unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
        fs::write(
            temp.path().join("packages/private/package.json"),
            app_package,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "packages/web");
    }

    #[test]
    fn declared_workspaces_skip_root_frontend_app_candidate() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
        fs::write(
            temp.path().join("package.json"),
            r#"{
  "private": true,
  "workspaces": ["apps/*"],
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
        )
        .unwrap();
        fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.frontend_apps.len(), 1);
        assert_eq!(inference.frontend_apps[0].dir, "apps/web");
    }

    #[test]
    fn explicit_empty_workspaces_suppress_frontend_fallback_scan() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("apps/web")).unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"private":true,"workspaces":[]}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("apps/web/package.json"),
            r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.frontend_apps.is_empty());
    }

    #[test]
    fn sqlx_metadata_dir_alone_enables_sqlx_inference() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".sqlx")).unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.sqlx_enabled, Some(true));
        assert_eq!(inference.rust_sqlx_metadata_dir.as_deref(), Some(".sqlx"));
        assert!(
            inference
                .signals
                .iter()
                .any(|signal| signal == "SQLx metadata directory .sqlx")
        );
    }

    #[test]
    fn sqlx_detection_warns_when_default_paths_are_synthesized() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = "0.8"
"#,
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.sqlx_enabled, Some(true));
        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("SQLx was detected but migration or metadata directories were not")
        }));
    }

    #[test]
    fn oversized_cargo_toml_reports_scan_warning() {
        let temp = tempfile::tempdir().unwrap();
        let mut manifest = String::from("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
        manifest.push_str(&"#".repeat((MAX_SCAN_FILE_BYTES as usize) + 1));
        fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("could not read TOML for inference")
                && warning.contains("is larger than")
        }));
    }

    #[test]
    fn oversized_text_scan_file_reports_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("src")).unwrap();
        fs::write(
            temp.path().join("src/lib.rs"),
            "x".repeat((MAX_SCAN_FILE_BYTES as usize) + 1),
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("could not read text for inference")
                && warning.contains("is larger than")
        }));
    }

    #[test]
    fn unreadable_yaml_inference_file_reports_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "x".repeat((MAX_SCAN_FILE_BYTES as usize) + 1),
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("could not read YAML for inference")
                && warning.contains("is larger than")
        }));
    }

    #[test]
    fn malformed_package_json_reports_scan_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("package.json"), "{not json").unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("could not read JSON for inference")
                && warning.contains("Failed to parse")
        }));
    }

    #[test]
    fn github_runner_is_inferred_from_workflows() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
    }

    #[test]
    fn github_runner_strips_inline_comments() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: ubuntu-latest # primary runner\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-latest"));
    }

    #[test]
    fn github_runner_single_item_array_is_inferred() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: [ubuntu-latest]\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-latest"));
    }

    #[test]
    fn github_runner_tie_break_prefers_newer_ubuntu_label() {
        let runners = BTreeMap::from([
            ("ubuntu-22.04".to_string(), 1),
            ("ubuntu-24.04".to_string(), 1),
        ]);

        assert_eq!(
            select_github_runner(&runners).as_deref(),
            Some("ubuntu-24.04")
        );
    }

    #[test]
    fn multiple_github_runners_are_reported_as_warnings() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/a.yml"),
            "jobs:\n  test:\n    runs-on: macos-latest\n",
        )
        .unwrap();
        fs::write(
            temp.path().join(".github/workflows/b.yml"),
            "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
        assert!(
            inference
                .warnings
                .iter()
                .any(|warning| { warning.contains("multiple GitHub Actions runners detected") })
        );
    }

    #[test]
    fn sqlx_with_windows_runner_reports_posix_command_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = "0.8"
"#,
        )
        .unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: windows-latest\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.sqlx_enabled, Some(true));
        assert_eq!(
            inference.ci_github_runner.as_deref(),
            Some("windows-latest")
        );
        assert!(inference.warnings.iter().any(|warning| {
            warning.contains("SQLx check command inference uses POSIX shell syntax")
        }));
    }

    #[test]
    fn multiline_github_runner_sequence_is_inferred() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on:\n      - ubuntu-24.04\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
    }

    #[test]
    fn composite_github_runner_labels_are_reported_as_warnings() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: [self-hosted, linux]\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.ci_github_runner.is_none());
        assert!(
            inference
                .warnings
                .iter()
                .any(|warning| { warning.contains("unsupported composite runs-on labels") })
        );
    }

    #[test]
    fn dynamic_github_runner_is_reported_as_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: ${{ matrix.runner }}\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.ci_github_runner.is_none());
        assert!(
            inference
                .warnings
                .iter()
                .any(|warning| { warning.contains("unsupported dynamic runs-on expression") })
        );
    }

    #[test]
    fn empty_github_runner_is_reported_as_warning() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  test:\n    runs-on: \"\"\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.ci_github_runner.is_none());
        assert!(
            inference
                .warnings
                .iter()
                .any(|warning| { warning.contains("empty runs-on value") })
        );
    }

    #[test]
    fn reusable_workflow_inputs_named_runs_on_are_not_inferred_as_job_runners() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
        fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  call:\n    uses: owner/repo/.github/workflows/test.yml@main\n    with:\n      runs-on: ubuntu-latest\n",
        )
        .unwrap();

        let inference = infer_adopt_answers(temp.path());

        assert!(inference.ci_github_runner.is_none());
    }
}
