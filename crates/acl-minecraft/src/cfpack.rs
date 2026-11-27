// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    MinecraftError, ModpackFile, ModpackFileSource, ModpackLoader, ModpackManifest, ProjectHost,
    ProjectType, ResolvedModpackFile, Result,
    curseforge::{self, ModFile, ModInfo},
    modpack::{
        ManualDownloadReason, ManualModpackFile, ModpackResolution, normalize_relative_path,
    },
};

pub const CFPACK_MANIFEST_FILE: &str = "manifest.json";
pub const CFPACK_MANIFEST_TYPE: &str = "minecraftModpack";
pub const CFPACK_MANIFEST_VERSION: u32 = 1;
pub const CFPACK_DEFAULT_OVERRIDES: &str = "overrides";

const fn default_true() -> bool {
    true
}

fn default_overrides() -> String {
    CFPACK_DEFAULT_OVERRIDES.to_string()
}

fn curseforge_target_dir(class_id: Option<u32>) -> &'static str {
    match ProjectType::from_curseforge_class_id(class_id) {
        ProjectType::ResourcePack => "resourcepacks",
        ProjectType::Shader => "shaderpacks",
        ProjectType::Mod | ProjectType::Modpack => "mods",
    }
}

fn curseforge_file_page_url(mod_info: Option<&ModInfo>, project_id: u64, file_id: u64) -> String {
    if let Some(base) = mod_info.and_then(ModInfo::website_url) {
        format!("{base}/files/{file_id}")
    } else {
        format!("https://www.curseforge.com/projects/{project_id}")
    }
}

fn sanitize_curseforge_file_name(raw: &str) -> Result<String> {
    let normalized = normalize_relative_path(raw)?;
    if normalized.contains('/') {
        return Err(MinecraftError::ModpackUnsafePath {
            path: raw.to_string(),
        });
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackModLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackMinecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<CfpackModLoader>,
    #[serde(default)]
    pub recommended_ram: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CfpackFile {
    // Uppercase ID is not Id
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "default_true")]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfpackManifest {
    pub minecraft: CfpackMinecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub files: Vec<CfpackFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

// Apparently having separate "name" and "version" fields in JSON was asking for too much.
fn parse_cfpack_loader_id(id: &str) -> Result<ModpackLoader> {
    let trimmed = id.trim();
    let Some((raw_kind, raw_version)) = trimmed.split_once('-') else {
        return Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        });
    };

    let kind = raw_kind.trim().to_ascii_lowercase();
    let version = raw_version.trim();

    if !matches!(kind.as_str(), "forge" | "neoforge" | "fabric" | "quilt") {
        return Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        });
    }

    if version.is_empty() {
        return Err(MinecraftError::ModpackEmptyDependencyVersion { dependency: kind });
    }

    let version = version.to_string();

    match kind.as_str() {
        "forge" => Ok(ModpackLoader::Forge(version)),
        "neoforge" => Ok(ModpackLoader::NeoForge(version)),
        "fabric" => Ok(ModpackLoader::Fabric(version)),
        "quilt" => Ok(ModpackLoader::Quilt(version)),
        _ => Err(MinecraftError::ModpackUnsupportedDependency {
            dependency: trimmed.to_string(),
        }),
    }
}

fn parse_cfpack_mod_loaders(mod_loaders: &[CfpackModLoader]) -> Result<Option<ModpackLoader>> {
    if mod_loaders.is_empty() {
        return Ok(None);
    }

    let mut primaries = mod_loaders.iter().filter(|loader| loader.primary);

    let selected = match (primaries.next(), primaries.next()) {
        // Exactly one primary loader
        (Some(primary), None) => primary,
        // Multiple primary loaders
        (Some(_), Some(_)) => return Err(MinecraftError::ModpackMultipleModLoaders),
        // No primary flag set: fall back only if there is a single loader entry
        (None, _) => match mod_loaders {
            [only] => only,
            _ => return Err(MinecraftError::ModpackMultipleModLoaders),
        },
    };

    let loader = parse_cfpack_loader_id(&selected.id)?;
    Ok(Some(loader))
}

impl TryFrom<CfpackManifest> for ModpackManifest {
    type Error = MinecraftError;

