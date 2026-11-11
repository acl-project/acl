// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, PathBuf};
use std::{collections::HashMap, path::Path};

use acl_common::maven::coordinate_to_path;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::DEFAULT_MANIFEST_TTL;
use crate::DownloadSource;
use crate::Library;
use crate::MinecraftError;
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
    pub install: LegacyInstallInfo,
    #[serde(rename = "versionInfo")]
    pub version_info: VersionPatch,
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

    pub async fn run_processors(
        &self,
        java_path: &Path,
        installer_path: &Path,
        minecraft_jar: &Path,
        libraries_dir: &Path,
        temp_dir: &Path,
    ) -> Result<()> {
        let data_map = self.build_data_map(installer_path, minecraft_jar, libraries_dir, temp_dir);

        for processor in &self.processors {
            if !processor.is_client() {
                continue;
            }

            let jar_rel = coordinate_to_path(&processor.jar).ok_or_else(|| {
                MinecraftError::InvalidMavenCoordinate {
                    coordinate: processor.jar.clone(),
                }
            })?;

            let jar_path = libraries_dir.join(jar_rel);

            let main_class = read_main_class(&jar_path)?;

            let mut cp_entries = Vec::new();
            for cp_coord in &processor.classpath {
                if let Some(rel) = coordinate_to_path(cp_coord) {
                    cp_entries.push(libraries_dir.join(rel));
                }
            }
            cp_entries.push(jar_path);

            let cp_str = std::env::join_paths(cp_entries)?;

            let args = processor.resolve_args(&data_map, libraries_dir);

            let status = tokio::process::Command::new(java_path)
                .arg("-cp")
                .arg(cp_str)
                .arg(main_class)
                .args(&args)
                .status()
                .await
                .map_err(|source| MinecraftError::Io {
                    path: java_path.to_path_buf(),
                    source,
                })?;

            if !status.success() {
                return Err(MinecraftError::ProcessorFailed {
                    jar: processor.jar.clone(),
                    exit_code: status.code().unwrap_or(-1),
                });
            }
        }

        Ok(())
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

/// Obtain the corresponding decompression path according to different directories and loader versions.
fn resolve_entry_dest(
    entry_name: &str,
    profile: &ForgeInstallProfile,
    libraries_dir: &Path,
    temp_dir: &Path,
) -> Option<PathBuf> {
    if let Some(stripped) = entry_name.strip_prefix("maven/") {
        return Some(libraries_dir.join(stripped));
    }

    if entry_name.starts_with("data/") {
        return Some(temp_dir.join(entry_name));
    }

    if let ForgeInstallProfile::Legacy(legacy) = profile
        && entry_name == legacy.install.file_path
    {
        return coordinate_to_path(&legacy.install.path).map(|p| libraries_dir.join(p));
    }

    None
}

/// The product after unpacking installer
pub struct ForgeInstaller {
    /// Unifiedly generated VersionPatch
    pub patch: VersionPatch,
    /// If it is a modern version (1.8+ /1.13+), return the configuration containing processors; if it is 1.7.10, it is None
    pub modern: Option<ModernInstallProfile>,
}

pub fn extract_installer(
    installer_path: &Path,
    libraries_dir: &Path,
    temp_dir: &Path,
) -> Result<ForgeInstaller> {
    let file = File::open(installer_path).map_err(|source| MinecraftError::Io {
        path: installer_path.to_path_buf(),
        source,
    })?;

    let mut zip = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
        path: installer_path.to_path_buf(),
        source,
    })?;

    // First parse install_profile.json to generate profile
    let mut profile_entry =
        zip.by_name("install_profile.json")
            .map_err(|source| MinecraftError::Zip {
                path: installer_path.to_path_buf(),
                source,
            })?;

    let mut profile_content = Vec::with_capacity(profile_entry.size() as usize);
    profile_entry
        .read_to_end(&mut profile_content)
        .map_err(|source| MinecraftError::Io {
            path: installer_path.to_path_buf(),
            source,
        })?;

    let profile: ForgeInstallProfile = serde_json::from_slice(&profile_content)?;

    drop(profile_entry);

    // Parse version.json to get the patch or get it directly from version info, for old versions
    let patch: VersionPatch = match &profile {
        ForgeInstallProfile::Legacy(profile) => profile.version_info.clone(),
        ForgeInstallProfile::Modern(_) => {
            if let Ok(mut entry) = zip.by_name("version.json") {
                let mut content = Vec::with_capacity(entry.size() as usize);
                entry
                    .read_to_end(&mut content)
                    .map_err(|source| MinecraftError::Io {
                        path: installer_path.to_path_buf(),
                        source,
                    })?;

                serde_json::from_slice(&content)?
            } else {
                return Err(MinecraftError::ForgeMissingVersionJson);
            }
        }
    };

    // Decompress resources
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|source| MinecraftError::Zip {
            path: installer_path.to_path_buf(),
            source,
        })?;

        if entry.is_dir() {
            continue;
        }

        let entry_name = entry.name().to_string();
        let rel_path = Path::new(&entry_name);

        // Prevent path penetration
        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            continue;
        }

        let Some(dest) = resolve_entry_dest(&entry_name, &profile, libraries_dir, temp_dir) else {
            continue;
        };

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|source| MinecraftError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|source| MinecraftError::Io {
                path: installer_path.to_path_buf(),
                source,
            })?;

        fs::write(&dest, buf).map_err(|source| MinecraftError::Io { path: dest, source })?;
    }

    let modern = match profile {
        ForgeInstallProfile::Modern(m) => Some(m),
        ForgeInstallProfile::Legacy(_) => None,
    };

    Ok(ForgeInstaller { patch, modern })
}

fn read_main_class(jar_path: &Path) -> Result<String> {
    let file = File::open(jar_path).map_err(|source| MinecraftError::Io {
        path: jar_path.to_path_buf(),
        source,
    })?;

    let mut zip = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
        path: jar_path.to_path_buf(),
        source,
    })?;

    let mut entry = zip
        .by_name("META-INF/MANIFEST.MF")
        .map_err(|source| MinecraftError::Zip {
            path: jar_path.to_path_buf(),
            source,
        })?;

    let mut content = String::new();

    entry
        .read_to_string(&mut content)
        .map_err(|source| MinecraftError::Io {
            path: jar_path.to_path_buf(),
            source,
        })?;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Main-Class:") {
            return Ok(rest.trim().to_string());
        }
    }

    Err(MinecraftError::MissingMainClass {
        jar_path: jar_path.to_path_buf(),
    })
}
