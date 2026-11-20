// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    MinecraftError, ProjectHost, Result,
    modpack::{
        ModpackFile, ModpackFileSource, ModpackLoader, ModpackManifest, normalize_relative_path,
    },
    modrinth::{FileHashes, SideSupport},
};

pub const MRPACK_FORMAT_VERSION: u32 = 1;
pub const MRPACK_INDEX_FILE: &str = "modrinth.index.json";
pub const MRPACK_GAME: &str = "minecraft";

const fn default_required_side() -> SideSupport {
    SideSupport::Required
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MrpackFileEnv {
    #[serde(default = "default_required_side")]
    pub client: SideSupport,
    #[serde(default = "default_required_side")]
    pub server: SideSupport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackFile {
    /// Relative path inside the instance directory, e.g. "mods/sodium.jar"
    pub path: String,
    #[serde(default)]
    pub hashes: FileHashes,
    /// If omitted, the file is required on both client and server
    #[serde(default)]
    pub env: Option<MrpackFileEnv>,
    #[serde(default)]
    pub downloads: Vec<String>,
    pub file_size: u64,
}

impl MrpackFile {
    /// Returns `None` if unsupported on the client, `Some(true)` if required,
    /// and `Some(false)` if optional.
    pub fn client_requirement(&self) -> Option<bool> {
        match self.env.map(|env| env.client) {
            None | Some(SideSupport::Required | SideSupport::Unknown) => Some(true),
            Some(SideSupport::Optional) => Some(false),
            Some(SideSupport::Unsupported) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MrpackIndex {
    pub format_version: u32,
    pub game: String,
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

fn parse_mrpack_dependencies(
    dependencies: &BTreeMap<String, String>,
) -> Result<(String, Option<ModpackLoader>)> {
    let mut game_version: Option<String> = None;
    let mut loader: Option<ModpackLoader> = None;

    for (key, value) in dependencies {
        let value = value.trim();
        if value.is_empty() {
            return Err(MinecraftError::ModpackEmptyDependencyVersion {
                dependency: key.clone(),
            });
        }

        if key == "minecraft" {
            game_version = Some(value.to_string());
            continue;
        }

        let parsed_loader = match key.as_str() {
            "fabric-loader" => ModpackLoader::Fabric(value.to_string()),
            "quilt-loader" => ModpackLoader::Quilt(value.to_string()),
            "forge" => ModpackLoader::Forge(value.to_string()),
            "neoforge" => ModpackLoader::NeoForge(value.to_string()),
            other => {
                return Err(MinecraftError::ModpackUnsupportedDependency {
                    dependency: other.to_string(),
                });
            }
        };

        if loader.is_some() {
            return Err(MinecraftError::ModpackMultipleModLoaders);
        }
        loader = Some(parsed_loader);
    }

    let game_version =
        game_version.ok_or_else(|| MinecraftError::ModpackMissingMinecraftVersion)?;

    Ok((game_version, loader))
}

impl TryFrom<MrpackIndex> for ModpackManifest {
    type Error = MinecraftError;

    fn try_from(index: MrpackIndex) -> Result<Self> {
        if index.format_version != MRPACK_FORMAT_VERSION {
            return Err(MinecraftError::ModpackUnsupportedFormatVersion {
                version: index.format_version,
            });
        }

        if index.game != MRPACK_GAME {
            return Err(MinecraftError::ModpackUnsupportedGame { game: index.game });
        }

        let (game_version, loader) = parse_mrpack_dependencies(&index.dependencies)?;

        let mut files = Vec::with_capacity(index.files.len());
        for file in index.files {
            let Some(required) = file.client_requirement() else {
                continue;
            };

            let normalized_path = normalize_relative_path(&file.path)?;
            if file.downloads.is_empty() || file.downloads.iter().any(|url| url.trim().is_empty()) {
                return Err(MinecraftError::ModpackMissingDownloads {
                    path: normalized_path,
                });
            }

            files.push(ModpackFile {
                source: ModpackFileSource::Direct {
                    path: normalized_path,
                    downloads: file.downloads,
                    size: file.file_size,
                    sha1: file.hashes.sha1,
                    sha512: file.hashes.sha512,
                },
                required,
            });
        }

        Ok(Self {
            host: ProjectHost::Modrinth,
            name: index.name,
            version: index.version_id,
            summary: index.summary,
            author: None,
            game_version,
            loader,
            recommended_ram_mb: None,
            override_dirs: vec!["overrides".to_string(), "client-overrides".to_string()],
            files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file(path: &str, env: Option<MrpackFileEnv>, downloads: Vec<String>) -> MrpackFile {
        MrpackFile {
            path: path.to_string(),
            hashes: FileHashes {
                sha1: Some("deadbeef".to_string()),
                sha512: Some("cafebabe".to_string()),
            },
            env,
            downloads,
            file_size: 1024,
        }
    }

    fn base_index(dependencies: BTreeMap<String, String>, files: Vec<MrpackFile>) -> MrpackIndex {
        MrpackIndex {
            format_version: 1,
            game: "minecraft".to_string(),
            version_id: "1.0.0".to_string(),
            name: "Sample Pack".to_string(),
            summary: Some("A sample modpack".to_string()),
            files,
            dependencies,
        }
    }

    #[test]
    fn test_mrpack_file_client_requirement() {
        let no_env = sample_file(
            "mods/a.jar",
            None,
            vec!["https://example.com/a.jar".to_string()],
        );
        assert_eq!(no_env.client_requirement(), Some(true));

        let required = sample_file(
            "mods/b.jar",
            Some(MrpackFileEnv {
                client: SideSupport::Required,
                server: SideSupport::Unsupported,
            }),
            vec!["https://example.com/b.jar".to_string()],
        );
        assert_eq!(required.client_requirement(), Some(true));

        let unknown = sample_file(
            "mods/c.jar",
            Some(MrpackFileEnv {
                client: SideSupport::Unknown,
                server: SideSupport::Required,
            }),
            vec!["https://example.com/c.jar".to_string()],
        );
        assert_eq!(unknown.client_requirement(), Some(true));

        let optional = sample_file(
            "mods/d.jar",
            Some(MrpackFileEnv {
                client: SideSupport::Optional,
                server: SideSupport::Required,
            }),
            vec!["https://example.com/d.jar".to_string()],
        );
        assert_eq!(optional.client_requirement(), Some(false));

        let unsupported = sample_file(
            "mods/e.jar",
            Some(MrpackFileEnv {
                client: SideSupport::Unsupported,
                server: SideSupport::Required,
            }),
            vec!["https://example.com/e.jar".to_string()],
        );
        assert_eq!(unsupported.client_requirement(), None);

        // Omitting `client` in JSON defaults `client` to `Required`
        let json = r#"{
            "path": "mods/f.jar",
            "hashes": {},
            "env": { "server": "unsupported" },
            "downloads": ["https://example.com/f.jar"],
            "fileSize": 512
        }"#;
        let parsed: MrpackFile =
            serde_json::from_str(json).expect("failed to deserialize MrpackFile");
        assert_eq!(
            parsed.env.expect("env should be present").client,
            SideSupport::Required
        );
        assert_eq!(parsed.client_requirement(), Some(true));
    }

    #[test]
    fn test_mrpack_index_try_from_loaders_and_files() {
        let loader_cases = [
            (None, None),
            (
                Some(("fabric-loader", "0.15.11")),
                Some(ModpackLoader::Fabric("0.15.11".to_string())),
            ),
            (
                Some(("quilt-loader", "0.26.0")),
                Some(ModpackLoader::Quilt("0.26.0".to_string())),
            ),
            (
                Some(("forge", "47.2.0")),
                Some(ModpackLoader::Forge("47.2.0".to_string())),
            ),
            (
                Some(("neoforge", "21.1.72")),
                Some(ModpackLoader::NeoForge("21.1.72".to_string())),
            ),
        ];

        for (loader_entry, expected_loader) in loader_cases {
            let mut deps = BTreeMap::new();
            deps.insert("minecraft".to_string(), "1.20.1".to_string());
            if let Some((key, ver)) = loader_entry {
                deps.insert(key.to_string(), ver.to_string());
            }

            let files = vec![
                sample_file(
                    "mods\\sodium.jar",
                    None,
                    vec!["https://cdn.modrinth.com/sodium.jar".to_string()],
                ),
                sample_file(
                    "mods/iris.jar",
                    Some(MrpackFileEnv {
                        client: SideSupport::Optional,
                        server: SideSupport::Unsupported,
                    }),
                    vec![
                        "https://cdn.modrinth.com/iris.jar".to_string(),
                        "https://mirror.example.com/iris.jar".to_string(),
                    ],
                ),
                sample_file(
                    "mods/server-only.jar",
                    Some(MrpackFileEnv {
                        client: SideSupport::Unsupported,
                        server: SideSupport::Required,
                    }),
                    vec!["https://cdn.modrinth.com/server-only.jar".to_string()],
                ),
            ];

            let index = base_index(deps, files);
            let manifest = ModpackManifest::try_from(index)
                .expect("valid mrpack index should convert to ModpackManifest");

            assert_eq!(manifest.host, ProjectHost::Modrinth);
            assert_eq!(manifest.name, "Sample Pack");
            assert_eq!(manifest.version, "1.0.0");
            assert_eq!(manifest.summary.as_deref(), Some("A sample modpack"));
            assert_eq!(manifest.author, None);
            assert_eq!(manifest.game_version, "1.20.1");
            assert_eq!(manifest.loader, expected_loader);
            assert_eq!(manifest.recommended_ram_mb, None);
            assert_eq!(
                manifest.override_dirs,
                vec!["overrides".to_string(), "client-overrides".to_string()]
            );

            assert_eq!(
                manifest.files,
                vec![
                    ModpackFile {
                        source: ModpackFileSource::Direct {
                            path: "mods/sodium.jar".to_string(),
                            downloads: vec!["https://cdn.modrinth.com/sodium.jar".to_string()],
                            size: 1024,
                            sha1: Some("deadbeef".to_string()),
                            sha512: Some("cafebabe".to_string()),
                        },
                        required: true,
                    },
                    ModpackFile {
                        source: ModpackFileSource::Direct {
                            path: "mods/iris.jar".to_string(),
                            downloads: vec![
                                "https://cdn.modrinth.com/iris.jar".to_string(),
                                "https://mirror.example.com/iris.jar".to_string(),
                            ],
                            size: 1024,
                            sha1: Some("deadbeef".to_string()),
                            sha512: Some("cafebabe".to_string()),
                        },
                        required: false,
                    },
                ]
            );
        }
    }

    #[test]
    fn test_mrpack_index_validation_errors() {
        let mut valid_deps = BTreeMap::new();
        valid_deps.insert("minecraft".to_string(), "1.20.1".to_string());

        // Unsupported format version
        let mut bad_format = base_index(valid_deps.clone(), Vec::new());
        bad_format.format_version = 2;
        assert!(matches!(
            ModpackManifest::try_from(bad_format),
            Err(MinecraftError::ModpackUnsupportedFormatVersion { version: 2 })
        ));

        // Unsupported game
        let mut bad_game = base_index(valid_deps.clone(), Vec::new());
        bad_game.game = "other".to_string();
        assert!(matches!(
            ModpackManifest::try_from(bad_game),
            Err(MinecraftError::ModpackUnsupportedGame { ref game }) if game == "other"
        ));

        // Missing minecraft dependency
        let mut no_mc = BTreeMap::new();
        no_mc.insert("fabric-loader".to_string(), "0.15.11".to_string());
        assert!(matches!(
            ModpackManifest::try_from(base_index(no_mc, Vec::new())),
            Err(MinecraftError::ModpackMissingMinecraftVersion)
        ));

        // Empty or whitespace-only dependency version
        let mut empty_dep = valid_deps.clone();
        empty_dep.insert("fabric-loader".to_string(), "   ".to_string());
        assert!(matches!(
            ModpackManifest::try_from(base_index(empty_dep, Vec::new())),
            Err(MinecraftError::ModpackEmptyDependencyVersion { ref dependency })
                if dependency == "fabric-loader"
        ));

        // Unsupported dependency key
        let mut unsupported_dep = valid_deps.clone();
        unsupported_dep.insert("liteloader".to_string(), "1.12.2".to_string());
        assert!(matches!(
            ModpackManifest::try_from(base_index(unsupported_dep, Vec::new())),
            Err(MinecraftError::ModpackUnsupportedDependency { ref dependency })
                if dependency == "liteloader"
        ));

        // Multiple mod loaders
        let mut multi_loader = valid_deps.clone();
        multi_loader.insert("fabric-loader".to_string(), "0.15.11".to_string());
        multi_loader.insert("forge".to_string(), "47.2.0".to_string());
        assert!(matches!(
            ModpackManifest::try_from(base_index(multi_loader, Vec::new())),
            Err(MinecraftError::ModpackMultipleModLoaders)
        ));

        // Empty downloads list
        let empty_downloads_file = sample_file("mods/no-url.jar", None, Vec::new());
        assert!(matches!(
            ModpackManifest::try_from(base_index(valid_deps.clone(), vec![empty_downloads_file])),
            Err(MinecraftError::ModpackMissingDownloads { ref path }) if path == "mods/no-url.jar"
        ));

        // Blank URL inside downloads list
        let blank_url_file = sample_file("mods/blank-url.jar", None, vec!["  ".to_string()]);
        assert!(matches!(
            ModpackManifest::try_from(base_index(valid_deps.clone(), vec![blank_url_file])),
            Err(MinecraftError::ModpackMissingDownloads { ref path })
                if path == "mods/blank-url.jar"
        ));

        // Unsafe path traversal in file path
        let unsafe_file = sample_file(
            "../mods/a.jar",
            None,
            vec!["https://example.com/a.jar".to_string()],
        );
        assert!(matches!(
            ModpackManifest::try_from(base_index(valid_deps, vec![unsafe_file])),
            Err(MinecraftError::ModpackUnsafePath { ref path }) if path == "../mods/a.jar"
        ));
    }
}
