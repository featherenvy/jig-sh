use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{RepoScan, push_scan_warning, read_toml_for_inference, relative_path_string};

const MAX_MIGRATION_SQL_DEPTH: usize = 3;

#[derive(Debug, Default)]
pub(super) struct SqlxInference {
    pub(super) enabled: bool,
    pub(super) migration_dir: Option<String>,
    pub(super) migration_dirs: Vec<String>,
    pub(super) metadata_dir: Option<String>,
    pub(super) check_command: Option<String>,
    pub(super) signals: Vec<String>,
}

pub(super) fn infer_rust_crate_roots(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let cargo_path = root.join("Cargo.toml");
    if !cargo_path.is_file() {
        return Vec::new();
    }
    let Some(parsed) = read_toml_for_inference(&cargo_path, warnings) else {
        return Vec::new();
    };
    let Some(workspace) = parsed.get("workspace").and_then(toml::Value::as_table) else {
        if parsed
            .get("package")
            .and_then(toml::Value::as_table)
            .is_some()
        {
            return vec![".".into()];
        }
        push_scan_warning(
            warnings,
            &cargo_path,
            "Cargo.toml has neither [workspace] nor [package]; Rust crate roots were not inferred",
        );
        return Vec::new();
    };
    let mut roots = BTreeSet::new();
    if let Some(members) = workspace.get("members").and_then(toml::Value::as_array) {
        for member in members.iter().filter_map(toml::Value::as_str) {
            if member.starts_with('!') {
                continue;
            }
            if let Some(root) = crate_root_from_workspace_member(member) {
                roots.insert(root);
            }
        }
    }
    if roots.is_empty() {
        roots.insert(".".into());
    }
    roots.into_iter().collect()
}

// Jig crate roots are parent directories whose direct children are crates.
pub(super) fn crate_root_from_workspace_member(member: &str) -> Option<String> {
    let path = member.trim().trim_end_matches('/');
    if path.is_empty() || path == "." {
        return Some(".".into());
    }
    let first_glob = path.find(['*', '[', '?']);
    if let Some(index) = first_glob {
        let prefix = path[..index].trim_end_matches('/');
        if prefix.is_empty() {
            return Some(".".into());
        }
        return Some(relative_path_string(Path::new(prefix)));
    }
    let parent = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let root = relative_path_string(parent);
    Some(if root.is_empty() { ".".into() } else { root })
}

