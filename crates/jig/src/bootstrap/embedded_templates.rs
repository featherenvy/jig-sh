pub(super) struct EmbeddedTemplateFile {
    pub(super) relative_path: &'static str,
    pub(super) contents: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_templates.rs"));

#[cfg(test)]
mod snapshot {
    use super::EmbeddedTemplateFile;

    include!("embedded_templates_snapshot.rs");
}

#[cfg(test)]
mod tests {
    use super::{EMBEDDED_TEMPLATE_FILES, snapshot};

    const REFRESH_COMMAND: &str = "JIG_REFRESH_EMBEDDED_TEMPLATE_SNAPSHOT=1 cargo check -p jig-sh";

    #[test]
    fn embedded_template_snapshot_matches_live_templates() {
        assert!(
            std::env::var_os("JIG_EMBEDDED_TEMPLATE_SNAPSHOT").is_none(),
            "JIG_EMBEDDED_TEMPLATE_SNAPSHOT makes live embedded templates come from the snapshot; unset it to run the snapshot drift test"
        );
        assert_eq!(
            EMBEDDED_TEMPLATE_FILES.len(),
            snapshot::EMBEDDED_TEMPLATE_FILES.len(),
            "embedded template snapshot file count is stale; run {REFRESH_COMMAND}"
        );

        for (live, snapshotted) in EMBEDDED_TEMPLATE_FILES
            .iter()
            .zip(snapshot::EMBEDDED_TEMPLATE_FILES)
        {
            assert_eq!(
                live.relative_path, snapshotted.relative_path,
                "embedded template snapshot paths are stale; run {REFRESH_COMMAND}"
            );
            assert_eq!(
                live.contents, snapshotted.contents,
                "embedded template snapshot contents are stale for {}; run {REFRESH_COMMAND}",
                live.relative_path
            );
        }
    }
}
