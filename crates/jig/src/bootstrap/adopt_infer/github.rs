use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml_ng::Value as YamlValue;

use super::scan::{RepoScan, push_scan_warning, read_yaml_for_inference, yaml_mapping_get};

pub(super) fn infer_ci_github_runner(
    root: &Path,
    scan: &RepoScan,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let workflows = root.join(".github/workflows");
    if !workflows.is_dir() {
        return None;
    }
    let mut files = scan
        .files_with_extensions(&["yml", "yaml"])
        .filter(|path| path.parent() == Some(workflows.as_path()))
        .cloned()
        .collect::<Vec<_>>();
    files.sort();
    let mut runners = BTreeMap::<String, usize>::new();
    for path in files {
        let Some(workflow) = read_yaml_for_inference(&path, warnings) else {
            continue;
        };
        collect_github_runners(&path, &workflow, warnings, &mut |runner| {
            *runners.entry(runner).or_default() += 1;
        });
    }
    if runners.len() > 1 {
        push_scan_warning(
            warnings,
            &workflows,
            "multiple GitHub Actions runners detected; using the most common runner with an ubuntu tie-break",
        );
    }
    select_github_runner(&runners)
}

pub(super) fn select_github_runner(runners: &BTreeMap<String, usize>) -> Option<String> {
    runners
        .iter()
        .max_by(|(left, left_count), (right, right_count)| {
            // Prefer the most common runner; use ubuntu labels as the stable
            // tie-break because the generated workflows are POSIX-oriented.
            // The final lexical tie-break keeps newer ubuntu version labels
            // such as ubuntu-24.04 ahead of older labels.
            left_count
                .cmp(right_count)
                .then_with(|| runner_preference(left).cmp(&runner_preference(right)))
                .then_with(|| left.cmp(right))
        })
        .map(|(runner, _)| runner.clone())
}

fn runner_preference(runner: &str) -> u8 {
    if runner.starts_with("ubuntu-") { 1 } else { 0 }
}

fn collect_github_runners<F>(
    path: &Path,
    workflow: &YamlValue,
    warnings: &mut Vec<String>,
    out: &mut F,
) where
    F: FnMut(String),
{
    let Some(jobs) = yaml_mapping_get(workflow, "jobs").and_then(YamlValue::as_mapping) else {
        return;
    };
    for job in jobs.values() {
        let Some(runs_on) = yaml_mapping_get(job, "runs-on") else {
            continue;
        };
        collect_github_runner_value(path, runs_on, warnings, out);
    }
}

fn collect_github_runner_value<F>(
    path: &Path,
    value: &YamlValue,
    warnings: &mut Vec<String>,
    out: &mut F,
) where
    F: FnMut(String),
{
    match value {
        YamlValue::String(runner) => {
            let runner = runner.trim();
            if runner.is_empty() {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses empty runs-on value",
                );
            } else if runner_is_static_scalar(runner) {
                out(runner.to_string());
            } else {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses unsupported dynamic runs-on expression",
                );
            }
        }
        YamlValue::Sequence(items) => {
            let runners = items
                .iter()
                .filter_map(YamlValue::as_str)
                .map(str::trim)
                .filter(|runner| !runner.is_empty())
                .collect::<Vec<_>>();
            if runners.len() == 1 && runner_is_static_scalar(runners[0]) {
                out(runners[0].to_string());
            } else {
                push_scan_warning(
                    warnings,
                    path,
                    "GitHub Actions runner uses unsupported composite runs-on labels",
                );
            }
        }
        YamlValue::Null => {
            push_scan_warning(
                warnings,
                path,
                "GitHub Actions runner uses empty runs-on value",
            );
        }
        _ => {
            push_scan_warning(
                warnings,
                path,
                "GitHub Actions runner uses unsupported runs-on shape",
            );
        }
    }
}

fn runner_is_static_scalar(value: &str) -> bool {
    !value.is_empty() && !value.contains("${{")
}