pub(super) fn infer_sqlx(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> SqlxInference {
    let mut out = SqlxInference::default();
    if scan
        .named_files("Cargo.toml")
        .any(|path| cargo_toml_mentions_sqlx(path, warnings))
    {
        out.enabled = true;
        out.signals.push("SQLx dependency in Cargo.toml".into());
    }
    if scan.has_dir_named_at_root(root, ".sqlx") {
        out.enabled = true;
        out.metadata_dir = Some(".sqlx".into());
        out.signals.push("SQLx metadata directory .sqlx".into());
    }
    out.migration_dirs = find_migration_dirs(root, scan);
    if let Some(dir) = out.migration_dirs.first() {
        out.enabled = true;
        out.migration_dir = Some(dir.clone());
        if out.migration_dirs.len() == 1 {
            out.signals.push(format!("migration directory {dir}"));
        } else {
            push_scan_warning(
                warnings,
                root,
                &format!(
                    "multiple migration directories detected; using alphabetically first {} unless overridden",
                    dir
                ),
            );
            out.signals.push(format!(
                "migration directories detected: {}",
                out.migration_dirs.join(", ")
            ));
        }
    }
    if scan.any_text_file(&["rs"], warnings, |text| {
        text.lines().any(rust_line_invokes_sqlx_migrate)
    }) {
        out.enabled = true;
        out.signals.push("sqlx::migrate! macro".into());
    }
    if scan.any_text_file(&["sh"], warnings, |text| {
        text.lines().any(shell_line_invokes_cargo_sqlx)
    }) || scan.any_text_file(&["yml", "yaml"], warnings, |text| {
        text.lines().any(yaml_run_invokes_cargo_sqlx)
    }) {
        out.enabled = true;
        out.signals.push("cargo sqlx command".into());
    }
    if out.enabled {
        let synthesized_migration_dir = out.migration_dir.is_none();
        let synthesized_metadata_dir = out.metadata_dir.is_none();
        out.migration_dir.get_or_insert_with(|| "migrations".into());
        out.metadata_dir.get_or_insert_with(|| ".sqlx".into());
        if synthesized_migration_dir || synthesized_metadata_dir {
            push_scan_warning(
                warnings,
                root,
                "SQLx was detected but migration or metadata directories were not; using default SQLx paths unless overridden",
            );
        }
        let metadata_dir = out.metadata_dir.as_deref().unwrap_or(".sqlx");
        let workspace_arg = if cargo_workspace_declared(root, warnings) {
            " --workspace"
        } else {
            ""
        };
        // `prepare --check` intentionally connects to the database while
        // comparing against the configured metadata directory. Adopt renders
        // this command for POSIX-like local and CI environments.
        // Windows SQLx checks should be supplied explicitly with
        // `--sqlx-check-command`.
        out.check_command = Some(format!(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='{}' cargo sqlx prepare --check{} -- --all-targets",
            metadata_dir.replace('\'', "'\\''"),
            workspace_arg
        ));
        out.signals
            .push("SQLx check command assumes online cargo sqlx prepare".into());
    } else {
        out.signals.push("no SQLx signals detected".into());
    }
    out
}

fn cargo_toml_mentions_sqlx(path: &Path, warnings: &mut Vec<String>) -> bool {
    let Some(parsed) = read_toml_for_inference(path, warnings) else {
        return false;
    };
    [
        "dependencies",
        "dev-dependencies",
        "build-dependencies",
        "workspace.dependencies",
    ]
    .iter()
    .any(|section| toml_section(&parsed, section).is_some_and(|table| table.contains_key("sqlx")))
}

fn cargo_workspace_declared(root: &Path, warnings: &mut Vec<String>) -> bool {
    let path = root.join("Cargo.toml");
    path.is_file()
        && read_toml_for_inference(&path, warnings)
            .is_some_and(|parsed| parsed.get("workspace").is_some())
}

fn toml_section<'a>(
    value: &'a toml::Value,
    dotted: &str,
) -> Option<&'a toml::map::Map<String, toml::Value>> {
    let mut cursor = value;
    for key in dotted.split('.') {
        cursor = cursor.get(key)?;
    }
    cursor.as_table()
}

fn find_migration_dirs(root: &Path, scan: &RepoScan) -> Vec<String> {
    let mut candidates = Vec::new();
    for path in scan.dirs_named("migrations") {
        if migration_dir_has_sql(path, scan) {
            candidates.push(relative_path_string(
                path.strip_prefix(root).unwrap_or(path),
            ));
        }
    }
    candidates.sort();
    candidates
}

fn migration_dir_has_sql(path: &Path, scan: &RepoScan) -> bool {
    scan.files_under(path).any(|entry_path| {
        let Ok(relative) = entry_path.strip_prefix(path) else {
            return false;
        };
        relative.components().count() <= MAX_MIGRATION_SQL_DEPTH + 1
            && migration_sql_file_is_supported(entry_path)
    })
}

fn migration_sql_file_is_supported(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
        return false;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(starts_with_ascii_digit)
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(starts_with_ascii_digit)
}

fn starts_with_ascii_digit(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_digit)
}

fn yaml_run_invokes_cargo_sqlx(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let Some(command) = trimmed.strip_prefix("run:") else {
        return false;
    };
    command_invokes_cargo_sqlx(strip_yaml_inline_comment(command).trim())
}

fn strip_yaml_inline_comment(value: &str) -> &str {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    for (index, ch) in value.char_indices() {
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '#' if !in_single_quote && !in_double_quote => return &value[..index],
            _ => {}
        }
    }
    value
}

fn rust_line_invokes_sqlx_migrate(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with("//")
        && !trimmed.starts_with("/*")
        && !trimmed.starts_with('*')
        && trimmed.contains("sqlx::migrate!")
}

fn shell_line_invokes_cargo_sqlx(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.starts_with('#') && command_invokes_cargo_sqlx(strip_shell_inline_comment(trimmed))
}

fn strip_shell_inline_comment(value: &str) -> &str {
    strip_yaml_inline_comment(value).trim()
}

fn command_invokes_cargo_sqlx(command: &str) -> bool {
    // Keep this conservative: detect direct invocations and skip comments or prose.
    let mut tokens = command
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')' | '"' | '\'')
        })
        .filter(|token| !token.is_empty())
        .skip_while(|token| token.contains('='));
    matches!(
        (tokens.next(), tokens.next()),
        (Some("cargo"), Some("sqlx"))
    )
}
