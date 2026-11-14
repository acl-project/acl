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

// In programming, as in rowing upstream, not to advance is to drift back.
// Forge is gone
const FORGE_PROMOS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_METADATA_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";

const NEOFORGE_METADATA_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
// This is so funny
const NEOFORGE_LEGACY_1201_METADATA_URL: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge";

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
pub struct InstallerProcessor {
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

impl InstallerProcessor {
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
                if arg.starts_with('[')
                    && arg.ends_with(']')
                    && let Some(coord) = arg.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
                    && let Some(rel_path) = coordinate_to_path(coord)
                {
                    return libraries_dir.join(rel_path).to_string_lossy().to_string();
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
    pub processors: Vec<InstallerProcessor>,
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
                let Some(coord) = raw_val.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
                else {
                    continue;
                };

                if let Some(rel_path) = coordinate_to_path(coord) {
                    let full_path = libraries_dir.join(rel_path);

                    // Ensure the directory exists
                    if let Some(parent) = full_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }

                    full_path.to_string_lossy().to_string()
                } else {
                    raw_val.to_string()
                }
            } else if raw_val.starts_with('/') {
                // Installer built-in data file (absolute path extracted to the temporary directory)
                let temp_path = temp_dir.join(raw_val.trim_start_matches('/'));

                if let Some(parent) = temp_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                temp_path.to_string_lossy().to_string()
            } else if raw_val.starts_with('\'') && raw_val.ends_with('\'') {
                // String literal (remove single quotes)
                if let Some(stripped) = raw_val
                    .strip_prefix('\'')
                    .and_then(|s| s.strip_suffix('\''))
                {
                    stripped.to_string()
                } else {
                    continue;
                }
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
                .kill_on_drop(true)
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
pub enum InstallProfile {
    /// 1.7.10 and ancient versions: with install and inline versionInfo
    Legacy(LegacyInstallProfile),
    /// 1.8 ~ 1.12.2 and 1.13+ modern versions: with version.json pointer, optional data and processors
    Modern(ModernInstallProfile),
}

/// Obtain the corresponding decompression path according to different directories and loader versions.
fn resolve_entry_dest(
    entry_name: &str,
    profile: &InstallProfile,
    libraries_dir: &Path,
    temp_dir: &Path,
) -> Option<PathBuf> {
    if let Some(stripped) = entry_name.strip_prefix("maven/") {
        return Some(libraries_dir.join(stripped));
    }

    if entry_name.starts_with("data/") {
        return Some(temp_dir.join(entry_name));
    }

    if let InstallProfile::Legacy(legacy) = profile
        && entry_name == legacy.install.file_path
    {
        return coordinate_to_path(&legacy.install.path).map(|p| libraries_dir.join(p));
    }

    None
}

/// The product after unpacking installer
pub struct ExtractedInstaller {
    /// Unifiedly generated VersionPatch
    pub patch: VersionPatch,
    /// If it is a modern version (1.8+ /1.13+), return the configuration containing processors; if it is 1.7.10, it is None
    pub modern: Option<ModernInstallProfile>,
}

pub fn extract_installer(
    installer_path: &Path,
    libraries_dir: &Path,
    temp_dir: &Path,
) -> Result<ExtractedInstaller> {
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

    let profile: InstallProfile = serde_json::from_slice(&profile_content)?;

    drop(profile_entry);

    // Parse version.json to get the patch or get it directly from version info, for old versions
    let patch: VersionPatch = match &profile {
        InstallProfile::Legacy(profile) => profile.version_info.clone(),
        InstallProfile::Modern(_) => {
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
            fs::create_dir_all(parent).map_err(|source| MinecraftError::Io {
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
        InstallProfile::Modern(m) => Some(m),
        InstallProfile::Legacy(_) => None,
    };

    Ok(ExtractedInstaller { patch, modern })
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

/// Download the Forge or NeoForge installer JAR.
pub async fn download_installer(
    client: &reqwest::Client,
    url: &str,
    installer_path: &Path,
) -> Result<()> {
    let response = client.get(url).send().await?.error_for_status()?;

    let bytes = response.bytes().await?;

    if let Some(parent) = installer_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| MinecraftError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }

    tokio::fs::write(installer_path, bytes)
        .await
        .map_err(|source| MinecraftError::Io {
            path: installer_path.to_path_buf(),
            source,
        })?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeoForgeManifest {
    pub versions: Vec<String>,
}

impl NeoForgeManifest {
    pub fn download_url(loader: &str) -> String {
        if loader.starts_with("1.20.1-") {
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/forge/{0}/forge-{0}-installer.jar",
                loader
            )
        } else {
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar",
                loader
            )
        }
    }

    pub fn neoforge_prefix(mc_version: &str) -> String {
        if mc_version == "1.20.1" {
            return "1.20.1-".to_string();
        }

        // Old Version 1.20.x ~ 1.21.x
        if let Some(legacy_rest) = mc_version.strip_prefix("1.") {
            return if legacy_rest.contains('.') {
                format!("{legacy_rest}.")
            } else {
                format!("{legacy_rest}.0.")
            };
        }

        let dot_count = mc_version.chars().filter(|&c| c == '.').count();
        if dot_count == 1 {
            format!("{mc_version}.0.")
        } else {
            format!("{mc_version}.")
        }
    }

    pub fn versions_for_mc(&self, mc_version: &str) -> Vec<String> {
        let prefix = Self::neoforge_prefix(mc_version);

        self.versions
            .iter()
            .filter(|v| v.starts_with(&prefix))
            .cloned()
            .collect()
    }

    pub async fn fetch_with_cache(
        client: &reqwest::Client,
        game_version: &str,
        cache_dir: &Path,
        download_source: DownloadSource,
        force_refresh: bool,
    ) -> Result<Self> {
        let url = if game_version == "1.20.1" {
            NEOFORGE_LEGACY_1201_METADATA_URL
        } else {
            NEOFORGE_METADATA_URL
        };

        fetch_cached_json(
            client,
            url,
            cache_dir,
            DEFAULT_MANIFEST_TTL,
            download_source,
            force_refresh,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use zip::write::{SimpleFileOptions, ZipWriter};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_forge_{}_{}_{}",
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
        zip.finish().expect("failed to finish zip file");
    }

    #[test]
    fn test_forge_promos_deserialization_and_methods() {
        let json = r#"{
            "homepage": "https://files.minecraftforge.net/net/minecraftforge/forge/",
            "promos": {
                "1.20.1-latest": "47.3.0",
                "1.20.1-recommended": "47.3.0",
                "1.12.2-latest": "14.23.5.2860"
            }
        }"#;

        let promos: ForgePromos =
            serde_json::from_str(json).expect("failed to deserialize ForgePromos");
        assert_eq!(promos.stable("1.20.1").as_deref(), Some("47.3.0"));
        assert_eq!(promos.latest("1.20.1").as_deref(), Some("47.3.0"));
        assert_eq!(promos.stable("1.12.2"), None);
        assert_eq!(promos.latest("1.12.2").as_deref(), Some("14.23.5.2860"));
        assert_eq!(promos.stable("1.19"), None);
        assert_eq!(promos.latest("1.19"), None);
    }

    #[test]
    fn test_forge_manifest_deserialization_and_download_url() {
        let json = r#"{
            "1.20.1": [
                "1.20.1-47.0.0",
                "1.20.1-47.3.0"
            ],
            "1.12.2": [
                "1.12.2-14.23.5.2860"
            ]
        }"#;

        let manifest: ForgeManifest =
            serde_json::from_str(json).expect("failed to deserialize ForgeManifest");
        let v_1201 = manifest
            .mc_to_forge
            .get("1.20.1")
            .expect("1.20.1 must exist");
        assert_eq!(v_1201, &vec!["1.20.1-47.0.0", "1.20.1-47.3.0"]);

        let v_1122 = manifest
            .mc_to_forge
            .get("1.12.2")
            .expect("1.12.2 must exist");
        assert_eq!(v_1122, &vec!["1.12.2-14.23.5.2860"]);

        assert_eq!(
            ForgeManifest::download_url("1.20.1-47.3.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
        );
    }

    #[test]
    fn test_forge_install_profile_legacy_and_modern_deserialization() {
        let legacy_json = r#"{
            "install": {
                "profileName": "Forge",
                "target": "1.7.10-Forge10.13.4.1614-1.7.10",
                "path": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
                "version": "Forge 10.13.4.1614-1.7.10",
                "filePath": "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar"
            },
            "versionInfo": {
                "id": "1.7.10-Forge10.13.4.1614-1.7.10",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "minecraftArguments": "--username ${auth_player_name}",
                "libraries": []
            }
        }"#;

        let legacy_profile: InstallProfile =
            serde_json::from_str(legacy_json).expect("failed to deserialize legacy profile");
        match legacy_profile {
            InstallProfile::Legacy(legacy) => {
                assert_eq!(
                    legacy.install.file_path,
                    "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar"
                );
                assert_eq!(
                    legacy.install.path,
                    "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10"
                );
                assert_eq!(legacy.version_info.id, "1.7.10-Forge10.13.4.1614-1.7.10");
            }
            InstallProfile::Modern(_) => panic!("expected Legacy variant"),
        }

        let modern_json = r#"{
            "spec": 1,
            "profile": "forge",
            "version": "1.20.1-forge-47.3.0",
            "json": "/version.json",
            "data": {
                "MAPPINGS": {
                    "client": "[de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412:mappings@txt]"
                },
                "BINPATCH": {
                    "client": "/data/client.lzma"
                },
                "SHA": {
                    "client": "'4b6c41d915c857a006634a12b6cdf9e08451a4fd'"
                }
            },
            "processors": [
                {
                    "sides": ["client"],
                    "jar": "net.minecraftforge:installertools:1.4.1",
                    "classpath": ["net.sf.jopt-simple:jopt-simple:6.0-alpha-3"],
                    "args": ["--task", "MCP_DATA", "--output", "{MAPPINGS}"]
                },
                {
                    "sides": ["server"],
                    "jar": "net.minecraftforge:installertools:1.4.1",
                    "args": ["--server-only"]
                },
                {
                    "jar": "net.minecraftforge:jarsplitter:1.1.4",
                    "args": ["--both"]
                }
            ],
            "libraries": []
        }"#;

        let modern_profile: InstallProfile =
            serde_json::from_str(modern_json).expect("failed to deserialize modern profile");
        match modern_profile {
            InstallProfile::Modern(modern) => {
                assert_eq!(modern.json, "/version.json");
                assert_eq!(modern.data.len(), 3);
                assert_eq!(modern.processors.len(), 3);
                assert!(modern.processors[0].is_client());
                assert!(!modern.processors[1].is_client());
                assert!(modern.processors[2].is_client()); // omitted sides defaults to true
            }
            InstallProfile::Legacy(_) => panic!("expected Modern variant"),
        }
    }

    #[test]
    fn test_modern_profile_build_data_map_and_resolve_args() {
        let temp = TempDirGuard::new("data_map");
        let installer_path = temp.path().join("forge-installer.jar");
        let minecraft_jar = temp.path().join("minecraft.jar");
        let libraries_dir = temp.path().join("libraries");
        let extract_temp = temp.path().join("temp");

        let mut data = HashMap::new();
        data.insert(
            "MAPPINGS".to_string(),
            SidedDataEntry::Sided {
                client: "[de.oceanlabs.mcp:mcp_config:1.20.1:mappings@txt]".to_string(),
                server: None,
            },
        );
        data.insert(
            "BINPATCH".to_string(),
            SidedDataEntry::Sided {
                client: "/data/client.lzma".to_string(),
                server: None,
            },
        );
        data.insert(
            "SHA".to_string(),
            SidedDataEntry::Sided {
                client: "'4b6c41d915c857a006634a12b6cdf9e08451a4fd'".to_string(),
                server: None,
            },
        );

        let profile = ModernInstallProfile {
            json: "/version.json".to_string(),
            path: None,
            libraries: Vec::new(),
            data,
            processors: Vec::new(),
        };

        let map = profile.build_data_map(
            &installer_path,
            &minecraft_jar,
            &libraries_dir,
            &extract_temp,
        );

        assert_eq!(map.get("SIDE").map(String::as_str), Some("client"));
        assert_eq!(
            map.get("MINECRAFT_JAR").map(String::as_str),
            Some(minecraft_jar.to_str().unwrap())
        );
        assert_eq!(
            map.get("INSTALLER").map(String::as_str),
            Some(installer_path.to_str().unwrap())
        );
        assert_eq!(
            map.get("LIBRARY_DIR").map(String::as_str),
            Some(libraries_dir.to_str().unwrap())
        );

        // Check MAPPINGS coord converted to libraries path
        let expected_mappings =
            libraries_dir.join("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1-mappings.txt");
        assert_eq!(
            map.get("MAPPINGS").map(String::as_str),
            Some(expected_mappings.to_str().unwrap())
        );
        // Verify parent directory was created
        assert!(expected_mappings.parent().unwrap().exists());

        // Check BINPATCH /data/... extracted to temp directory path
        let expected_binpatch = extract_temp.join("data/client.lzma");
        assert_eq!(
            map.get("BINPATCH").map(String::as_str),
            Some(expected_binpatch.to_str().unwrap())
        );
        assert!(expected_binpatch.parent().unwrap().exists());

        // Check SHA stripped single quotes
        assert_eq!(
            map.get("SHA").map(String::as_str),
            Some("4b6c41d915c857a006634a12b6cdf9e08451a4fd")
        );

        // Test resolve_args
        let processor = InstallerProcessor {
            jar: "net.minecraftforge:installertools:1.4.1".to_string(),
            classpath: vec![],
            args: vec![
                "--task".to_string(),
                "MCP_DATA".to_string(),
                "--input".to_string(),
                "[de.oceanlabs.mcp:mcp_config:1.20.1@zip]".to_string(),
                "--output".to_string(),
                "{MAPPINGS}".to_string(),
                "--archive".to_string(),
                "{INSTALLER}".to_string(),
                "--exec".to_string(),
                "{ROOT}/run.sh".to_string(),
            ],
            sides: None,
            outputs: None,
        };

        let resolved = processor.resolve_args(&map, &libraries_dir);
        let expected_input_zip =
            libraries_dir.join("de/oceanlabs/mcp/mcp_config/1.20.1/mcp_config-1.20.1.zip");
        let expected_root_script = format!("{}/run.sh", map.get("ROOT").unwrap());

        assert_eq!(resolved[0], "--task");
        assert_eq!(resolved[1], "MCP_DATA");
        assert_eq!(resolved[2], "--input");
        assert_eq!(resolved[3], expected_input_zip.to_str().unwrap());
        assert_eq!(resolved[4], "--output");
        assert_eq!(resolved[5], expected_mappings.to_str().unwrap());
        assert_eq!(resolved[6], "--archive");
        assert_eq!(resolved[7], installer_path.to_str().unwrap());
        assert_eq!(resolved[8], "--exec");
        assert_eq!(resolved[9], expected_root_script);
    }

    #[test]
    fn test_resolve_entry_dest() {
        let temp = TempDirGuard::new("entry_dest");
        let libraries_dir = temp.path().join("libraries");
        let temp_dir = temp.path().join("temp");

        let legacy_profile = InstallProfile::Legacy(LegacyInstallProfile {
            install: LegacyInstallInfo {
                file_path: "forge-1.7.10-universal.jar".to_string(),
                path: "net.minecraftforge:forge:1.7.10".to_string(),
            },
            version_info: VersionPatch {
                id: "1.7.10".to_string(),
                inherits_from: None,
                main_class: None,
                minecraft_arguments: None,
                arguments: None,
                libraries: Vec::new(),
            },
        });

        // Rule 1: maven/ prefix
        let dest_maven = resolve_entry_dest(
            "maven/net/minecraftforge/forge/1.12.2/forge-1.12.2.jar",
            &legacy_profile,
            &libraries_dir,
            &temp_dir,
        );
        assert_eq!(
            dest_maven,
            Some(libraries_dir.join("net/minecraftforge/forge/1.12.2/forge-1.12.2.jar"))
        );

        // Rule 2: data/ prefix
        let dest_data = resolve_entry_dest(
            "data/client.lzma",
            &legacy_profile,
            &libraries_dir,
            &temp_dir,
        );
        assert_eq!(dest_data, Some(temp_dir.join("data/client.lzma")));

        // Rule 3: Legacy matching universal jar
        let dest_legacy = resolve_entry_dest(
            "forge-1.7.10-universal.jar",
            &legacy_profile,
            &libraries_dir,
            &temp_dir,
        );
        assert_eq!(
            dest_legacy,
            Some(libraries_dir.join("net/minecraftforge/forge/1.7.10/forge-1.7.10.jar"))
        );

        // Unrelated file
        let dest_other =
            resolve_entry_dest("big_logo.png", &legacy_profile, &libraries_dir, &temp_dir);
        assert_eq!(dest_other, None);
    }

    #[test]
    fn test_extract_installer_legacy() {
        let temp = TempDirGuard::new("extract_legacy");
        let installer_path = temp.path().join("installer-legacy.jar");
        let libraries_dir = temp.path().join("libraries");
        let temp_dir = temp.path().join("temp");

        let install_profile_json = r#"{
            "install": {
                "profileName": "Forge",
                "target": "1.7.10-Forge",
                "path": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
                "version": "Forge 10.13.4.1614",
                "filePath": "forge-1.7.10-universal.jar"
            },
            "versionInfo": {
                "id": "1.7.10-Forge-10.13.4.1614",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "libraries": []
            }
        }"#;

        let mock_universal_bytes = b"mock universal jar content";

        create_test_zip(
            &installer_path,
            &[
                ("install_profile.json", install_profile_json.as_bytes()),
                ("forge-1.7.10-universal.jar", mock_universal_bytes),
                ("big_logo.png", b"ignored logo"),
            ],
        );

        let extracted = extract_installer(&installer_path, &libraries_dir, &temp_dir)
            .expect("extract_installer legacy should succeed");

        assert_eq!(extracted.patch.id, "1.7.10-Forge-10.13.4.1614");
        assert_eq!(
            extracted.patch.main_class.as_deref(),
            Some("net.minecraft.launchwrapper.Launch")
        );
        assert!(extracted.modern.is_none());

        // Verify universal jar extracted to expected Maven location
        let expected_jar_path = libraries_dir.join(
            "net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10.jar",
        );
        assert!(expected_jar_path.exists());
        assert_eq!(fs::read(&expected_jar_path).unwrap(), mock_universal_bytes);
    }

    #[test]
    fn test_extract_installer_modern() {
        let temp = TempDirGuard::new("extract_modern");
        let installer_path = temp.path().join("installer-modern.jar");
        let libraries_dir = temp.path().join("libraries");
        let temp_dir = temp.path().join("temp");

        let install_profile_json = r#"{
            "spec": 1,
            "profile": "forge",
            "version": "1.20.1-forge-47.3.0",
            "json": "/version.json",
            "data": {},
            "processors": [
                {
                    "jar": "net.minecraftforge:installertools:1.4.1",
                    "classpath": [],
                    "args": []
                }
            ],
            "libraries": []
        }"#;

        let version_json = r#"{
            "id": "1.20.1-forge-47.3.0",
            "inheritsFrom": "1.20.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "libraries": []
        }"#;

