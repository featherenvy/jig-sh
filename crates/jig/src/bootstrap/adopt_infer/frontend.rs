use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use super::super::FrontendApp;
use super::repo::safe_name;
use super::scan::{
    MAX_SCAN_DEPTH, entry_is_dir, push_scan_warning, read_dir_entries, read_json_for_inference,
    read_yaml_for_inference, relative_path_string, should_skip_dir, yaml_mapping_get,
};

// Matches the default frontend coverage threshold rendered by the bootstrap template.
const FRONTEND_COVERAGE_THRESHOLD: u32 = 80;
const REQUIRED_FRONTEND_SCRIPTS: &[&str] = &["lint", "typecheck", "build:bundle", "test:coverage"];

pub(super) fn infer_frontend_apps(
    root: &Path,
    inferred_repo_name: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<FrontendApp> {
    let workspace_declared = workspace_declaration_present(root, warnings);
    let workspace_candidates = workspace_package_dirs(root, warnings);
    let mut candidates = Vec::new();
    if workspace_candidates.is_empty() && !workspace_declared {
        if root.join("package.json").is_file() {
            candidates.push(root.to_path_buf());
        }
        for glob in ["apps/*", "packages/*"] {
            expand_workspace_glob(root, glob, &mut candidates, 0, warnings);
        }
        for dir in ["web", "frontend"] {
            let candidate = root.join(dir);
            if candidate.join("package.json").is_file() {
                candidates.push(candidate);
            }
        }
    } else {
        candidates.extend(workspace_candidates);
    }
    candidates.sort();
    candidates.dedup();

    let mut apps = Vec::new();
    let mut names = BTreeSet::new();
    for dir in candidates {
        let package_path = dir.join("package.json");
        let Some(package) = read_json_for_inference(&package_path, warnings) else {
            continue;
        };
        let Some(scripts) = package.get("scripts").and_then(JsonValue::as_object) else {
            continue;
        };
        let Some(dev_script) = non_empty_script(scripts, "dev") else {
            continue;
        };
        let missing_scripts = REQUIRED_FRONTEND_SCRIPTS
            .iter()
            .copied()
            .filter(|script| non_empty_script(scripts, script).is_none())
            .collect::<Vec<_>>();
        if !missing_scripts.is_empty() {
            push_scan_warning(
                warnings,
                &package_path,
                &format!(
                    "frontend package has a dev script but is missing required CI scripts: {}",
                    missing_scripts.join(", ")
                ),
            );
            continue;
        }
        let relative = relative_path_string(dir.strip_prefix(root).unwrap_or(&dir));
        let dir_value = if relative.is_empty() {
            ".".into()
        } else {
            relative
        };
        let base_name = if dir_value == "." {
            inferred_repo_name.unwrap_or("web").to_string()
        } else {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("web")
                .to_string()
        };
        let name = unique_frontend_name(safe_frontend_name(&base_name), &mut names);
        let kind = if script_looks_like_vite(dev_script) {
            "vite"
        } else {
            "env-port"
        };
        apps.push(FrontendApp {
            name,
            dir: dir_value,
            coverage_threshold: FRONTEND_COVERAGE_THRESHOLD,
            kind: kind.into(),
        });
    }
    apps.sort_by(|left, right| left.dir.cmp(&right.dir));
    apps
}

fn workspace_package_dirs(root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let mut globs = Vec::new();
    globs.extend(package_json_workspace_globs(root, warnings));
    globs.extend(pnpm_workspace_globs(root, warnings));
    let mut dirs = Vec::new();
    let mut exclusions = Vec::new();
    for glob in globs.iter().filter(|glob| !glob.starts_with('!')) {
        if glob_escapes_root(glob) {
            continue;
        }
        expand_workspace_glob(root, glob, &mut dirs, 0, warnings);
    }
    for glob in globs.iter().filter_map(|glob| glob.strip_prefix('!')) {
        if glob_escapes_root(glob) {
            continue;
        }
        expand_workspace_glob(root, glob, &mut exclusions, 0, warnings);
    }
    exclusions.sort();
    exclusions.dedup();
    dirs.retain(|dir| exclusions.binary_search(dir).is_err());
    dirs
}

fn workspace_declaration_present(root: &Path, warnings: &mut Vec<String>) -> bool {
    package_json_has_workspaces(root, warnings) || root.join("pnpm-workspace.yaml").is_file()
}

fn package_json_has_workspaces(root: &Path, warnings: &mut Vec<String>) -> bool {
    let package_path = root.join("package.json");
    if !package_path.is_file() {
        return false;
    }
    let Some(package) = read_json_for_inference(&package_path, warnings) else {
        return false;
    };
    package.get("workspaces").is_some()
}

fn package_json_workspace_globs(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let package_path = root.join("package.json");
    if !package_path.is_file() {
        return Vec::new();
    }
    let Some(package) = read_json_for_inference(&package_path, warnings) else {
        return Vec::new();
    };
    let Some(workspaces) = package.get("workspaces") else {
        return Vec::new();
    };
    if let Some(items) = workspaces.as_array() {
        return items
            .iter()
            .filter_map(JsonValue::as_str)
            .map(str::to_string)
            .collect();
    }
    workspaces
        .get("packages")
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn pnpm_workspace_globs(root: &Path, warnings: &mut Vec<String>) -> Vec<String> {
    let path = root.join("pnpm-workspace.yaml");
    let Some(yaml) = read_yaml_for_inference(&path, warnings) else {
        return Vec::new();
    };
    let Some(packages) = yaml_mapping_get(&yaml, "packages") else {
        push_scan_warning(
            warnings,
            &path,
            "pnpm-workspace.yaml did not declare supported packages globs",
        );
        return Vec::new();
    };
    let mut globs = Vec::new();
    match packages {
        YamlValue::Sequence(items) => {
            for item in items {
                if let Some(glob) = yaml_workspace_glob(item) {
                    globs.push(glob);
                } else {
                    push_scan_warning(
                        warnings,
                        &path,
                        "pnpm-workspace.yaml contains unsupported non-string packages entries",
                    );
                }
            }
        }
        value => {
            if let Some(glob) = yaml_workspace_glob(value) {
                globs.push(glob);
            } else {
                push_scan_warning(
                    warnings,
                    &path,
                    "pnpm-workspace.yaml packages must be a string array",
                );
            }
        }
    }
    if globs.is_empty() {
        push_scan_warning(
            warnings,
            &path,
            "pnpm-workspace.yaml did not declare supported packages globs",
        );
    }
    globs
}

fn yaml_workspace_glob(value: &YamlValue) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            // Unquoted YAML entries like `!packages/private` parse as a tag
            // with a null value. Recover that shape as the pnpm exclusion glob
            // the user wrote.
            if let YamlValue::Tagged(tagged) = value
                && matches!(tagged.value, YamlValue::Null)
            {
                let tag = tagged.tag.to_string();
                return tag.starts_with('!').then_some(tag);
            }
            None
        })
}

