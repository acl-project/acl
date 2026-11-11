// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};

use acl_common::maven::coordinate_to_path;
use serde::{Deserialize, Serialize};

use crate::DEFAULT_MANIFEST_TTL;
use crate::DownloadSource;
use crate::Library;
use crate::Result;
use crate::VersionPatch;
use crate::cache::fetch_cached_json;

const FORGE_PROMOS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_METADATA_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgePromos {
    pub homepage: String,
    /// The latest loader version and recommended loader version for the game version
    pub promos: HashMap<String, String>,
}

impl ForgePromos {
    /// Get stable loader version
    pub fn stable(&self, version: &str) -> Option<String> {
        let v = format!("{version}-recommended");
        self.promos.get(&v).cloned()
    }

    /// Get latest loader version
    pub fn latest(&self, version: &str) -> Option<String> {
        let v = format!("{version}-latest");
        self.promos.get(&v).cloned()
    }

    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Self> {
        fetch_cached_json(
            client,
            FORGE_PROMOS_URL,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
            download_source,
            force_refresh,
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeManifest {
    #[serde(flatten)]
    pub mc_to_forge: HashMap<String, Vec<String>>,
}

impl ForgeManifest {
    pub fn download_url(loader: &str) -> String {
        format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{0}/forge-{0}-installer.jar",
            loader
        )
    }

    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Self> {
        fetch_cached_json(
            client,
            FORGE_METADATA_URL,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
            download_source,
            force_refresh,
        )
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyInstallInfo {
    #[serde(rename = "filePath")]
    /// Corresponds to universal.jar in the root directory
    pub file_path: String,
    /// Corresponding Maven coordinates
    pub path: String,
}

/// Used for legacy version parsing prior to 1.7.10
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyInstallProfile {
    install: LegacyInstallInfo,
    #[serde(rename = "versionInfo")]
    version_info: VersionPatch,
}

/// Corresponds to the entry in data, used for processor command-line argument substitution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SidedDataEntry {
    /// In most cases: distinguish client / server separately
    Sided {
        client: String,
        #[serde(default)]
        server: Option<String>,
    },
    /// Very few string literals
    Raw(String),
}

impl SidedDataEntry {
    pub fn client(&self) -> &str {
        match self {
            Self::Sided { client, .. } => client,
            Self::Raw(s) => s,
        }
    }
}

/// Corresponds to a single build task (Processor) for 1.13+
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeProcessor {
    /// The executor's own main Jar (Maven coordinates)
    pub jar: String,

    #[serde(default)]
    /// Additional dependency libraries required by the executor (Maven coordinate list)
    pub classpath: Vec<String>,

    #[serde(default)]
    /// Run parameters (including placeholders such as {MINECRAFT_JAR}, {MAPPINGS})
    pub args: Vec<String>,

    #[serde(default)]
    /// ["server"] or ["client"]; if omitted, both the client and server will run.
    pub sides: Option<Vec<String>>,

    #[serde(default)]
    /// Output file verification (Path -> SHA1)
    pub outputs: Option<HashMap<String, String>>,
}

impl ForgeProcessor {
    pub fn is_client(&self) -> bool {
        match &self.sides {
            Some(sides) => sides.iter().any(|s| s == "client"),
            None => true,
        }
    }

    pub fn resolve_args(
        &self,
        data_map: &HashMap<String, String>,
        libraries_dir: &Path,
    ) -> Vec<String> {
        self.args
            .iter()
            .map(|raw_arg| {
                let mut arg = raw_arg.clone();
                //  Replace [Maven coordinates]
                if arg.starts_with('[') && arg.ends_with(']') {
                    let coord = &arg[1..arg.len() - 1];
                    if let Some(rel_path) = coordinate_to_path(coord) {
                        return libraries_dir.join(rel_path).to_string_lossy().to_string();
                    }
                }

                // Replace {key}
                for (k, v) in data_map {
                    let placeholder = format!("{{{k}}}");
                    if arg.contains(&placeholder) {
                        arg = arg.replace(&placeholder, v);
                    }
                }

                arg
            })
            .collect()
    }
}

/// Modern profile after 1.8
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModernInstallProfile {
    #[serde(default = "default_version_json")]
    pub json: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub data: HashMap<String, SidedDataEntry>,
    #[serde(default)]
    pub processors: Vec<ForgeProcessor>,
}

impl ModernInstallProfile {
    pub fn build_data_map(
        &self,
        installer_path: &Path,
        minecraft_jar: &Path,
        libraries_dir: &Path,
        temp_dir: &Path,
    ) -> HashMap<String, String> {
        let mut map = HashMap::new();

        // Fixed constant
        map.insert("SIDE".to_string(), "client".to_string());
        map.insert(
            "MINECRAFT_JAR".to_string(),
            minecraft_jar.to_string_lossy().to_string(),
        );
        map.insert(
            "ROOT".to_string(),
            libraries_dir
                .parent()
                .unwrap_or(libraries_dir)
                .to_string_lossy()
                .to_string(),
        );
        map.insert(
            "INSTALLER".to_string(),
            installer_path.to_string_lossy().to_string(),
        );
        map.insert(
            "LIBRARY_DIR".to_string(),
            libraries_dir.to_string_lossy().to_string(),
        );

        for (key, entry) in &self.data {
            let raw_val = entry.client();
            let resolved = if raw_val.starts_with('[') && raw_val.ends_with(']') {
                let coord = &raw_val[1..raw_val.len() - 1];

                if let Some(rel_path) = coordinate_to_path(coord) {
                    let full_path = libraries_dir.join(rel_path);

                    // Ensure the directory exists
                    if let Some(parent) = full_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    full_path.to_string_lossy().to_string()
                } else {
                    raw_val.to_string()
                }
            } else if raw_val.starts_with('/') {
                // Installer built-in data file (absolute path extracted to the temporary directory)
                let temp_path = temp_dir.join(raw_val.trim_start_matches('/'));

                if let Some(parent) = temp_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                temp_path.to_string_lossy().to_string()
            } else if raw_val.starts_with('\'') && raw_val.ends_with('\'') {
                // String literal (remove single quotes)
                raw_val[1..raw_val.len() - 1].to_string()
            } else {
                raw_val.to_string()
            };

            map.insert(key.clone(), resolved);
        }

        map
    }
}

fn default_version_json() -> String {
    "/version.json".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ForgeInstallProfile {
    /// 1.7.10 and ancient versions: with install and inline versionInfo
    Legacy(LegacyInstallProfile),
    /// 1.8 ~ 1.12.2 and 1.13+ modern versions: with version.json pointer, optional data and processors
    Modern(ModernInstallProfile),
}
