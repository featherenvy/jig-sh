use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::{ANSWERS_FILE, UVX_BIN_ENV, external_program};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopierMode {
    Copy,
    Update,
    Recopy,
}

impl CopierMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Update => "update",
            Self::Recopy => "recopy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CopierCommandSpec {
    pub(super) program: String,
    pub(super) args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CopySpecOptions<'a> {
    pub(super) answers_data_path: Option<&'a Path>,
    pub(super) vcs_ref: Option<&'a str>,
    pub(super) force: bool,
    pub(super) overwrite: bool,
    pub(super) use_defaults: bool,
    pub(super) skip_tasks: bool,
}

pub(super) fn build_copy_spec(
    template: &str,
    destination: &Path,
    options: CopySpecOptions<'_>,
) -> CopierCommandSpec {
    let mut args = vec![
        "--from".into(),
        "copier".into(),
        "copier".into(),
        CopierMode::Copy.as_str().into(),
        "--trust".into(),
        "--answers-file".into(),
        ANSWERS_FILE.into(),
    ];
    if options.skip_tasks {
        args.push("--skip-tasks".into());
    }
    if let Some(answers_data_path) = options.answers_data_path {
        args.push("--data-file".into());
        args.push(answers_data_path.display().to_string());
    }
    if options.force {
        args.push("--force".into());
    } else {
        if options.overwrite {
            args.push("--overwrite".into());
        }
        if options.use_defaults {
            args.push("--defaults".into());
        }
    }
    if let Some(vcs_ref) = options.vcs_ref {
        args.push("--vcs-ref".into());
        args.push(vcs_ref.to_string());
    }
    args.push(template.to_string());
    args.push(destination.display().to_string());

    CopierCommandSpec {
        program: external_program(UVX_BIN_ENV, "uvx"),
        args,
    }
}

pub(super) fn build_update_spec(
    mode: CopierMode,
    destination: &Path,
    answers_file: &Path,
    vcs_ref: Option<&str>,
    defaults: bool,
    exclude_destination_answers: bool,
) -> CopierCommandSpec {
    let mut args = vec![
        "--from".into(),
        "copier".into(),
        "copier".into(),
        mode.as_str().into(),
        "--trust".into(),
        "--answers-file".into(),
        answers_file.display().to_string(),
    ];
    if defaults || mode == CopierMode::Recopy {
        args.push("--defaults".into());
    }
    if mode == CopierMode::Recopy {
        args.push("--overwrite".into());
    }
    if let Some(vcs_ref) = vcs_ref {
        args.push("--vcs-ref".into());
        args.push(vcs_ref.to_string());
    }
    if exclude_destination_answers {
        args.push("--exclude".into());
        args.push(ANSWERS_FILE.into());
    }
    args.push(destination.display().to_string());

    CopierCommandSpec {
        program: external_program(UVX_BIN_ENV, "uvx"),
        args,
    }
}

pub(super) fn run_copier(
    spec: CopierCommandSpec,
    current_dir: Option<&Path>,
    interactive: bool,
) -> Result<()> {
    let mut command = Command::new(&spec.program);
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    command.args(&spec.args);

    if interactive {
        let status = command
            .status()
            .with_context(|| format!("Failed to start {}", spec.program))?;
        if !status.success() {
            bail!(
                "Copier command failed with status {}",
                status.code().unwrap_or(1)
            );
        }
    } else {
        let output = command
            .output()
            .with_context(|| format!("Failed to start {}", spec.program))?;
        if !output.status.success() {
            bail!(
                "Copier command failed with status {}\nstdout:\n{}\nstderr:\n{}",
                output.status.code().unwrap_or(1),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    Ok(())
}
