use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{RepoScan, push_scan_warning};

pub(super) fn infer_package_manager(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let lockfiles = [
        ("pnpm-lock.yaml", "pnpm"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
        ("yarn.lock", "yarn"),
    ];
    let root_matches = lockfiles
        .iter()
        .filter(|(lockfile, _)| root.join(lockfile).is_file())
        .map(|(_, manager)| *manager)
        .collect::<Vec<_>>();
    if let Some(manager) = root_matches.first() {
        let managers = root_matches.iter().copied().collect::<BTreeSet<_>>();
        if managers.len() > 1 {
            push_scan_warning(
                warnings,
                root,
                &format!("multiple root package manager lockfiles detected; using {manager}"),
            );
        }
        return Some((*manager).into());
    }

    let mut matches = Vec::new();
    for (lockfile, manager) in lockfiles {
        for path in scan.named_files(lockfile) {
            matches.push((path_depth(root, path), path.clone(), manager));
        }
    }
    matches.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let managers = matches
        .iter()
        .map(|(_, _, manager)| *manager)
        .collect::<BTreeSet<_>>();
    if managers.len() > 1 {
        push_scan_warning(
            warnings,
            root,
            &format!(
                "multiple package manager lockfiles detected; using {}",
                matches[0].2
            ),
        );
    }
    matches.first().map(|(_, _, manager)| (*manager).into())
}

fn path_depth(root: &Path, path: &Path) -> usize {
    path.strip_prefix(root)
        .map(|path| path.components().count())
        .unwrap_or(usize::MAX)
}
