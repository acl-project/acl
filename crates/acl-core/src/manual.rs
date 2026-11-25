// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistence for CurseForge modpack files that graciously require manual human intervention.
//!
//! Because nothing elevates the modded Minecraft experience quite like alt-tabbing out of a
//! native launcher, opening a web browser, waiting through countdown timers, and admiring
//! high-value banner advertisements for every single third-party-restricted mod.
//!
//! Since CurseForge's API thoughtfully omits download URLs to safeguard this vital user
//! journey, we serialize these files here so users can savor the manual download process
//! at their own leisure.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader},
    path::{Path, PathBuf},
};

use crate::{CoreError, Result};
use acl_minecraft::ManualModpackFile;
use md5::{Digest, Md5};
use sha1::Sha1;
use tracing::{info, warn};

pub const MANUAL_DOWNLOADS_FILE: &str = "manual_downloads.json";

fn hash_file<D>(path: &Path) -> io::Result<String>
where
    D: Digest + io::Write,
{
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut hasher = D::new();
    io::copy(&mut reader, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn hash_file_sha1(path: &Path) -> io::Result<String> {
    hash_file::<Sha1>(path)
}

fn hash_file_md5(path: &Path) -> io::Result<String> {
    hash_file::<Md5>(path)
}

#[derive(Default)]
struct FileHashCache {
    sha1: HashMap<PathBuf, Option<String>>,
    md5: HashMap<PathBuf, Option<String>>,
}

impl FileHashCache {
    fn sha1(&mut self, path: &Path) -> Option<&str> {
        self.sha1
            .entry(path.to_path_buf())
            .or_insert_with(|| match hash_file_sha1(path) {
                Ok(h) => Some(h),
                Err(err) => {
                    warn!("Failed to compute SHA-1 for {}: {err}", path.display());
                    None
                }
            })
            .as_deref()
    }

    fn md5(&mut self, path: &Path) -> Option<&str> {
        self.md5
            .entry(path.to_path_buf())
            .or_insert_with(|| match hash_file_md5(path) {
                Ok(h) => Some(h),
                Err(err) => {
                    warn!("Failed to compute MD5 for {}: {err}", path.display());
                    None
                }
            })
            .as_deref()
    }

    fn matches(&mut self, entry: &ManualModpackFile, path: &Path, len: u64) -> bool {
        let expected_sha1 = entry
            .sha1
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let expected_md5 = entry
            .md5
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if expected_sha1.is_none() && expected_md5.is_none() {
            return false;
        }

        // Check the length first to avoid unnecessary hashing
        let Some(expected_len) = entry.file_length else {
            return false;
        };
        if len != expected_len {
            return false;
        }

        if let Some(expected) = expected_sha1 {
            return self
                .sha1(path)
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected));
        }

        if let Some(expected) = expected_md5 {
            return self
                .md5(path)
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected));
        }
        false
    }
}

fn list_regular_files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => {
            warn!("Failed to read directory {}: {err}", dir.display());
            return Vec::new();
        }
    };

    let mut files = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();

        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };

        if meta.is_file() {
            files.push((path, meta.len()));
        }
    }

    files
}

fn path(instance_dir: &Path) -> PathBuf {
    instance_dir.join(MANUAL_DOWNLOADS_FILE)
}

pub fn exists(instance_dir: &Path) -> bool {
    path(instance_dir).exists()
}

pub fn load(instance_dir: &Path) -> Result<Vec<ManualModpackFile>> {
    let path = path(instance_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path).map_err(|source| CoreError::Io { path, source })?;

    let list = serde_json::from_reader(file)?;

    Ok(list)
}

pub fn save(instance_dir: &Path, files: &[ManualModpackFile]) -> Result<()> {
    // Avoid empty JSON files
    if files.is_empty() {
        return clear(instance_dir);
    }

    let path = path(instance_dir);

    let content = serde_json::to_string_pretty(files)?;

    acl_common::fs::write_atomic(&path, content.as_bytes()).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;

    info!("Saved {} successfully", path.display());

    Ok(())
}

pub fn clear(instance_dir: &Path) -> Result<()> {
    let path = path(instance_dir);
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(&path).map_err(|source| CoreError::Io {
        path: path.clone(),
        source,
    })?;

    info!("Removed {} successfully", path.display());

    Ok(())
}

/// Scans the instance directory (mods / resourcepacks / shaderpacks),
/// and filters out complete downloads based on size and SHA-1/MD5 checksums.
pub fn find_missing(
    instance_dir: &Path,
    files: &[ManualModpackFile],
) -> Result<Vec<ManualModpackFile>> {
    let mut file_cache = FileHashCache::default();
    let mut missing = Vec::new();
    for file in files {
        let rel_path = acl_minecraft::modpack::normalize_relative_path(&file.target_dir)?;

        let dir = instance_dir.join(rel_path);

        let candidates = list_regular_files(&dir);

        let found = candidates
            .iter()
            .any(|(path, len)| file_cache.matches(file, path, *len));

        if !found {
            missing.push(file.clone());
        }
    }

    Ok(missing)
}

/// For drag-and-drop and `FileDialog::open_multiple`:
/// Matches external files against pending items by size and hash, copying matches
/// to the destination directory. Returns the count of successfully imported files.
pub fn import_files(
    instance_dir: &Path,
    pending: &[ManualModpackFile],
    sources: &[PathBuf],
) -> Result<u32> {
    let mut file_cache = FileHashCache::default();

    let mut imported: u32 = 0;
    for entry in pending {
        for src in sources {
            let Ok(meta) = fs::metadata(src) else {
                continue;
            };

            if !meta.is_file() {
                continue;
            }

            if file_cache.matches(entry, src, meta.len()) {
                let rel_path = acl_minecraft::modpack::normalize_relative_path(&entry.target_dir)?;
                let target_dir = instance_dir.join(rel_path);
                fs::create_dir_all(&target_dir).map_err(|source| CoreError::Io {
                    path: target_dir.clone(),
                    source,
                })?;

                let Some(name) = &entry.file_name else {
                    continue;
                };

                let clean_name = acl_minecraft::modpack::normalize_relative_path(name)?;
                if clean_name.contains('/') {
                    continue;
                }
                let dest = target_dir.join(clean_name);

                // Resolve symlinks to avoid interference

                let canon_src = fs::canonicalize(src).map_err(|source| CoreError::Io {
                    path: src.to_path_buf(),
                    source,
                })?;

                let same_file =
                    fs::canonicalize(&dest).is_ok_and(|canon_dest| canon_dest == canon_src);

                if !same_file {
                    fs::copy(src, &dest).map_err(|source| CoreError::Io { path: dest, source })?;
                }
                imported = imported.saturating_add(1);
                break;
            }
        }
    }

    Ok(imported)
}

/// Used by `FileDialog::select_folder`:
/// Lists regular files in `scan_dir` and delegates directly to `import_files`.
pub fn import_from_dir(
    instance_dir: &Path,
    pending: &[ManualModpackFile],
    scan_dir: &Path,
) -> Result<u32> {
    let candidates: Vec<PathBuf> = list_regular_files(scan_dir)
        .into_iter()
        .map(|(path, _)| path)
        .collect();

    import_files(instance_dir, pending, &candidates)
}