        let mock_maven_lib = b"mock maven jar content";
        let mock_data_lzma = b"mock lzma data content";

        create_test_zip(
            &installer_path,
            &[
                ("install_profile.json", install_profile_json.as_bytes()),
                ("version.json", version_json.as_bytes()),
                (
                    "maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0.jar",
                    mock_maven_lib,
                ),
                ("data/client.lzma", mock_data_lzma),
            ],
        );

        let extracted = extract_installer(&installer_path, &libraries_dir, &temp_dir)
            .expect("extract_installer modern should succeed");

        assert_eq!(extracted.patch.id, "1.20.1-forge-47.3.0");
        assert_eq!(
            extracted.patch.main_class.as_deref(),
            Some("cpw.mods.bootstraplauncher.BootstrapLauncher")
        );
        let modern = extracted.modern.expect("must be modern");
        assert_eq!(modern.processors.len(), 1);

        // Verify maven/ library extracted to libraries/ without maven/ prefix
        let expected_maven_path =
            libraries_dir.join("net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0.jar");
        assert!(expected_maven_path.exists());
        assert_eq!(fs::read(&expected_maven_path).unwrap(), mock_maven_lib);

        // Verify data/ extracted to temp_dir
        let expected_data_path = temp_dir.join("data/client.lzma");
        assert!(expected_data_path.exists());
        assert_eq!(fs::read(&expected_data_path).unwrap(), mock_data_lzma);
    }

    #[test]
    fn test_extract_installer_missing_version_json_error() {
        let temp = TempDirGuard::new("extract_missing_vjson");
        let installer_path = temp.path().join("broken-installer.jar");
        let libraries_dir = temp.path().join("libraries");
        let temp_dir = temp.path().join("temp");

        let install_profile_json = r#"{
            "spec": 1,
            "profile": "forge",
            "version": "1.20.1",
            "json": "/version.json",
            "processors": [],
            "data": {},
            "libraries": []
        }"#;

        // No version.json in archive
        create_test_zip(
            &installer_path,
            &[("install_profile.json", install_profile_json.as_bytes())],
        );

        match extract_installer(&installer_path, &libraries_dir, &temp_dir) {
            Err(MinecraftError::ForgeMissingVersionJson) => {}
            Ok(_) => panic!("should fail when version.json is missing in modern profile"),
            Err(err) => panic!("expected ForgeMissingVersionJson, got: {err}"),
        }
    }

    #[test]
    fn test_read_main_class_success_and_missing() {
        let temp = TempDirGuard::new("main_class");
        let valid_jar = temp.path().join("valid.jar");
        let missing_jar = temp.path().join("missing.jar");

        let manifest_content =
            "Manifest-Version: 1.0\r\nMain-Class: net.minecraftforge.installer.SimpleTool\r\n\r\n";
        create_test_zip(
            &valid_jar,
            &[("META-INF/MANIFEST.MF", manifest_content.as_bytes())],
        );

        let parsed = read_main_class(&valid_jar).expect("should find Main-Class");
        assert_eq!(parsed, "net.minecraftforge.installer.SimpleTool");

        let no_main_manifest = "Manifest-Version: 1.0\r\nCreated-By: 1.8.0\r\n\r\n";
        create_test_zip(
            &missing_jar,
            &[("META-INF/MANIFEST.MF", no_main_manifest.as_bytes())],
        );

        let err = read_main_class(&missing_jar).expect_err("should fail without Main-Class");
        assert!(matches!(err, MinecraftError::MissingMainClass { .. }));
    }

    async fn spawn_mock_server(
        status_code: u16,
        status_text: &str,
        payload: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind mock server");
        let local_addr = listener.local_addr().expect("failed to get local addr");

        let status_text = status_text.to_string();
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let payload = payload.clone();
                let status_text = status_text.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let header = format!(
                        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status_code,
                        status_text,
                        payload.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&payload).await;
                    let _ = stream.flush().await;
                });
            }
        });

        (format!("http://{local_addr}"), handle)
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("failed to create test client")
    }

    #[tokio::test]
    async fn test_download_forge_installer_success() {
        let temp = TempDirGuard::new("installer_success");
        let payload = b"fake forge installer jar content".to_vec();
        let (server_url, handle) = spawn_mock_server(200, "OK", payload.clone()).await;

        let client = test_client();
        // Ensure parent directories are created automatically
        let dest = temp.path().join("nested").join("sub").join("installer.jar");
        let download_url = format!("{server_url}/forge-installer.jar");

        let result = download_installer(&client, &download_url, &dest).await;
        assert!(
            result.is_ok(),
            "download should succeed: {:?}",
            result.err()
        );

        let written = std::fs::read(&dest).expect("target file should exist");
        assert_eq!(written, payload);

        handle.abort();
    }

    #[tokio::test]
    async fn test_download_forge_installer_http_error() {
        let temp = TempDirGuard::new("installer_error");
        let (server_url, handle) = spawn_mock_server(404, "Not Found", vec![]).await;

        let client = test_client();
        let dest = temp.path().join("installer.jar");
        let download_url = format!("{server_url}/missing-installer.jar");

        let result = download_installer(&client, &download_url, &dest).await;
        assert!(result.is_err(), "should fail on 404 status code");
        assert!(!dest.exists(), "file should not be created on HTTP error");

        handle.abort();
    }

    #[test]
    fn test_neoforge_manifest_deserialization() {
        let json = r#"{
            "isSnapshot": false,
            "versions": [
                "20.2.3-beta",
                "20.4.195",
                "21.0.167",
                "21.1.72",
                "26.1.0.1-beta"
            ]
        }"#;

        let manifest: NeoForgeManifest =
            serde_json::from_str(json).expect("failed to deserialize NeoForgeManifest");
        assert_eq!(
            manifest.versions,
            vec![
                "20.2.3-beta",
                "20.4.195",
                "21.0.167",
                "21.1.72",
                "26.1.0.1-beta"
            ]
        );
    }

    #[test]
    fn test_neoforge_download_url() {
        assert_eq!(
            NeoForgeManifest::download_url("21.1.72"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.72/neoforge-21.1.72-installer.jar"
        );
        assert_eq!(
            NeoForgeManifest::download_url("20.4.195"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.195/neoforge-20.4.195-installer.jar"
        );
        // Legacy 1.20.1 pattern
        assert_eq!(
            NeoForgeManifest::download_url("1.20.1-47.1.106"),
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
        );
    }

    #[test]
    fn test_neoforge_prefix_rules() {
        // Legacy 1.20.1 pattern
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.20.1"), "1.20.1-");

        // Legacy 1.20.x ~ 1.21.x patterns
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.20.4"), "20.4.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.20.2"), "20.2.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.21"), "21.0.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.21.1"), "21.1.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("1.21.4"), "21.4.");

        // Year-based versioning patterns (26.1, 26.1.1)
        assert_eq!(NeoForgeManifest::neoforge_prefix("26.1"), "26.1.0.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("26.1.1"), "26.1.1.");
        assert_eq!(NeoForgeManifest::neoforge_prefix("26.2.0"), "26.2.0.");
    }

    #[test]
    fn test_neoforge_versions_for_mc() {
        let manifest = NeoForgeManifest {
            versions: vec![
                "1.20.1-47.1.84".to_string(),
                "1.20.1-47.1.106".to_string(),
                "20.4.195".to_string(),
                "20.4.237".to_string(),
                "21.0.167".to_string(),
                "21.1.72".to_string(),
                "21.1.120".to_string(),
                "26.1.0.1-beta".to_string(),
                "26.1.0.15".to_string(),
                "26.1.1.5".to_string(),
            ],
        };

        // 1.20.1 matches 1.20.1-x
        assert_eq!(
            manifest.versions_for_mc("1.20.1"),
            vec!["1.20.1-47.1.84", "1.20.1-47.1.106"]
        );

        // 1.20.4 matches 20.4.x
        assert_eq!(
            manifest.versions_for_mc("1.20.4"),
            vec!["20.4.195", "20.4.237"]
        );

        // 1.21 matches only 21.0.x, not 21.1.x
        assert_eq!(manifest.versions_for_mc("1.21"), vec!["21.0.167"]);

        // 1.21.1 matches 21.1.x
        assert_eq!(
            manifest.versions_for_mc("1.21.1"),
            vec!["21.1.72", "21.1.120"]
        );

        // 26.1 matches 26.1.0.x
        assert_eq!(
            manifest.versions_for_mc("26.1"),
            vec!["26.1.0.1-beta", "26.1.0.15"]
        );

        // 26.1.1 matches 26.1.1.x
        assert_eq!(manifest.versions_for_mc("26.1.1"), vec!["26.1.1.5"]);

        // Non-existent version returns empty
        assert!(manifest.versions_for_mc("1.19").is_empty());
        assert!(manifest.versions_for_mc("25.1").is_empty());
    }
}
