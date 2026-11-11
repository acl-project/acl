// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{collections::HashMap, path::Path};

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
