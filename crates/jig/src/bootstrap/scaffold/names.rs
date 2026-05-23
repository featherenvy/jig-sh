use std::path::{Component, Path};

use anyhow::{Result, bail};

pub(super) fn default_repo_name(destination: &Path) -> String {
    destination
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("app")
        .to_string()
}

pub(super) fn sanitize_package_name(value: &str) -> Result<String> {
    let mut package = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if previous_dash {
                continue;
            }
            previous_dash = true;
        } else {
            previous_dash = false;
        }
        package.push(mapped);
    }
    let mut package = package.trim_matches('-').to_string();
    if package.is_empty() {
        bail!("Could not derive a Rust package name from '{value}'");
    }
    if !is_valid_rust_crate_identifier(&package.replace('-', "_")) {
        package = format!("app-{package}");
    }
    Ok(package)
}

pub(super) fn validate_scaffold_name(label: &str, value: &str) -> Result<()> {
    if !value.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        bail!("Scaffold {label} must contain at least one ASCII letter or digit");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        bail!("Scaffold {label} contains unsupported characters: {value}");
    }
    Ok(())
}

fn is_valid_rust_crate_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !is_rust_keyword(value)
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}

pub(super) fn validate_scaffold_relative_path(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("Scaffold {label} must be a non-empty relative path");
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_'))
    {
        bail!("Scaffold {label} contains unsupported characters: {value}");
    }
    if value.contains("//") {
        bail!("Scaffold {label} must not contain empty path segments: {value}");
    }
    let path = Path::new(value);
    if path.is_absolute() {
        bail!("Scaffold {label} must be relative: {value}");
    }
    for component in path.components() {
        match component {
            Component::Normal(part) if !part.is_empty() => {}
            Component::CurDir | Component::ParentDir => {
                bail!("Scaffold {label} must not contain '.' or '..': {value}");
            }
            _ => bail!("Scaffold {label} must be relative: {value}"),
        }
    }
    Ok(())
}
