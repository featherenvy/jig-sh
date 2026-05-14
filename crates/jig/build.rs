use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=JIG_ASSUME_RELEASE_BUILD");
    println!("cargo:rerun-if-env-changed=CI");

    let assume_release = env::var_os("JIG_ASSUME_RELEASE_BUILD").is_some();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo");
    add_git_rerun_inputs(&manifest_dir);

    let detected_policy = detect_template_pin_policy(&manifest_dir);
    let policy = if assume_release {
        if detected_policy != TemplatePinPolicy::Released {
            println!(
                "cargo:warning=JIG_ASSUME_RELEASE_BUILD is overriding Jig's detected {detected_policy} build policy; use this only after version/tag validation."
            );
        }
        TemplatePinPolicy::Released
    } else {
        detected_policy
    };

    emit_template_pin_policy(policy);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemplatePinPolicy {
    Released,
    Unreleased,
    Unknown,
}

impl TemplatePinPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::Unreleased => "unreleased",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for TemplatePinPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

fn detect_template_pin_policy(manifest_dir: &str) -> TemplatePinPolicy {
    if !is_git_worktree(manifest_dir) {
        if warn_about_git_metadata_without_git(manifest_dir) {
            return TemplatePinPolicy::Unreleased;
        }
        // Published crates do not include .git metadata. Treat them as release
        // artifacts so crates.io installs can still use the version tag.
        return TemplatePinPolicy::Unknown;
    }

    let head_tags = git_head_tags(manifest_dir);
    let clean = git_tree_is_clean(manifest_dir);
    if exact_version_tag(&head_tags) && clean {
        TemplatePinPolicy::Released
    } else {
        warn_ci_about_unreleased_policy(&head_tags, clean);
        TemplatePinPolicy::Unreleased
    }
}

fn emit_template_pin_policy(policy: TemplatePinPolicy) {
    println!(
        "cargo:rustc-env=JIG_BUILD_OFFICIAL_TEMPLATE_PIN={}",
        policy.as_str()
    );
}

fn add_git_rerun_inputs(manifest_dir: &str) {
    for path in [
        Path::new(manifest_dir).join("build.rs"),
        Path::new(manifest_dir).join("Cargo.toml"),
        Path::new(manifest_dir).join("src"),
        Path::new(manifest_dir).join("../../Cargo.toml"),
        Path::new(manifest_dir).join("../../Cargo.lock"),
        Path::new(manifest_dir).join("../../templates"),
    ] {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    for git_dir in git_dirs(manifest_dir) {
        for path in [
            git_dir.join("HEAD"),
            git_dir.join("index"),
            git_dir.join("packed-refs"),
            git_dir.join("refs/heads"),
            git_dir.join("refs/tags"),
            git_dir.join("commondir"),
        ] {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

fn git_dirs(manifest_dir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for args in [
        ["rev-parse", "--git-dir"],
        ["rev-parse", "--git-common-dir"],
    ] {
        if let Some(path) =
            git_output(manifest_dir, &args).map(|path| absolute_git_path(manifest_dir, path))
        {
            if !dirs.contains(&path) {
                dirs.push(path);
            }
        }
    }
    dirs
}

fn absolute_git_path(manifest_dir: &str, path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        Path::new(manifest_dir).join(path)
    }
}

fn is_git_worktree(manifest_dir: &str) -> bool {
    git_output(manifest_dir, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

fn exact_version_tag(head_tags: &[String]) -> bool {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is set by Cargo");
    let expected_tag = format!("v{version}");
    head_tags.iter().any(|tag| tag == &expected_tag)
}

fn git_head_tags(manifest_dir: &str) -> Vec<String> {
    git_output(manifest_dir, &["tag", "--points-at", "HEAD"])
        .map(|tags| {
            tags.lines()
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn warn_ci_about_unreleased_policy(head_tags: &[String], clean: bool) {
    if env::var_os("CI").is_none() {
        return;
    }
    if head_tags.is_empty() {
        println!(
            "cargo:warning=Jig build found no git tags pointing at HEAD; release builds need fetched tags or JIG_ASSUME_RELEASE_BUILD=1 after version/tag validation."
        );
    }
    if !clean {
        println!(
            "cargo:warning=Jig build found tracked working-tree changes; release builds need a clean checkout or JIG_ASSUME_RELEASE_BUILD=1 after version/tag validation."
        );
    }
}

fn warn_about_git_metadata_without_git(manifest_dir: &str) -> bool {
    if find_git_marker(manifest_dir).is_some() {
        println!(
            "cargo:warning=Jig build found .git metadata but could not query git; default template pin policy will be treated as unreleased."
        );
        return true;
    }
    false
}

fn find_git_marker(manifest_dir: &str) -> Option<PathBuf> {
    let mut path = Some(Path::new(manifest_dir));
    while let Some(current) = path {
        let marker = current.join(".git");
        if marker.exists() {
            return Some(marker);
        }
        path = current.parent();
    }
    None
}

fn git_tree_is_clean(manifest_dir: &str) -> bool {
    git_output(
        manifest_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .is_some_and(|status| status.trim().is_empty())
}

fn git_output(manifest_dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(manifest_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