    fn try_from(manifest: CfpackManifest) -> Result<Self> {
        if manifest.manifest_version != CFPACK_MANIFEST_VERSION {
            return Err(MinecraftError::ModpackUnsupportedFormatVersion {
                version: manifest.manifest_version,
            });
        }

        if manifest.manifest_type != CFPACK_MANIFEST_TYPE {
            return Err(MinecraftError::ModpackUnsupportedGame {
                game: manifest.manifest_type,
            });
        }

        let game_version = manifest.minecraft.version.trim();
        if game_version.is_empty() {
            return Err(MinecraftError::ModpackMissingMinecraftVersion);
        }

        let loader = parse_cfpack_mod_loaders(&manifest.minecraft.mod_loaders)?;

        let override_dirs = {
            let trimmed = manifest.overrides.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![normalize_relative_path(trimmed)?]
            }
        };

        let author = manifest
            .author
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty());

        let recommended_ram_mb = manifest.minecraft.recommended_ram.filter(|&ram| ram > 0);

        let files = manifest
            .files
            .into_iter()
            .map(|file| ModpackFile {
                source: ModpackFileSource::CurseForge {
                    project_id: file.project_id,
                    file_id: file.file_id,
                },
                required: file.required,
            })
            .collect();

        Ok(Self {
            host: ProjectHost::CurseForge,
            name: manifest.name,
            version: manifest.version,
            summary: None,
            author,
            game_version: game_version.to_string(),
            loader,
            recommended_ram_mb,
            override_dirs,
            files,
        })
    }
}

pub async fn resolve_files(
    entries: &[CfpackFile],
    client: &reqwest::Client,
    cache_dir: &Path,
) -> Result<ModpackResolution> {
    if entries.is_empty() {
        return Ok(ModpackResolution::default());
    }

    let file_ids: Vec<u64> = entries.iter().map(|e| e.file_id).collect();
    let project_ids: Vec<u64> = entries.iter().map(|e| e.project_id).collect();

    let (files, mods) = tokio::try_join!(
        curseforge::fetch_files_batch_with_cache(client, &file_ids, cache_dir, false),
        curseforge::fetch_projects_batch_with_cache(client, &project_ids, cache_dir, false),
    )?;

    let files_map: HashMap<u64, ModFile> = files.into_iter().map(|f| (f.id, f)).collect();
    let mods_map: HashMap<u64, ModInfo> = mods.into_iter().map(|m| (m.id, m)).collect();

    let mut resolution = ModpackResolution {
        files: Vec::with_capacity(entries.len()),
        manual_files: Vec::new(),
    };

    for entry in entries {
        let mod_info = mods_map.get(&entry.project_id);
        let project_name = mod_info.map(|m| m.name.clone());
        let target_dir = curseforge_target_dir(mod_info.and_then(|m| m.class_id));
        let page_url = curseforge_file_page_url(mod_info, entry.project_id, entry.file_id);

        let Some(mod_file) = files_map.get(&entry.file_id) else {
            warn!(
                "CurseForge file metadata not found for project {}, file {}",
                entry.project_id, entry.file_id
            );

            resolution.manual_files.push(ManualModpackFile {
                project_id: entry.project_id,
                file_id: entry.file_id,
                project_name,
                file_name: None,
                target_dir: target_dir.to_string(),
                page_url,
                required: entry.required,
                reason: ManualDownloadReason::MetadataNotFound,
                file_length: None,
                sha1: None,
                md5: None,
            });
            continue;
        };

        let Some(download_url) = mod_file
            .download_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
        else {
            warn!(
                "CurseForge file '{}' (project {}, file {}) requires manual download",
                mod_file.file_name, entry.project_id, entry.file_id
            );
            resolution.manual_files.push(ManualModpackFile {
                project_id: entry.project_id,
                file_id: entry.file_id,
                project_name,
                file_name: Some(mod_file.file_name.clone()),
                target_dir: target_dir.to_string(),
                page_url,
                required: entry.required,
                reason: ManualDownloadReason::DistributionDisabled,
                file_length: (mod_file.file_length > 0).then_some(mod_file.file_length),
                sha1: mod_file.sha1(),
                md5: mod_file.md5(),
            });
            continue;
        };

        let clean_name = sanitize_curseforge_file_name(&mod_file.file_name)?;
        let path = format!("{target_dir}/{clean_name}");

        resolution.files.push(ResolvedModpackFile {
            path,
            urls: vec![download_url.to_string()],
            size: mod_file.file_length,
            sha1: mod_file.sha1(),
            sha512: None,
        });
    }

    Ok(resolution)
}