fn expand_workspace_glob(
    root: &Path,
    glob: &str,
    out: &mut Vec<PathBuf>,
    depth: usize,
    warnings: &mut Vec<String>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let segments = glob.split('/').collect::<Vec<_>>();
    expand_segments(root, root, &segments, out, depth, warnings)
}

fn expand_segments(
    root: &Path,
    base: &Path,
    segments: &[&str],
    out: &mut Vec<PathBuf>,
    depth: usize,
    warnings: &mut Vec<String>,
) {
    if depth > MAX_SCAN_DEPTH || (depth > 0 && should_skip_dir(base)) {
        return;
    }
    if segments.is_empty() {
        if base.join("package.json").is_file() {
            out.push(base.to_path_buf());
        }
        return;
    }
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    if *first == "**" {
        expand_segments(root, base, rest, out, depth + 1, warnings);
        for entry in read_dir_entries(base, warnings) {
            if entry_is_dir(&entry, warnings) {
                expand_segments(root, &entry.path(), segments, out, depth + 1, warnings);
            }
        }
        return;
    }
    if first.contains('*') {
        for entry in read_dir_entries(base, warnings) {
            if entry_is_dir(&entry, warnings)
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| segment_matches(first, name))
            {
                expand_segments(root, &entry.path(), rest, out, depth + 1, warnings);
            }
        }
    } else {
        let next = base.join(first);
        // `root` and `next` are lexical paths under the already resolved
        // destination, and directory symlinks are not followed during expansion.
        if next.starts_with(root) {
            expand_segments(root, &next, rest, out, depth + 1, warnings);
        }
    }
}

fn non_empty_script<'a>(
    scripts: &'a serde_json::Map<String, JsonValue>,
    name: &str,
) -> Option<&'a str> {
    scripts
        .get(name)
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn script_looks_like_vite(value: &str) -> bool {
    let tokens = value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')'))
        .map(|token| token.trim_matches(['"', '\'']))
        .filter(|token| !token.is_empty())
        .map(|token| token.rsplit('/').next().unwrap_or(token))
        .collect::<Vec<_>>();
    let Some(vite_index) = tokens
        .iter()
        .position(|token| *token == "vite" || token.starts_with("vite@"))
    else {
        return false;
    };
    !tokens[vite_index + 1..]
        .iter()
        .any(|token| matches!(*token, "build" | "preview" | "optimize"))
}

fn safe_frontend_name(value: &str) -> String {
    safe_name(value, "web")
}

fn unique_frontend_name(name: String, seen: &mut BTreeSet<String>) -> String {
    if seen.insert(name.clone()) {
        return name;
    }
    for index in 2.. {
        let candidate = format!("{name}-{index}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

pub(super) fn segment_matches(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    };
    let mut remaining = name;
    let mut parts = pattern.split('*').peekable();
    if let Some(first) = parts.next()
        && !first.is_empty()
    {
        let Some(stripped) = remaining.strip_prefix(first) else {
            return false;
        };
        remaining = stripped;
    }
    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        if parts.peek().is_none() && !pattern.ends_with('*') {
            return remaining[index..].ends_with(part);
        }
        remaining = &remaining[index + part.len()..];
    }
    pattern.ends_with('*') || remaining.is_empty()
}

fn glob_escapes_root(glob: &str) -> bool {
    Path::new(glob).is_absolute()
        || Path::new(glob)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
}
