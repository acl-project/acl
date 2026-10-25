use std::{collections::HashMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tracing::{debug, warn};

use crate::{MinecraftError, Result, VersionEntry, VersionType, rule::Rule};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadFile {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDownloads {
    pub client: DownloadFile,
    pub server: Option<DownloadFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndexInfo {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(rename = "totalSize")]
    pub total_size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaVersionInfo {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryArtifact {
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<LibraryArtifact>,
    pub classifiers: Option<HashMap<String, LibraryArtifact>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractRules {
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub natives: Option<HashMap<String, String>>,
    pub rules: Option<Vec<Rule>>,
    pub extract: Option<ExtractRules>,
}

impl Library {
    /// Check whether this library should be enabled on Linux.
    pub fn is_allowed_on_linux(&self) -> bool {
        match &self.rules {
            Some(rules) => Rule::is_allowed_on_linux(rules),
            None => true,
        }
    }

    /// Convert Maven coordinate `group:artifact:version[:classifier]` into relative jar path. for old version and fabric
    pub fn maven_artifact_path(&self) -> Option<String> {
        let parts: Vec<&str> = self.name.split(':').collect();
        if parts.len() < 3 {
            return None;
        }

        let group_path = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];

        let file_name = if let Some(classifier) = parts.get(3) {
            format!("{artifact}-{version}-{classifier}.jar")
        } else {
            format!("{artifact}-{version}.jar")
        };

        Some(format!("{group_path}/{artifact}/{version}/{file_name}"))
    }
}

/// To handle compatibility with fields that may be either a single value or an array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s.clone()],
            Self::Multiple(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Simple(String),
    Conditional {
        rules: Vec<Rule>,
        value: StringOrVec,
    },
}

impl ArgumentEntry {
    /// Resolve argument values if allowed on Linux.
    pub fn resolve_for_linux(&self) -> Vec<String> {
        match self {
            Self::Simple(arg) => vec![arg.clone()],
            Self::Conditional { rules, value } => {
                if Rule::is_allowed_on_linux(rules) {
                    value.to_vec()
                } else {
                    Vec::new()
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionMetadata {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: VersionType,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    pub assets: Option<String>,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexInfo,
    pub downloads: VersionDownloads,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersionInfo>,
    pub libraries: Vec<Library>,
}

impl VersionMetadata {
    pub fn verify_sha1(bytes: &[u8], expected_sha1: &str, version_id: &str) -> Result<()> {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        let actual = hex::encode(hasher.finalize());

        if !actual.eq_ignore_ascii_case(expected_sha1) {
            return Err(MinecraftError::HashMismatch {
                target: version_id.to_string(),
                expected: expected_sha1.to_string(),
                actual,
            });
        }

        Ok(())
    }

    /// Fetch version metadata JSON from local cache (`versions/<id>/<id>.json`) or remote URL,
    /// verifying its SHA-1 against `entry.sha1`.
    pub async fn fetch_and_verify(
        client: &reqwest::Client,
        entry: &VersionEntry,
        versions_dir: &Path,
    ) -> Result<Self> {
        let version_json_path = versions_dir
            .join(&entry.id)
            .join(format!("{}.json", entry.id));
        // Check whether the local cache exists and is valid.
        if version_json_path.exists() {
            match fs::read(&version_json_path) {
                Ok(bytes) => {
                    if Self::verify_sha1(&bytes, &entry.sha1, &entry.id).is_ok()
                        && let Ok(metadata) = serde_json::from_slice::<Self>(&bytes)
                    {
                        debug!(
                            "Loaded verified version metadata for '{}' from {}",
                            entry.id,
                            version_json_path.display()
                        );
                        return Ok(metadata);
                    }

                    warn!(
                        "Local version metadata at {} failed verification or parsing, re-fetching",
                        version_json_path.display()
                    );
                }

                Err(err) => {
                    warn!(
                        "Failed to read cached version metadata at {}: {err}",
                        version_json_path.display()
                    );
                }
            }
        }

        let response = client.get(&entry.url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;

        Self::verify_sha1(&bytes, &entry.sha1, &entry.id)?;
        let metadata: Self = serde_json::from_slice(&bytes)?;

        if let Err(err) = acl_common::fs::write_atomic(&version_json_path, &bytes) {
            warn!(
                "Failed to save version metadata to {}: {err}",
                version_json_path.display()
            );
        }

        Ok(metadata)
    }

    /// Return all libraries that are enabled on Linux.
    pub fn active_libraries(&self) -> Vec<&Library> {
        self.libraries
            .iter()
            .filter(|lib| lib.is_allowed_on_linux())
            .collect()
    }

    /// Resolve game arguments for Linux (supports both modern `arguments.game` and legacy `minecraftArguments`).
    pub fn resolve_game_arguments(&self) -> Vec<String> {
        if let Some(args) = &self.arguments {
            args.game
                .iter()
                .flat_map(ArgumentEntry::resolve_for_linux)
                .collect()
        } else if let Some(legacy) = &self.minecraft_arguments {
            legacy.split_whitespace().map(str::to_string).collect()
        } else {
            Vec::new()
        }
    }

    /// Resolve JVM arguments for Linux.
    pub fn resolve_jvm_arguments(&self) -> Vec<String> {
        if let Some(args) = &self.arguments {
            args.jvm
                .iter()
                .flat_map(ArgumentEntry::resolve_for_linux)
                .collect()
        } else {
            Vec::new()
        }
    }
}
