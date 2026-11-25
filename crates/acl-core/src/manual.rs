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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use acl_minecraft::ManualDownloadReason;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(prefix: &str) -> Self {
            let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "acl_core_manual_{}_{}_{}",
                prefix,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create test temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn md5_hex(bytes: &[u8]) -> String {
        let mut hasher = Md5::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn sample_entry(
        project_id: u64,
        file_id: u64,
        file_name: Option<&str>,
        target_dir: &str,
        file_length: Option<u64>,
        sha1: Option<String>,
        md5: Option<String>,
    ) -> ManualModpackFile {
        ManualModpackFile {
            project_id,
            file_id,
            project_name: Some(format!("Project {project_id}")),
            file_name: file_name.map(str::to_string),
            target_dir: target_dir.to_string(),
            page_url: format!("https://www.curseforge.com/projects/{project_id}/files/{file_id}"),
            required: true,
            reason: ManualDownloadReason::DistributionDisabled,
            file_length,
            sha1,
            md5,
        }
    }

    #[test]
    fn test_manual_persistence_lifecycle() {
        let temp = TempDirGuard::new("persistence");
        let dir = temp.path();

        // Missing file reports exists == false and loads as empty
        assert!(!exists(dir));
        assert!(load(dir).expect("load missing").is_empty());

        // Clear on non-existent file is a no-op
        clear(dir).expect("clear missing");

        // Save non-empty list and load back
        let entries = vec![sample_entry(
            10,
            100,
            Some("alpha.jar"),
            "mods",
            Some(128),
            Some("abc123".to_string()),
            Some("def456".to_string()),
        )];
        save(dir, &entries).expect("save entries");
        assert!(exists(dir));
        assert_eq!(load(dir).expect("load saved"), entries);

        // Saving an empty slice removes manual_downloads.json
        save(dir, &[]).expect("save empty removes file");
        assert!(!exists(dir));
        assert!(load(dir).expect("load after empty save").is_empty());

        // Clear removes an existing file
        save(dir, &entries).expect("save again");
        assert!(exists(dir));
        clear(dir).expect("clear existing");
        assert!(!exists(dir));

        // Corrupt JSON returns CoreError::Json
        fs::write(dir.join(MANUAL_DOWNLOADS_FILE), b"{ invalid json").expect("write corrupt json");
        assert!(matches!(load(dir), Err(CoreError::Json(_))));
    }

    #[test]
    fn test_find_missing_checksum_and_length_matching() {
        let temp = TempDirGuard::new("find_missing");
        let dir = temp.path();
        let mods_dir = dir.join("mods");
        let rp_dir = dir.join("resourcepacks");
        fs::create_dir_all(&mods_dir).expect("create mods dir");
        fs::create_dir_all(&rp_dir).expect("create resourcepacks dir");
        // Nested directory inside mods/ must be ignored
        fs::create_dir_all(mods_dir.join("nested_subdir")).expect("create nested subdir");

        let sha1_bytes = b"mod matched by sha1";
        let md5_bytes = b"pack matched by md5 only";
        let wrong_sha1_bytes = b"file with matching md5 but wrong sha1";

        fs::write(mods_dir.join("any_local_name.jar"), sha1_bytes).expect("write sha1 mod");
        fs::write(rp_dir.join("local_pack.zip"), md5_bytes).expect("write md5 pack");
        fs::write(mods_dir.join("mismatch.jar"), wrong_sha1_bytes).expect("write mismatch jar");

        let matched_sha1 = sample_entry(
            1,
            101,
            Some("expected_a.jar"),
            "mods",
            Some(sha1_bytes.len() as u64),
            // Uppercase + surrounding spaces must still match
            Some(format!("  {}  ", sha1_hex(sha1_bytes).to_uppercase())),
            None,
        );
        let matched_md5 = sample_entry(
            2,
            102,
            Some("expected_b.zip"),
            "resourcepacks",
            Some(md5_bytes.len() as u64),
            Some("   ".to_string()),
            Some(format!(" {} ", md5_hex(md5_bytes).to_uppercase())),
        );
        // SHA-1 is present but wrong -> must NOT fall back to MD5
        let wrong_sha1_valid_md5 = sample_entry(
            3,
            103,
            Some("mismatch.jar"),
            "mods",
            Some(wrong_sha1_bytes.len() as u64),
            Some("0000000000000000000000000000000000000000".to_string()),
            Some(md5_hex(wrong_sha1_bytes)),
        );
        // Length mismatch -> treated as missing even when hash matches
        let wrong_len = sample_entry(
            4,
            104,
            Some("wrong_len.jar"),
            "mods",
            Some(sha1_bytes.len() as u64 + 1),
            Some(sha1_hex(sha1_bytes)),
            None,
        );
        // Missing file_length -> treated as missing
        let missing_len = sample_entry(
            5,
            105,
            Some("no_len.jar"),
            "mods",
            None,
            Some(sha1_hex(sha1_bytes)),
            Some(md5_hex(sha1_bytes)),
        );
        // Missing both hashes -> treated as missing
        let no_hashes = sample_entry(
            6,
            106,
            Some("no_hashes.jar"),
            "mods",
            Some(sha1_bytes.len() as u64),
            None,
            Some("   ".to_string()),
        );
        // Target directory does not exist -> treated as missing, not error
        let missing_dir_entry = sample_entry(
            7,
            107,
            Some("shader.zip"),
            "shaderpacks",
            Some(sha1_bytes.len() as u64),
            Some(sha1_hex(sha1_bytes)),
            None,
        );

        let all = vec![
            matched_sha1,
            matched_md5,
            wrong_sha1_valid_md5.clone(),
            wrong_len.clone(),
            missing_len.clone(),
            no_hashes.clone(),
            missing_dir_entry.clone(),
        ];

        let missing = find_missing(dir, &all).expect("find_missing should succeed");
        assert_eq!(
            missing,
            vec![
                wrong_sha1_valid_md5,
                wrong_len,
                missing_len,
                no_hashes,
                missing_dir_entry,
            ]
        );

        // Unsafe target_dir must fail with an error
        let unsafe_entry = sample_entry(
            8,
            108,
            Some("escape.jar"),
            "../escape",
            Some(10),
            Some("deadbeef".to_string()),
            None,
        );
        assert!(find_missing(dir, &[unsafe_entry]).is_err());
    }

    #[test]
    fn test_import_files_and_import_from_dir() {
        let temp = TempDirGuard::new("import");
        let instance_dir = temp.path().join("instance");
        let downloads_dir = temp.path().join("downloads");
        fs::create_dir_all(&instance_dir).expect("create instance dir");
        fs::create_dir_all(&downloads_dir).expect("create downloads dir");
        fs::create_dir_all(downloads_dir.join("ignored_subdir")).expect("create downloads subdir");

        let mod_bytes = b"restricted mod payload for sha1 import";
        let pack_bytes = b"restricted resourcepack payload for md5 import";
        let unrelated_bytes = b"unrelated downloaded file";

        let src_mod = downloads_dir.join("browser-renamed-mod.jar");
        let src_pack = downloads_dir.join("browser-renamed-pack.zip");
        let src_unrelated = downloads_dir.join("notes.txt");
        fs::write(&src_mod, mod_bytes).expect("write src mod");
        fs::write(&src_pack, pack_bytes).expect("write src pack");
        fs::write(&src_unrelated, unrelated_bytes).expect("write unrelated file");

        let mod_entry = sample_entry(
            10,
            100,
            Some("expected-mod.jar"),
            "mods",
            Some(mod_bytes.len() as u64),
            Some(sha1_hex(mod_bytes)),
            None,
        );
        let pack_entry = sample_entry(
            20,
            200,
            Some("expected-pack.zip"),
            "resourcepacks",
            Some(pack_bytes.len() as u64),
            None,
            Some(md5_hex(pack_bytes)),
        );
        let pending = vec![mod_entry.clone(), pack_entry.clone()];

        // import_files copies matching files into newly created target dirs and skips non-matching / dirs / missing sources
        let imported = import_files(
            &instance_dir,
            &pending,
            &[
                downloads_dir.join("nonexistent.jar"),
                downloads_dir.join("ignored_subdir"),
                src_unrelated.clone(),
                src_mod.clone(),
                src_pack.clone(),
            ],
        )
        .expect("import_files should succeed");
        assert_eq!(imported, 2);

        let dest_mod = instance_dir.join("mods/expected-mod.jar");
        let dest_pack = instance_dir.join("resourcepacks/expected-pack.zip");
        assert_eq!(fs::read(&dest_mod).expect("read copied mod"), mod_bytes);
        assert_eq!(fs::read(&dest_pack).expect("read copied pack"), pack_bytes);
        assert!(
            find_missing(&instance_dir, &pending)
                .expect("find_missing after import")
                .is_empty()
        );

        // Importing the destination file onto itself (directly and via symlink) must not truncate it
        let self_imported = import_files(
            &instance_dir,
            std::slice::from_ref(&mod_entry),
            std::slice::from_ref(&dest_mod),
        )
        .expect("self import should succeed");
        assert_eq!(self_imported, 1);
        assert_eq!(
            fs::read(&dest_mod).expect("read mod after self import"),
            mod_bytes
        );

        #[cfg(unix)]
        {
            let link_path = downloads_dir.join("symlink_to_dest.jar");
            std::os::unix::fs::symlink(&dest_mod, &link_path).expect("create symlink");
            let sym_imported = import_files(
                &instance_dir,
                std::slice::from_ref(&mod_entry),
                &[link_path],
            )
            .expect("symlink self import should succeed");
            assert_eq!(sym_imported, 1);
            assert_eq!(
                fs::read(&dest_mod).expect("read mod after symlink self import"),
                mod_bytes
            );
        }

        // Entries with None file_name or nested path in file_name are skipped; unsafe file_name errors
        let no_name_entry = sample_entry(
            30,
            300,
            None,
            "mods",
            Some(mod_bytes.len() as u64),
            Some(sha1_hex(mod_bytes)),
            None,
        );
        let nested_name_entry = sample_entry(
            31,
            301,
            Some("sub/nested.jar"),
            "mods",
            Some(mod_bytes.len() as u64),
            Some(sha1_hex(mod_bytes)),
            None,
        );
        let skipped_count = import_files(
            &instance_dir,
            &[no_name_entry, nested_name_entry],
            std::slice::from_ref(&src_mod),
        )
        .expect("entries with skipped names should return Ok(0)");
        assert_eq!(skipped_count, 0);
        assert!(!instance_dir.join("mods/sub/nested.jar").exists());

        let unsafe_name_entry = sample_entry(
            32,
            302,
            Some("../escape.jar"),
            "mods",
            Some(mod_bytes.len() as u64),
            Some(sha1_hex(mod_bytes)),
            None,
        );
        assert!(import_files(&instance_dir, &[unsafe_name_entry], &[src_mod]).is_err());

        // import_from_dir imports from scan_dir into a fresh instance dir and handles missing scan_dir
        let fresh_instance = temp.path().join("fresh_instance");
        fs::create_dir_all(&fresh_instance).expect("create fresh instance dir");

        let from_missing = import_from_dir(
            &fresh_instance,
            &pending,
            &temp.path().join("does_not_exist"),
        )
        .expect("import_from_dir on missing dir should return 0");
        assert_eq!(from_missing, 0);

        let from_dir = import_from_dir(&fresh_instance, &pending, &downloads_dir)
            .expect("import_from_dir should import matching files");
        assert_eq!(from_dir, 2);
        assert!(
            find_missing(&fresh_instance, &pending)
                .expect("find_missing on fresh instance")
                .is_empty()
        );
    }
}
