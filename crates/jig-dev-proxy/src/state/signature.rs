use std::fs;
use std::path::Path;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

pub(crate) type FileSignature = Option<(SystemTime, u64, [u8; 32])>;

pub(crate) fn file_signature(path: &Path) -> FileSignature {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len(), file_hash(path)?))
}

fn file_hash(path: &Path) -> Option<[u8; 32]> {
    let bytes = fs::read(path).ok()?;
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Some(out)
}
