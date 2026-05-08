use std::collections::BTreeSet;
use std::path::PathBuf;

use tempfile::TempDir;

pub(super) struct StagedRender {
    pub(super) _root: TempDir,
    pub(super) destination: PathBuf,
    pub(super) managed_paths: BTreeSet<PathBuf>,
}
