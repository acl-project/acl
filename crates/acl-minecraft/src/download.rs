use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    // Label name used for logging.
    pub name: String,
    pub url: String,
    // The fully assembled complete local save path.
    pub destination: PathBuf,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub completed_files: usize,
    pub total_files: usize,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}