#[cfg(test)]
mod tests {
    use acl_common::hash::url_sha1;

    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::cache::Cached;
    use crate::curseforge::{CURSEFORGE_API_BASE_URL, DEFAULT_SEARCH_TTL, FileHash};

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests serialize access via curseforge::tests::ENV_LOCK to avoid concurrent env modification.
            unsafe {
                std::env::set_var(key, val);
            }
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests serialize access via curseforge::tests::ENV_LOCK to avoid concurrent env modification.
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: Tests serialize access via curseforge::tests::ENV_LOCK to avoid concurrent env modification.
            unsafe {
                match &self.original {
                    Some(val) => std::env::set_var(self.key, val),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_cfpack_{}_{}_{}",
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

    #[derive(Serialize, Deserialize)]
    struct TestDataResponse<T> {
        data: T,
    }

    fn base_manifest(mod_loaders: Vec<CfpackModLoader>, files: Vec<CfpackFile>) -> CfpackManifest {
        CfpackManifest {
            minecraft: CfpackMinecraft {
                version: "1.20.1".to_string(),
                mod_loaders,
                recommended_ram: Some(4096),
            },
            manifest_type: CFPACK_MANIFEST_TYPE.to_string(),
            manifest_version: CFPACK_MANIFEST_VERSION,
            name: "Sample CF Pack".to_string(),
            version: "1.2.3".to_string(),
            author: Some("  PackAuthor  ".to_string()),
            files,
            overrides: CFPACK_DEFAULT_OVERRIDES.to_string(),
        }
    }

    fn sample_mod_info(id: u64, class_id: Option<u32>, name: &str) -> ModInfo {
        ModInfo {
            id,
            class_id,
            name: name.to_string(),
            slug: name.to_ascii_lowercase().replace(' ', "-"),
            summary: String::new(),
            download_count: 100,
            logo: None,
            authors: Vec::new(),
            categories: Vec::new(),
            latest_files_indexes: Vec::new(),
            allow_mod_distribution: Some(true),
            links: None,
        }
    }

    fn sample_mod_file(
        id: u64,
        mod_id: u64,
        file_name: &str,
        download_url: Option<&str>,
        sha1: Option<&str>,
    ) -> ModFile {
        ModFile {
            id,
            mod_id,
            display_name: file_name.to_string(),
            file_name: file_name.to_string(),
            release_type: 1,
            file_date: "2026-01-01T00:00:00Z".to_string(),
            file_length: 4096,
            download_count: 50,
            download_url: download_url.map(str::to_string),
            game_versions: vec!["1.20.1".to_string()],
            hashes: sha1
                .map(|v| {
                    vec![
                        FileHash {
                            value: "md5ignored".to_string(),
                            algo: 2,
                        },
                        FileHash {
                            value: v.to_string(),
                            algo: 1,
                        },
                    ]
                })
                .unwrap_or_default(),
            dependencies: Vec::new(),
        }
    }

    fn write_batch_cache<T: Serialize + serde::de::DeserializeOwned>(
        cache_dir: &Path,
        endpoint: &str,
        body: &serde_json::Value,
        data: Vec<T>,
    ) {
        let url = format!("{CURSEFORGE_API_BASE_URL}/{endpoint}");
        let cache_seed = format!("POST:{url}:{body}");
        let cache_file = cache_dir.join(url_sha1(&cache_seed));
        let resp = TestDataResponse { data };
        Cached::new(resp, DEFAULT_SEARCH_TTL)
            .expect("create batch cache entry")
            .save_to_file(&cache_file)
            .expect("save batch cache entry");
    }

    #[test]
    fn test_cfpack_manifest_deserialization_defaults() {
        let json = r#"{
            "minecraft": {
                "version": "1.20.1"
            },
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "Minimal Pack",
            "isLocked": false,
            "files": [
                {
                    "projectID": 238222,
                    "fileID": 5101366
                },
                {
                    "projectID": 306612,
                    "fileID": 5200000,
                    "required": false
                }
            ]
        }"#;

        let parsed: CfpackManifest =
            serde_json::from_str(json).expect("minimal cfpack manifest should deserialize");

        assert_eq!(parsed.minecraft.version, "1.20.1");
        assert!(parsed.minecraft.mod_loaders.is_empty());
        assert_eq!(parsed.minecraft.recommended_ram, None);
        assert_eq!(parsed.manifest_type, CFPACK_MANIFEST_TYPE);
        assert_eq!(parsed.manifest_version, CFPACK_MANIFEST_VERSION);
        assert_eq!(parsed.name, "Minimal Pack");
        assert_eq!(parsed.version, "");
        assert_eq!(parsed.author, None);
        assert_eq!(parsed.overrides, CFPACK_DEFAULT_OVERRIDES);
        assert_eq!(
            parsed.files,
            vec![
                CfpackFile {
                    project_id: 238_222,
                    file_id: 5_101_366,
                    required: true,
                },
                CfpackFile {
                    project_id: 306_612,
                    file_id: 5_200_000,
                    required: false,
                },
            ]
        );
    }

    #[test]
    fn test_cfpack_try_from_loaders_and_metadata() {
        let loader_cases = [
            (Vec::new(), None),
            (
                vec![CfpackModLoader {
                    id: "Forge-47.2.0".to_string(),
                    primary: true,
                }],
                Some(ModpackLoader::Forge("47.2.0".to_string())),
            ),
            (
                vec![CfpackModLoader {
                    id: "neoforge-21.1.72-beta".to_string(),
                    primary: true,
                }],
                Some(ModpackLoader::NeoForge("21.1.72-beta".to_string())),
            ),
            (
                vec![
                    CfpackModLoader {
                        id: "liteloader-1.12.2".to_string(),
                        primary: false,
                    },
                    CfpackModLoader {
                        id: "fabric-0.15.11".to_string(),
                        primary: true,
                    },
                ],
                Some(ModpackLoader::Fabric("0.15.11".to_string())),
            ),
            // Single loader with primary = false falls back to that loader
            (
                vec![CfpackModLoader {
                    id: "quilt-0.26.0".to_string(),
                    primary: false,
                }],
                Some(ModpackLoader::Quilt("0.26.0".to_string())),
            ),
        ];

        for (mod_loaders, expected_loader) in loader_cases {
            let files = vec![
                CfpackFile {
                    project_id: 100,
                    file_id: 1000,
                    required: true,
                },
                CfpackFile {
                    project_id: 200,
                    file_id: 2000,
                    required: false,
                },
            ];

            let mut cf = base_manifest(mod_loaders, files);
            cf.overrides = "custom\\overrides/".to_string();

            let manifest = ModpackManifest::try_from(cf)
                .expect("valid cfpack manifest should convert to ModpackManifest");

            assert_eq!(manifest.host, ProjectHost::CurseForge);
            assert_eq!(manifest.name, "Sample CF Pack");
            assert_eq!(manifest.version, "1.2.3");
            assert_eq!(manifest.summary, None);
            assert_eq!(manifest.author.as_deref(), Some("PackAuthor"));
            assert_eq!(manifest.game_version, "1.20.1");
            assert_eq!(manifest.loader, expected_loader);
            assert_eq!(manifest.recommended_ram_mb, Some(4096));
            assert_eq!(manifest.override_dirs, vec!["custom/overrides".to_string()]);
            assert_eq!(
                manifest.files,
                vec![
                    ModpackFile {
                        source: ModpackFileSource::CurseForge {
                            project_id: 100,
                            file_id: 1000,
                        },
                        required: true,
                    },
                    ModpackFile {
                        source: ModpackFileSource::CurseForge {
                            project_id: 200,
                            file_id: 2000,
                        },
                        required: false,
                    },
                ]
            );
        }

        // Blank author, zero recommended_ram, and blank overrides are normalized away
        let mut edge = base_manifest(Vec::new(), Vec::new());
        edge.author = Some("   ".to_string());
        edge.minecraft.recommended_ram = Some(0);
        edge.overrides = "   ".to_string();

        let edge_manifest =
            ModpackManifest::try_from(edge).expect("edge metadata should still be valid");
        assert_eq!(edge_manifest.author, None);
        assert_eq!(edge_manifest.recommended_ram_mb, None);
        assert!(edge_manifest.override_dirs.is_empty());
    }

    #[test]
    fn test_cfpack_try_from_validation_errors() {
        // Unsupported manifestVersion
        let mut bad_version = base_manifest(Vec::new(), Vec::new());
        bad_version.manifest_version = 2;
        assert!(matches!(
            ModpackManifest::try_from(bad_version),
            Err(MinecraftError::ModpackUnsupportedFormatVersion { version: 2 })
        ));

        // Unsupported manifestType
        let mut bad_type = base_manifest(Vec::new(), Vec::new());
        bad_type.manifest_type = "otherModpack".to_string();
        assert!(matches!(
            ModpackManifest::try_from(bad_type),
            Err(MinecraftError::ModpackUnsupportedGame { ref game }) if game == "otherModpack"
        ));

        // Blank minecraft.version
        let mut empty_mc = base_manifest(Vec::new(), Vec::new());
        empty_mc.minecraft.version = "   ".to_string();
        assert!(matches!(
            ModpackManifest::try_from(empty_mc),
            Err(MinecraftError::ModpackMissingMinecraftVersion)
        ));

        // Multiple primary loaders
        let multi_primary = base_manifest(
            vec![
                CfpackModLoader {
                    id: "forge-47.2.0".to_string(),
                    primary: true,
                },
                CfpackModLoader {
                    id: "fabric-0.15.11".to_string(),
                    primary: true,
                },
            ],
            Vec::new(),
        );
        assert!(matches!(
            ModpackManifest::try_from(multi_primary),
            Err(MinecraftError::ModpackMultipleModLoaders)
        ));

        // Multiple non-primary loaders
        let multi_non_primary = base_manifest(
            vec![
                CfpackModLoader {
                    id: "forge-47.2.0".to_string(),
                    primary: false,
                },
                CfpackModLoader {
                    id: "fabric-0.15.11".to_string(),
                    primary: false,
                },
            ],
            Vec::new(),
        );
        assert!(matches!(
            ModpackManifest::try_from(multi_non_primary),
            Err(MinecraftError::ModpackMultipleModLoaders)
        ));

        // Loader ID missing '-' separator
        let no_dash = base_manifest(
            vec![CfpackModLoader {
                id: "forge".to_string(),
                primary: true,
            }],
            Vec::new(),
        );
        assert!(matches!(
            ModpackManifest::try_from(no_dash),
            Err(MinecraftError::ModpackUnsupportedDependency { ref dependency })
                if dependency == "forge"
        ));

        // Unknown loader prefix
        let unknown_loader = base_manifest(
            vec![CfpackModLoader {
                id: "liteloader-1.12.2".to_string(),
                primary: true,
            }],
            Vec::new(),
        );
        assert!(matches!(
            ModpackManifest::try_from(unknown_loader),
            Err(MinecraftError::ModpackUnsupportedDependency { ref dependency })
                if dependency == "liteloader-1.12.2"
        ));

        // Known loader with empty version
        let empty_loader_ver = base_manifest(
            vec![CfpackModLoader {
                id: "forge-   ".to_string(),
                primary: true,
            }],
            Vec::new(),
        );
        assert!(matches!(
            ModpackManifest::try_from(empty_loader_ver),
            Err(MinecraftError::ModpackEmptyDependencyVersion { ref dependency })
                if dependency == "forge"
        ));

        // Unsafe overrides path
        for unsafe_override in ["../escape", "/etc/passwd"] {
            let mut bad_override = base_manifest(Vec::new(), Vec::new());
            bad_override.overrides = unsafe_override.to_string();
            assert!(matches!(
                ModpackManifest::try_from(bad_override),
                Err(MinecraftError::ModpackUnsafePath { ref path }) if path == unsafe_override
            ));
        }
    }

    #[tokio::test]
    async fn test_cfpack_resolve_files_from_cache() {
        let _lock = crate::curseforge::tests::ENV_LOCK.lock().await;
        let temp = TempDirGuard::new("resolve_files");
        let client = reqwest::Client::new();

        // Empty input returns empty resolution immediately even without an API key
        {
            let _guard = EnvGuard::remove("CURSEFORGE_API_KEY");
            let empty = resolve_files(&[], &client, temp.path())
                .await
                .expect("empty entries should succeed without API key");
            assert_eq!(empty, ModpackResolution::default());
        }

        let _guard = EnvGuard::set("CURSEFORGE_API_KEY", "test-cfpack-key");

        let entries = vec![
            // Mod (classId = 6) -> mods/
            CfpackFile {
                project_id: 10,
                file_id: 100,
                required: true,
            },
            // ResourcePack (classId = 12) -> resourcepacks/
            CfpackFile {
                project_id: 20,
                file_id: 200,
                required: true,
            },
            // Shader (classId = 6552) -> shaderpacks/
            CfpackFile {
                project_id: 30,
                file_id: 300,
                required: false,
            },
            // Unknown/None classId -> defaults to mods/
            CfpackFile {
                project_id: 40,
                file_id: 400,
                required: true,
            },
            // Distribution disabled (download_url = None) -> manual_files
            CfpackFile {
                project_id: 50,
                file_id: 500,
                required: true,
            },
            // Blank download_url -> manual_files (DistributionDisabled)
            CfpackFile {
                project_id: 60,
                file_id: 600,
                required: false,
            },
            // Missing from API response -> manual_files (MetadataNotFound)
            CfpackFile {
                project_id: 70,
                file_id: 700,
                required: true,
            },
        ];

        write_batch_cache(
            temp.path(),
            "mods/files",
            &serde_json::json!({ "fileIds": [100_u64, 200, 300, 400, 500, 600, 700] }),
            vec![
                sample_mod_file(
                    100,
                    10,
                    "jei.jar",
                    Some("  https://edge.forgecdn.net/files/100/jei.jar  "),
                    Some("sha1_jei"),
                ),
                sample_mod_file(
                    200,
                    20,
                    "faithful.zip",
                    Some("https://edge.forgecdn.net/files/200/faithful.zip"),
                    None,
                ),
                sample_mod_file(
                    300,
                    30,
                    "bsl.zip",
                    Some("https://edge.forgecdn.net/files/300/bsl.zip"),
                    Some("sha1_bsl"),
                ),
                sample_mod_file(
                    400,
                    40,
                    "fallback.jar",
                    Some("https://edge.forgecdn.net/files/400/fallback.jar"),
                    None,
                ),
                sample_mod_file(500, 50, "restricted.jar", None, Some("sha1_res")),
                sample_mod_file(600, 60, "blank_url.zip", Some("   "), None),
            ],
        );

        write_batch_cache(
            temp.path(),
            "mods",
            &serde_json::json!({ "modIds": [10_u64, 20, 30, 40, 50, 60, 70] }),
            vec![
                sample_mod_info(10, Some(6), "JEI"),
                sample_mod_info(20, Some(12), "Faithful"),
                sample_mod_info(30, Some(6552), "BSL Shaders"),
                sample_mod_info(40, None, "Fallback Mod"),
                sample_mod_info(50, Some(6), "Restricted Mod"),
                sample_mod_info(60, Some(12), "Blank URL Pack"),
                sample_mod_info(70, Some(6552), "Deleted Shader"),
            ],
        );

        let resolution = resolve_files(&entries, &client, temp.path())
            .await
            .expect("cached cfpack files should resolve");

        assert_eq!(
            resolution.files,
            vec![
                ResolvedModpackFile {
                    path: "mods/jei.jar".to_string(),
                    urls: vec!["https://edge.forgecdn.net/files/100/jei.jar".to_string()],
                    size: 4096,
                    sha1: Some("sha1_jei".to_string()),
                    sha512: None,
                },
                ResolvedModpackFile {
                    path: "resourcepacks/faithful.zip".to_string(),
                    urls: vec!["https://edge.forgecdn.net/files/200/faithful.zip".to_string()],
                    size: 4096,
                    sha1: None,
                    sha512: None,
                },
                ResolvedModpackFile {
                    path: "shaderpacks/bsl.zip".to_string(),
                    urls: vec!["https://edge.forgecdn.net/files/300/bsl.zip".to_string()],
                    size: 4096,
                    sha1: Some("sha1_bsl".to_string()),
                    sha512: None,
                },
                ResolvedModpackFile {
                    path: "mods/fallback.jar".to_string(),
                    urls: vec!["https://edge.forgecdn.net/files/400/fallback.jar".to_string()],
                    size: 4096,
                    sha1: None,
                    sha512: None,
                },
            ]
        );

        assert_eq!(
            resolution.manual_files,
            vec![
                ManualModpackFile {
                    project_id: 50,
                    file_id: 500,
                    project_name: Some("Restricted Mod".to_string()),
                    file_name: Some("restricted.jar".to_string()),
                    target_dir: "mods".to_string(),
                    page_url: "https://www.curseforge.com/minecraft/mc-mods/restricted-mod/files/500"
                        .to_string(),
                    required: true,
                    reason: ManualDownloadReason::DistributionDisabled,
                    file_length: Some(4096),
                    sha1: Some("sha1_res".to_string()),
                    md5: Some("md5ignored".to_string()),
                },
                ManualModpackFile {
                    project_id: 60,
                    file_id: 600,
                    project_name: Some("Blank URL Pack".to_string()),
                    file_name: Some("blank_url.zip".to_string()),
                    target_dir: "resourcepacks".to_string(),
                    page_url:
                        "https://www.curseforge.com/minecraft/texture-packs/blank-url-pack/files/600"
                            .to_string(),
                    required: false,
                    reason: ManualDownloadReason::DistributionDisabled,
                    file_length: Some(4096),
                    sha1: None,
                    md5: None,
                },
                ManualModpackFile {
                    project_id: 70,
                    file_id: 700,
                    project_name: Some("Deleted Shader".to_string()),
                    file_name: None,
                    target_dir: "shaderpacks".to_string(),
                    page_url:
                        "https://www.curseforge.com/minecraft/shaders/deleted-shader/files/700"
                            .to_string(),
                    required: true,
                    reason: ManualDownloadReason::MetadataNotFound,
                    file_length: None,
                    sha1: None,
                    md5: None,
                },
            ]
        );

        // Zero file_length from CurseForge metadata maps to file_length: None
        {
            let zero_temp = TempDirGuard::new("resolve_zero_len");
            let zero_entries = vec![CfpackFile {
                project_id: 88,
                file_id: 888,
                required: true,
            }];
            let mut zero_file = sample_mod_file(888, 88, "zero.jar", None, Some("sha1_zero"));
            zero_file.file_length = 0;

            write_batch_cache(
                zero_temp.path(),
                "mods/files",
                &serde_json::json!({ "fileIds": [888_u64] }),
                vec![zero_file],
            );
            write_batch_cache(
                zero_temp.path(),
                "mods",
                &serde_json::json!({ "modIds": [88_u64] }),
                vec![sample_mod_info(88, Some(6), "Zero Length Mod")],
            );

            let zero_res = resolve_files(&zero_entries, &client, zero_temp.path())
                .await
                .expect("zero-length restricted file should resolve as manual");
            assert_eq!(zero_res.manual_files.len(), 1);
            assert_eq!(zero_res.manual_files[0].file_length, None);
            assert_eq!(zero_res.manual_files[0].sha1.as_deref(), Some("sha1_zero"));
            assert_eq!(zero_res.manual_files[0].md5.as_deref(), Some("md5ignored"));
        }

        // Unsafe file_name returned by API (containing path traversal or subdirectory) is rejected
        for unsafe_name in ["../evil.jar", "sub/nested.jar"] {
            let unsafe_temp = TempDirGuard::new("resolve_unsafe_name");
            let unsafe_entries = vec![CfpackFile {
                project_id: 99,
                file_id: 999,
                required: true,
            }];

            write_batch_cache(
                unsafe_temp.path(),
                "mods/files",
                &serde_json::json!({ "fileIds": [999_u64] }),
                vec![sample_mod_file(
                    999,
                    99,
                    unsafe_name,
                    Some("https://edge.forgecdn.net/files/999/mod.jar"),
                    None,
                )],
            );
            write_batch_cache(
                unsafe_temp.path(),
                "mods",
                &serde_json::json!({ "modIds": [99_u64] }),
                vec![sample_mod_info(99, Some(6), "Bad File")],
            );

            let err = resolve_files(&unsafe_entries, &client, unsafe_temp.path())
                .await
                .expect_err("unsafe CurseForge file_name should be rejected");
            assert!(matches!(
                err,
                MinecraftError::ModpackUnsafePath { ref path } if path == unsafe_name
            ));
        }
    }
}
