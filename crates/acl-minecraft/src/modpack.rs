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

use zip::ZipArchive;
use zip::result::ZipError;

use crate::MinecraftError;
use crate::ProjectHost;
use crate::Result;
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
    /// false indicates optional files (Modrinth env.client == optional or CurseForge required == false)
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
    ///Minecraft version, such as "1.20.1"
    pub game_version: String,
    /// None means  Vanilla package
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
    pub fn parse_modpack_from_zip(path: &Path) -> Result<Self> {
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
            Err(ZipError::FileNotFound) => Err(MinecraftError::IndexJsonNotFound {
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

                if entry.is_dir() {
                    continue;
                }

                let normalized_name = entry.name().replace('\\', "/");
                let Some(rel_raw) = normalized_name.strip_prefix(&prefix) else {
                    continue;
                };

                let rel_path = normalize_relative_path(rel_raw)?;

                let dest = target_dir.join(&rel_path);
                let Some(parent) = dest.parent() else {
                    continue;
                };
                fs::create_dir_all(parent).map_err(|source| MinecraftError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;

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
