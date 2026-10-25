use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{Result, Rule, VersionEntry, VersionType, download::DownloadTask};

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
    /// Fetch version metadata JSON from local cache (`versions/<id>/<id>.json`) or remote URL,
    /// verifying its SHA-1 against `entry.sha1`.
    pub async fn fetch_and_verify(
        client: &reqwest::Client,
        entry: &VersionEntry,
        versions_dir: &Path,
    ) -> Result<Self> {
        let path = versions_dir
            .join(&entry.id)
            .join(format!("{}.json", entry.id));

        crate::cache::fetch_verified_json(client, &entry.url, &entry.sha1, &entry.id, &path).await
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

    pub fn client_download_task(&self, versions_dir: &Path) -> DownloadTask {
        let client = &self.downloads.client;
        DownloadTask {
            name: format!("{}.jar", self.id),
            url: client.url.clone(),
            destination: versions_dir.join(&self.id).join(format!("{}.jar", self.id)),
            sha1: client.sha1.clone(),
            size: client.size,
        }
    }

    pub fn library_download_task(&self, libraries_dir: &Path) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();

        for lib in self.active_libraries() {
            let Some(downloads) = &lib.downloads else {
                continue;
            };

            if let Some(artifact) = &downloads.artifact {
                let rel_path = artifact.path.clone().or_else(|| lib.maven_artifact_path());
                if let Some(rel_path) = rel_path {
                    tasks.push(DownloadTask {
                        name: lib.name.clone(),
                        url: artifact.url.clone(),
                        destination: libraries_dir.join(rel_path),
                        sha1: artifact.sha1.clone(),
                        size: artifact.size,
                    });
                } else {
                    warn!(
                        "Skipping library '{}' with invalid Maven coordinate",
                        lib.name
                    );
                }
            }

            if let (Some(natives), Some(classifiers)) = (&lib.natives, &downloads.classifiers)
                && let Some(classifier_key) = natives.get("linux")
                && let Some(native_artifact) = classifiers.get(classifier_key)
                && let Some(rel_path) = &native_artifact.path
            {
                tasks.push(DownloadTask {
                    name: format!("{}:{classifier_key}", lib.name),
                    url: native_artifact.url.clone(),
                    destination: libraries_dir.join(rel_path),
                    sha1: native_artifact.sha1.clone(),
                    size: native_artifact.size,
                });
            }
        }
        tasks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule::{OsRule, RuleAction};

    #[test]
    fn test_maven_artifact_path() {
        let standard_lib = Library {
            name: "com.mojang:authlib:1.5.25".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert_eq!(
            standard_lib.maven_artifact_path(),
            Some("com/mojang/authlib/1.5.25/authlib-1.5.25.jar".to_string())
        );

        let classifier_lib = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert_eq!(
            classifier_lib.maven_artifact_path(),
            Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar".to_string())
        );

        let invalid_lib = Library {
            name: "invalid-name".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert_eq!(invalid_lib.maven_artifact_path(), None);
    }

    #[test]
    fn test_string_or_vec_to_vec() {
        let single = StringOrVec::Single("-Xms512M".to_string());
        assert_eq!(single.to_vec(), vec!["-Xms512M".to_string()]);

        let multiple =
            StringOrVec::Multiple(vec!["-XX:+UseG1GC".to_string(), "-Dfoo=bar".to_string()]);
        assert_eq!(
            multiple.to_vec(),
            vec!["-XX:+UseG1GC".to_string(), "-Dfoo=bar".to_string()]
        );
    }

    #[test]
    fn test_argument_entry_resolve_for_linux() {
        let simple = ArgumentEntry::Simple("--demo".to_string());
        assert_eq!(simple.resolve_for_linux(), vec!["--demo".to_string()]);

        let allowed_cond = ArgumentEntry::Conditional {
            rules: vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("linux".to_string()),
                    arch: None,
                }),
                features: None,
            }],
            value: StringOrVec::Multiple(vec![
                "-Dos.name=Linux".to_string(),
                "-Dlinux.only=true".to_string(),
            ]),
        };
        assert_eq!(
            allowed_cond.resolve_for_linux(),
            vec![
                "-Dos.name=Linux".to_string(),
                "-Dlinux.only=true".to_string()
            ]
        );

        let disallowed_cond = ArgumentEntry::Conditional {
            rules: vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("windows".to_string()),
                    arch: None,
                }),
                features: None,
            }],
            value: StringOrVec::Single("-Dwindows.only=true".to_string()),
        };
        assert!(disallowed_cond.resolve_for_linux().is_empty());
    }

    #[test]
    fn test_active_libraries() {
        let linux_lib = Library {
            name: "org.lwjgl:lwjgl-linux:3.3.3".to_string(),
            downloads: None,
            natives: None,
            rules: Some(vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("linux".to_string()),
                    arch: None,
                }),
                features: None,
            }]),
            extract: None,
        };

        let windows_lib = Library {
            name: "org.lwjgl:lwjgl-windows:3.3.3".to_string(),
            downloads: None,
            natives: None,
            rules: Some(vec![Rule {
                action: RuleAction::Allow,
                os: Some(OsRule {
                    name: Some("windows".to_string()),
                    arch: None,
                }),
                features: None,
            }]),
            extract: None,
        };

        let common_lib = Library {
            name: "com.google.guava:guava:32.1.2-jre".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };

        let metadata = VersionMetadata {
            id: "1.21".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            minecraft_arguments: None,
            arguments: None,
            assets: Some("17".to_string()),
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 100,
                total_size: 200,
                url: "https://example.com/assets.json".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "client_sha1".to_string(),
                    size: 1000,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![linux_lib.clone(), windows_lib, common_lib.clone()],
        };

        let active = metadata.active_libraries();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].name, linux_lib.name);
        assert_eq!(active[1].name, common_lib.name);
    }

    #[test]
    fn test_resolve_game_and_jvm_arguments() {
        let metadata_modern = VersionMetadata {
            id: "1.21".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            minecraft_arguments: None,
            arguments: Some(Arguments {
                game: vec![
                    ArgumentEntry::Simple("--username".to_string()),
                    ArgumentEntry::Simple("${auth_player_name}".to_string()),
                ],
                jvm: vec![
                    ArgumentEntry::Simple("-Xmx2G".to_string()),
                    ArgumentEntry::Conditional {
                        rules: vec![Rule {
                            action: RuleAction::Disallow,
                            os: Some(OsRule {
                                name: Some("linux".to_string()),
                                arch: None,
                            }),
                            features: None,
                        }],
                        value: StringOrVec::Single("-Dwindows.only=true".to_string()),
                    },
                ],
            }),
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 100,
                total_size: 200,
                url: "https://example.com".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "sha".to_string(),
                    size: 10,
                    url: "url".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: Vec::new(),
        };

        assert_eq!(
            metadata_modern.resolve_game_arguments(),
            vec!["--username", "${auth_player_name}"]
        );
        assert_eq!(metadata_modern.resolve_jvm_arguments(), vec!["-Xmx2G"]);

        let metadata_legacy = VersionMetadata {
            id: "1.12.2".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            minecraft_arguments: Some(
                "--username ${auth_player_name} --version ${version_name}".to_string(),
            ),
            arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "1.12".to_string(),
                sha1: "def".to_string(),
                size: 50,
                total_size: 100,
                url: "https://example.com".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "sha".to_string(),
                    size: 10,
                    url: "url".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: Vec::new(),
        };

        assert_eq!(
            metadata_legacy.resolve_game_arguments(),
            vec![
                "--username",
                "${auth_player_name}",
                "--version",
                "${version_name}"
            ]
        );
        assert!(metadata_legacy.resolve_jvm_arguments().is_empty());
    }

    #[test]
    fn test_version_metadata_deserialization() {
        let json_data = r#"{
            "id": "1.21.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": {
                "id": "17",
                "sha1": "0123456789abcdef0123456789abcdef01234567",
                "size": 12345,
                "totalSize": 67890,
                "url": "https://resources.download.minecraft.net/17"
            },
            "downloads": {
                "client": {
                    "sha1": "abcdef0123456789abcdef0123456789abcdef01",
                    "size": 54321,
                    "url": "https://piston-data.mojang.com/v1/objects/client.jar"
                },
                "server": {
                    "sha1": "fedcba9876543210fedcba9876543210fedcba98",
                    "size": 65432,
                    "url": "https://piston-data.mojang.com/v1/objects/server.jar"
                }
            },
            "javaVersion": {
                "component": "java-runtime-gamma",
                "majorVersion": 21
            },
            "libraries": [
                {
                    "name": "com.mojang:authlib:6.0.55",
                    "downloads": {
                        "artifact": {
                            "path": "com/mojang/authlib/6.0.55/authlib-6.0.55.jar",
                            "sha1": "1111222233334444555566667777888899990000",
                            "size": 1024,
                            "url": "https://libraries.minecraft.net/authlib.jar"
                        }
                    }
                }
            ]
        }"#;

        let parsed: VersionMetadata =
            serde_json::from_str(json_data).expect("Failed to parse version metadata");
        assert_eq!(parsed.id, "1.21.1");
        assert_eq!(parsed.version_type, VersionType::Release);
        assert_eq!(parsed.main_class, "net.minecraft.client.main.Main");
        assert_eq!(parsed.downloads.client.size, 54321);
        assert!(parsed.downloads.server.is_some());
        assert_eq!(
            parsed.java_version.as_ref().map(|j| j.major_version),
            Some(21)
        );
        assert_eq!(parsed.libraries.len(), 1);
        assert_eq!(parsed.libraries[0].name, "com.mojang:authlib:6.0.55");
    }
}
