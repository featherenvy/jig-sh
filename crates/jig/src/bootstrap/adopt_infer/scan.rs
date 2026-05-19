use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

pub(super) const MAX_SCAN_FILE_BYTES: u64 = 512 * 1024;
pub(super) const MAX_SCAN_DEPTH: usize = 5;
pub(super) const MAX_SCAN_WARNINGS: usize = 20;

#[derive(Debug)]
pub(super) struct RepoScan {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl RepoScan {
    pub(super) fn collect(root: &Path, warnings: &mut Vec<String>) -> Self {
        let mut scan = Self {
            files: Vec::new(),
            dirs: Vec::new(),
        };
        scan.collect_inner(root, root, 0, warnings);
        scan.files.sort();
        scan.dirs.sort();
        scan
    }

    pub(super) fn named_files<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.files
            .iter()
            .filter(move |path| path.file_name().and_then(|value| value.to_str()) == Some(name))
    }

    pub(super) fn files_with_extensions<'a>(
        &'a self,
        extensions: &'a [&'a str],
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.files.iter().filter(move |path| {
            extensions
                .iter()
                .any(|ext| path.extension().and_then(|value| value.to_str()) == Some(*ext))
        })
    }

    pub(super) fn dirs_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.dirs
            .iter()
            .filter(move |path| path.file_name().and_then(|value| value.to_str()) == Some(name))
    }

    pub(super) fn has_dir_named_at_root(&self, root: &Path, name: &str) -> bool {
        self.dirs.iter().any(|path| {
            path.parent() == Some(root)
                && path.file_name().and_then(|value| value.to_str()) == Some(name)
        })
    }

    pub(super) fn any_text_file<F>(
        &self,
        extensions: &[&str],
        warnings: &mut Vec<String>,
        mut predicate: F,
    ) -> bool
    where
        F: FnMut(&str) -> bool,
    {
        self.files_with_extensions(extensions)
            .any(|path| match read_limited_text(path) {
                Ok(text) => predicate(&text),
                Err(error) => {
                    push_scan_warning(
                        warnings,
                        path,
                        &format!("could not read text for inference: {error}"),
                    );
                    false
                }
            })
    }

    pub(super) fn files_under<'a>(
        &'a self,
        dir: &'a Path,
    ) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.files.iter().filter(move |path| path.starts_with(dir))
    }

    fn collect_inner(&mut self, root: &Path, dir: &Path, depth: usize, warnings: &mut Vec<String>) {
        if depth > MAX_SCAN_DEPTH {
            push_scan_warning(warnings, dir, "maximum inference scan depth reached");
            return;
        }
        if depth > 0 && should_skip_dir(dir) {
            return;
        }
        for entry in read_dir_entries(dir, warnings) {
            let path = entry.path();
            if entry_is_dir(&entry, warnings) {
                if path.starts_with(root) {
                    self.dirs.push(path.clone());
                    self.collect_inner(root, &path, depth + 1, warnings);
                }
            } else if entry_is_file(&entry, warnings) {
                self.files.push(path);
            }
        }
    }
}

pub(super) fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name,
                ".git"
                    | ".jig"
                    | ".idea"
                    | ".next"
                    | ".svelte-kit"
                    | ".turbo"
                    | ".venv"
                    | "build"
                    | "coverage"
                    | "dist"
                    | "node_modules"
                    | "out"
                    | "target"
                    | "tmp"
                    | "vendor"
            )
        })
}

pub(super) fn read_dir_entries(dir: &Path, warnings: &mut Vec<String>) -> Vec<fs::DirEntry> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            push_scan_warning(warnings, dir, &format!("could not read directory: {error}"));
            return Vec::new();
        }
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(error) => {
                push_scan_warning(
                    warnings,
                    dir,
                    &format!("could not read directory entry: {error}"),
                );
                None
            }
        })
        .collect()
}

pub(super) fn entry_is_dir(entry: &fs::DirEntry, warnings: &mut Vec<String>) -> bool {
    match entry.file_type() {
        // Directory symlinks are not followed. That avoids cycles and prevents
        // adopt inference from pulling in files outside the repository tree.
        Ok(file_type) => file_type.is_dir(),
        Err(error) => {
            push_scan_warning(
                warnings,
                &entry.path(),
                &format!("could not inspect file type: {error}"),
            );
            false
        }
    }
}

pub(super) fn entry_is_file(entry: &fs::DirEntry, warnings: &mut Vec<String>) -> bool {
    match entry.file_type() {
        Ok(file_type) => file_type.is_file(),
        Err(error) => {
            push_scan_warning(
                warnings,
                &entry.path(),
                &format!("could not inspect file type: {error}"),
            );
            false
        }
    }
}

pub(super) fn push_scan_warning(warnings: &mut Vec<String>, path: &Path, message: &str) {
    if warnings.len() >= MAX_SCAN_WARNINGS {
        return;
    }
    if warnings.len() + 1 == MAX_SCAN_WARNINGS {
        warnings.push("additional inference scan warnings omitted".into());
        return;
    }
    warnings.push(format!("{}: {message}", path.display()));
}

pub(super) fn read_toml(path: &Path) -> Result<toml::Value> {
    let text = read_limited_text(path)?;
    toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

pub(super) fn read_toml_for_inference(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<toml::Value> {
    match read_toml(path) {
        Ok(value) => Some(value),
        Err(error) => {
            push_scan_warning(
                warnings,
                path,
                &format!("could not read TOML for inference: {error}"),
            );
            None
        }
    }
}

pub(super) fn read_json(path: &Path) -> Result<JsonValue> {
    let text = read_limited_text(path)?;
    serde_json::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

pub(super) fn read_json_for_inference(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<JsonValue> {
    match read_json(path) {
        Ok(value) => Some(value),
        Err(error) => {
            push_scan_warning(
                warnings,
                path,
                &format!("could not read JSON for inference: {error}"),
            );
            None
        }
    }
}

pub(super) fn read_yaml_for_inference(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Option<YamlValue> {
    let text = match read_limited_text(path) {
        Ok(text) => text,
        Err(error) => {
            push_scan_warning(
                warnings,
                path,
                &format!("could not read YAML for inference: {error}"),
            );
            return None;
        }
    };
    match serde_yaml_ng::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            push_scan_warning(
                warnings,
                path,
                &format!("could not parse YAML for inference: {error}"),
            );
            None
        }
    }
}

pub(super) fn yaml_mapping_get<'a>(value: &'a YamlValue, key: &str) -> Option<&'a YamlValue> {
    value.as_mapping()?.iter().find_map(|(candidate, value)| {
        if candidate.as_str() == Some(key) {
            Some(value)
        } else {
            None
        }
    })
}

pub(super) fn read_limited_text(path: &Path) -> Result<String> {
    let file =
        fs::File::open(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_SCAN_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if bytes.len() as u64 > MAX_SCAN_FILE_BYTES {
        anyhow::bail!(
            "{} is larger than {MAX_SCAN_FILE_BYTES} bytes",
            path.display()
        );
    }
    String::from_utf8(bytes).with_context(|| format!("Failed to read {}", path.display()))
}

pub(super) fn relative_path_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            Component::CurDir => Some("."),
            unexpected => {
                debug_assert!(
                    false,
                    "relative path contained unsupported component: {unexpected:?}"
                );
                None
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}
