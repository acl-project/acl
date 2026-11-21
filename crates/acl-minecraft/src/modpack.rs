// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::path::Component;
use std::path::Path;

use tracing::warn;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::MinecraftError;
use crate::ProjectHost;
use crate::Result;
use crate::download::DownloadTask;
use crate::mrpack::MRPACK_INDEX_FILE;
use crate::mrpack::MrpackIndex;

const MAX_INDEX_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModpackLoader {
    Fabric(String),
    Quilt(String),
    Forge(String),
    NeoForge(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModpackFileSource {
    /// Modrinth: The download address, target path, and verification information are all known
    Direct {
        /// Verified relative instance path, such as "mods/sodium.jar"
        path: String,
        downloads: Vec<String>,
        size: u64,
        sha1: Option<String>,
        sha512: Option<String>,
    },
    /// CurseForge: Adopts a minimalist design by omitting trivial details like the filename, path, URL, or hash.
    /// Please beg the API for everything else.
    CurseForge { project_id: u64, file_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackFile {
    pub source: ModpackFileSource,
    /// False indicates optional files (Modrinth env.client == optional or CurseForge required == false)
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModpackManifest {
    pub host: ProjectHost,
    pub name: String,
    pub version: String,
    /// Modrinth has summary; CurseForge is usually None
    pub summary: Option<String>,
    /// CurseForge has author; Modrinth is usually None
    pub author: Option<String>,
    /// Minecraft version, such as "1.20.1"
    pub game_version: String,
    /// None means Vanilla package
    pub loader: Option<ModpackLoader>,
    /// CurseForge optional recommended memory (MB); Modrinth is None
    pub recommended_ram_mb: Option<u32>,
    /// List of overlay directories within the ZIP that need to be decompressed to the instance root directory in order:
    /// - Modrinth: ["overrides", "client-overrides"]
    /// - CurseForge: [manifest.overrides] (usually ["overrides"])
    pub override_dirs: Vec<String>,
    /// List of remote files that the client needs to process (the entries with client == unsupported in Modrinth have been filtered out)
    pub files: Vec<ModpackFile>,
}

impl ModpackManifest {
    pub fn parse_from_zip(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|source| MinecraftError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let mut archive = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
            path: path.to_path_buf(),
            source,
        })?;

        match archive.by_name(MRPACK_INDEX_FILE) {
            Ok(entry) => {
                let mut content =
                    Vec::with_capacity(usize::try_from(entry.size().min(1024 * 1024)).unwrap_or(0));

                entry
                    .take(MAX_INDEX_SIZE.saturating_add(1))
                    .read_to_end(&mut content)
                    .map_err(|source| MinecraftError::Io {
                        path: path.to_path_buf(),
                        source,
                    })?;

                if content.len() as u64 > MAX_INDEX_SIZE {
                    return Err(MinecraftError::ModpackIndexTooLarge {
                        max_bytes: MAX_INDEX_SIZE,
                    });
                }

                let mrpack: MrpackIndex = serde_json::from_slice(&content)?;

                ModpackManifest::try_from(mrpack)
            }
            Err(ZipError::FileNotFound) => Err(MinecraftError::ModpackIndexNotFound {
                path: path.to_path_buf(),
            }),
            Err(source) => Err(MinecraftError::Zip {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    pub fn extract_overrides(&self, path: &Path, target_dir: &Path) -> Result<()> {
        let file = File::open(path).map_err(|source| MinecraftError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let mut archive = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
            path: path.to_path_buf(),
            source,
        })?;

        for dir in &self.override_dirs {
            let trimmed = dir.trim_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            let prefix = format!("{trimmed}/");

            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).map_err(|source| MinecraftError::Zip {
                    path: path.to_path_buf(),
                    source,
                })?;

                if entry.is_dir() || entry.is_symlink() {
                    continue;
                }

                let normalized_name = entry.name().replace('\\', "/");

                if normalized_name.ends_with('/') {
                    continue;
                }

                let Some(rel_raw) = normalized_name.strip_prefix(&prefix) else {
                    continue;
                };

                let rel_path = normalize_relative_path(rel_raw)?;

                let dest = target_dir.join(&rel_path);
                let Some(parent) = dest.parent() else {
                    continue;
                };

                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent).map_err(|source| MinecraftError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }

                let out_file = File::create(&dest).map_err(|source| MinecraftError::Io {
                    path: dest.to_path_buf(),
                    source,
                })?;

                let mut writer = BufWriter::new(out_file);

                io::copy(&mut entry, &mut writer).map_err(|source| MinecraftError::Io {
                    path: dest.to_path_buf(),
                    source,
                })?;

                writer.flush().map_err(|source| MinecraftError::Io {
                    path: dest.to_path_buf(),
                    source,
                })?;
            }
        }

        Ok(())
    }

    /// Converts direct modpack file entries into download tasks targeting `game_dir`.
    /// Entries requiring external API resolution (such as CurseForge) are skipped here.
    pub fn download_tasks(&self, game_dir: &Path, include_optional: bool) -> Vec<DownloadTask> {
        let mut tasks = Vec::with_capacity(self.files.len());

        for file in &self.files {
            if !include_optional && !file.required {
                continue;
            }

            let ModpackFileSource::Direct {
                path,
                downloads,
                size,
                sha1,
                ..
            } = &file.source
            else {
                continue;
            };

            let rel_path = match normalize_relative_path(path) {
                Ok(p) => p,
                Err(err) => {
                    warn!("Skipping modpack file with unsafe path '{path}': {err}");
                    continue;
                }
            };
            let Some(url) = downloads.iter().find(|u| !u.trim().is_empty()) else {
                warn!("Skipping modpack file '{rel_path}' with no valid download URL");
                continue;
            };

            tasks.push(DownloadTask {
                name: rel_path.clone(),
                url: url.clone(),
                destination: game_dir.join(&rel_path),
                sha1: sha1.clone().unwrap_or_default(),
                size: *size,
            });
        }

        tasks
    }
}

/// The final download file after parsing the URL
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModpackFile {
    /// Path relative to the instance directory (CurseForge is mapped to mods/, resourcepacks/, shaderpacks/, etc. by classId)
    pub path: String,
    /// List of candidate download links (in order fallback)
    pub urls: Vec<String>,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

/// Validates and normalizes a relative modpack file path (prevents path traversal).
pub fn normalize_relative_path(raw: &str) -> Result<String> {
    let replaced = raw.replace('\\', "/");
    let mut parts = Vec::new();

    for component in Path::new(&replaced).components() {
        match component {
            Component::Normal(part) => {
                let Some(s) = part.to_str() else {
                    return Err(MinecraftError::ModpackUnsafePath {
                        path: raw.to_string(),
                    });
                };
                // Reject Windows drive prefix like "C:" on Unix hosts
                if parts.is_empty() && s.contains(':') {
                    return Err(MinecraftError::ModpackUnsafePath {
                        path: raw.to_string(),
                    });
                }
                parts.push(s);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MinecraftError::ModpackUnsafePath {
                    path: raw.to_string(),
                });
            }
        }
    }

    if parts.is_empty() {
        return Err(MinecraftError::ModpackEmptyPath);
    }

    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_modpack_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
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

    fn create_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("failed to create zip file");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        for (name, content) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, options)
                    .expect("failed to add directory to zip");
            } else {
                zip.start_file(*name, options)
                    .expect("failed to start zip file");
                zip.write_all(content)
                    .expect("failed to write zip file content");
            }
        }

        zip.finish().expect("failed to finish zip");
    }

    fn sample_index_json() -> &'static [u8] {
        br#"{
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "2.1.0",
            "name": "Test Pack",
            "summary": "Integration test pack",
            "files": [
                {
                    "path": "mods/sodium.jar",
                    "hashes": { "sha1": "abc123", "sha512": "def456" },
                    "downloads": ["https://cdn.modrinth.com/sodium.jar"],
                    "fileSize": 2048
                }
            ],
            "dependencies": {
                "minecraft": "1.20.1",
                "fabric-loader": "0.15.11"
            }
        }"#
    }

    #[test]
    fn test_normalize_relative_path() {
        assert_eq!(
            normalize_relative_path("mods/sodium.jar").expect("valid path"),
            "mods/sodium.jar"
        );
        assert_eq!(
            normalize_relative_path("mods\\sub\\a.jar").expect("valid backslash path"),
            "mods/sub/a.jar"
        );
        assert_eq!(
            normalize_relative_path("./config/a.toml").expect("valid ./ path"),
            "config/a.toml"
        );

        let unsafe_cases = [
            "../escape.jar",
            "mods/../../escape.jar",
            "/etc/passwd",
            "\\Windows\\System32",
            "C:/mods/a.jar",
            "C:\\mods\\a.jar",
        ];
        for raw in unsafe_cases {
            assert!(
                matches!(
                    normalize_relative_path(raw),
                    Err(MinecraftError::ModpackUnsafePath { ref path }) if path == raw
                ),
                "expected ModpackUnsafePath for '{raw}'"
            );
        }

        for raw in ["", ".", "./"] {
            assert!(
                matches!(
                    normalize_relative_path(raw),
                    Err(MinecraftError::ModpackEmptyPath)
                ),
                "expected ModpackEmptyPath for '{raw}'"
            );
        }
    }

    #[test]
    fn test_parse_from_zip_success_and_errors() {
        let temp = TempDirGuard::new("parse_zip");

        let valid_zip = temp.path().join("valid.mrpack");
        create_test_zip(&valid_zip, &[(MRPACK_INDEX_FILE, sample_index_json())]);

        let manifest = ModpackManifest::parse_from_zip(&valid_zip)
            .expect("valid mrpack zip should parse successfully");
        assert_eq!(manifest.host, ProjectHost::Modrinth);
        assert_eq!(manifest.name, "Test Pack");
        assert_eq!(manifest.version, "2.1.0");
        assert_eq!(manifest.summary.as_deref(), Some("Integration test pack"));
        assert_eq!(manifest.game_version, "1.20.1");
        assert_eq!(
            manifest.loader,
            Some(ModpackLoader::Fabric("0.15.11".to_string()))
        );
        assert_eq!(manifest.files.len(), 1);

        // Non-existent archive
        let missing_zip = temp.path().join("missing.mrpack");
        assert!(matches!(
            ModpackManifest::parse_from_zip(&missing_zip),
            Err(MinecraftError::Io { ref path, .. }) if path == &missing_zip
        ));

        // Corrupt non-zip bytes
        let corrupt_zip = temp.path().join("corrupt.mrpack");
        fs::write(&corrupt_zip, b"not a zip archive").expect("failed to write corrupt zip");
        assert!(matches!(
            ModpackManifest::parse_from_zip(&corrupt_zip),
            Err(MinecraftError::Zip { ref path, .. }) if path == &corrupt_zip
        ));

        // Missing modrinth.index.json
        let no_index_zip = temp.path().join("no_index.mrpack");
        create_test_zip(&no_index_zip, &[("overrides/options.txt", b"fov:90")]);
        assert!(matches!(
            ModpackManifest::parse_from_zip(&no_index_zip),
            Err(MinecraftError::ModpackIndexNotFound { ref path }) if path == &no_index_zip
        ));

        // Invalid JSON inside modrinth.index.json
        let bad_json_zip = temp.path().join("bad_json.mrpack");
        create_test_zip(&bad_json_zip, &[(MRPACK_INDEX_FILE, b"{ invalid json")]);
        assert!(matches!(
            ModpackManifest::parse_from_zip(&bad_json_zip),
            Err(MinecraftError::Json(_))
        ));
    }

    #[test]
    fn test_extract_overrides_priority_and_filtering() {
        let temp = TempDirGuard::new("extract_overrides");
        let zip_path = temp.path().join("pack.mrpack");
        let target_dir = temp.path().join("instance");
        fs::create_dir_all(&target_dir).expect("failed to create target dir");

        // Place client-overrides before overrides in the archive to ensure
        // extraction order strictly follows `override_dirs`.
        create_test_zip(
            &zip_path,
            &[
                (MRPACK_INDEX_FILE, sample_index_json()),
                ("client-overrides/config/common.toml", b"client"),
                ("client-overrides/options.txt", b"fov:90"),
                ("overrides/", b""),
                ("overrides/config/common.toml", b"base"),
                ("overrides/config/base_only.toml", b"only_base"),
                ("server-overrides/server.properties", b"server"),
            ],
        );

        let manifest =
            ModpackManifest::parse_from_zip(&zip_path).expect("failed to parse test modpack");
        manifest
            .extract_overrides(&zip_path, &target_dir)
            .expect("failed to extract overrides");

        assert_eq!(
            fs::read(target_dir.join("config/common.toml")).expect("common.toml should exist"),
            b"client"
        );
        assert_eq!(
            fs::read(target_dir.join("config/base_only.toml"))
                .expect("base_only.toml should exist"),
            b"only_base"
        );
        assert_eq!(
            fs::read(target_dir.join("options.txt")).expect("options.txt should exist"),
            b"fov:90"
        );
        assert!(!target_dir.join("server.properties").exists());
        assert!(!target_dir.join(MRPACK_INDEX_FILE).exists());
    }

    #[test]
    fn test_extract_overrides_rejects_unsafe_path() {
        let temp = TempDirGuard::new("extract_unsafe");
        let zip_path = temp.path().join("unsafe.mrpack");
        let target_dir = temp.path().join("instance");
        fs::create_dir_all(&target_dir).expect("failed to create target dir");

        create_test_zip(
            &zip_path,
            &[
                (MRPACK_INDEX_FILE, sample_index_json()),
                ("overrides/../escape.txt", b"evil"),
            ],
        );

        let manifest =
            ModpackManifest::parse_from_zip(&zip_path).expect("failed to parse test modpack");
        let err = manifest
            .extract_overrides(&zip_path, &target_dir)
            .expect_err("unsafe override path should be rejected");
        assert!(matches!(
            err,
            MinecraftError::ModpackUnsafePath { ref path } if path == "../escape.txt"
        ));
        assert!(!temp.path().join("escape.txt").exists());
    }
}
