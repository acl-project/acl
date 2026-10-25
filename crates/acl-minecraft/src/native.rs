use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

use tracing::{debug, warn};
use zip::ZipArchive;

use crate::{MinecraftError, Result};

pub const DEFAULT_NATIVE_EXCLUDE: &str = "META-INF/";

#[derive(Debug, Clone)]
pub struct NativeArchive {
    /// Library coordinate name for logging.
    pub name: String,
    /// Full local path to the downloaded native `.jar` file.
    pub archive_path: PathBuf,
    /// Path prefixes to exclude during extraction (e.g., `["META-INF/"]`).
    pub exclude: Vec<String>,
}

impl NativeArchive {
    pub fn is_excluded(&self, entry_name: &str) -> bool {
        if self.name.starts_with(DEFAULT_NATIVE_EXCLUDE) {
            return true;
        }

        self.exclude
            .iter()
            .any(|prefix| entry_name.starts_with(prefix))
    }
}

/// Extract a single native `.jar` archive into `natives_dir`.
pub fn extract_native_archive(archive: &NativeArchive, natives_dir: &Path) -> Result<()> {
    debug!(
        "Extracting native library '{}' from {}",
        archive.name,
        archive.archive_path.display()
    );

    let file = File::open(&archive.archive_path).map_err(|source| MinecraftError::Io {
        path: archive.archive_path.clone(),
        source,
    })?;

    let mut zip = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
        path: archive.archive_path.clone(),
        source,
    })?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|source| MinecraftError::Zip {
            path: archive.archive_path.clone(),
            source,
        })?;

        if entry.is_dir() {
            continue;
        }

        let entry_name = entry.name().to_string();

        if archive.is_excluded(&entry_name) {
            continue;
        }

        let rel_path = Path::new(&entry_name);

        if rel_path.as_os_str().is_empty()
            || rel_path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            warn!(
                "Skipping unsafe zip entry '{}' in native archive '{}'",
                entry_name, archive.name
            );
            continue;
        }

        // Flatten directory structure
        let Some(file_name) = rel_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Only extract Linux shared objects (.so).
        if !file_name.ends_with(".so") && !file_name.contains(".so.") {
            continue;
        }

        let dest_path = natives_dir.join(file_name);

        let mut buf = Vec::with_capacity(entry.size() as usize);

        entry
            .read_to_end(&mut buf)
            .map_err(|source| MinecraftError::Io {
                path: archive.archive_path.clone(),
                source,
            })?;

        acl_common::fs::write_atomic(&dest_path, &buf).map_err(|source| MinecraftError::Io {
            path: archive.archive_path.clone(),
            source,
        })?;
    }

    Ok(())
}

pub fn extract_all_natives(archives: &[NativeArchive], natives_dir: &Path) -> Result<()> {
    for archive in archives {
        extract_native_archive(archive, natives_dir)?;
    }

    Ok(())
}
