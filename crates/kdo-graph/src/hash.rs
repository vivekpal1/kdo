//! Content hashing for projects using blake3, parallelized with rayon.

use rayon::prelude::*;
use std::path::Path;
use tracing::debug;

/// Compute a deterministic blake3 hash of all files in a directory.
///
/// Files are sorted by path before hashing to ensure determinism.
/// Uses rayon for parallel file reading.
pub fn content_hash_dir(dir: &Path) -> [u8; 32] {
    let walker = ignore::WalkBuilder::new(dir)
        .hidden(true)
        .git_ignore(true)
        .add_custom_ignore_filename(".kdoignore")
        .build();

    let mut file_paths: Vec<_> = walker
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .map(|entry| entry.into_path())
        .collect();

    // Sort for determinism
    file_paths.sort();

    // Read files in parallel and collect (path, content) pairs
    let file_contents: Vec<(String, Vec<u8>)> = file_paths
        .par_iter()
        .filter_map(|path| {
            let content = std::fs::read(path).ok()?;
            let rel = path.strip_prefix(dir).unwrap_or(path);
            Some((rel.to_string_lossy().to_string(), content))
        })
        .collect();

    // Hash sequentially in sorted order
    let mut hasher = blake3::Hasher::new();
    for (rel_path, content) in &file_contents {
        hasher.update(rel_path.as_bytes());
        hasher.update(&(content.len() as u64).to_le_bytes());
        hasher.update(content);
    }

    let hash = *hasher.finalize().as_bytes();
    debug!(dir = %dir.display(), hash = %hex::encode(hash), files = file_contents.len(), "computed content hash");
    hash
}

/// Simple hex encoding for display.
mod hex {
    /// Encode bytes as lowercase hexadecimal.
    pub fn encode(bytes: [u8; 32]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
