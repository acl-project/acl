use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    Result, Rule, VersionEntry, VersionType, download::DownloadTask, native::NativeArchive,
};

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
    /// Resolve relative artifact path from explicit `artifact.path` or Maven coordinate fallback.
    pub fn artifact_relative_path(&self, artifact: &LibraryArtifact) -> Option<String> {
        artifact.path.clone().or_else(|| self.maven_artifact_path())
    }

    /// Check whether the Maven coordinate represents a modern Linux native artifact (1.19+).
    /// For example: `org.lwjgl:lwjgl:3.3.3:natives-linux`
    pub fn is_modern_linux_native(&self) -> bool {
        self.name
            .split(':')
            .nth(3)
            .is_some_and(|classifier| classifier == "natives-linux")
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
    /// Minecraft version id not instance id
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
        instance_id: &str,
        versions_dir: &Path,
    ) -> Result<Self> {
        let path = versions_dir
            .join(instance_id)
            .join(format!("{instance_id}.json"));

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
            // for old version
            vec![
                "-Djava.library.path=${natives_directory}".to_owned(),
                "-cp".to_owned(),
                "${classpath}".to_owned(),
            ]
        }
    }

    pub fn client_download_task(&self, instance_id: &str, versions_dir: &Path) -> DownloadTask {
        let client = &self.downloads.client;
        DownloadTask {
            name: format!("{}.jar", instance_id),
            url: client.url.clone(),
            destination: versions_dir
                .join(instance_id)
                .join(format!("{}.jar", instance_id)),
            sha1: client.sha1.clone(),
            size: client.size,
        }
    }

    pub fn library_download_tasks(&self, libraries_dir: &Path) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();
        let mut seen_hashed = HashSet::new();
        for lib in self.active_libraries() {
            let Some(downloads) = &lib.downloads else {
                continue;
            };

            if let Some(artifact) = &downloads.artifact {
                if !seen_hashed.insert(artifact.sha1.clone()) {
                    continue;
                }

                let rel_path = lib.artifact_relative_path(artifact);
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
                if !seen_hashed.insert(native_artifact.sha1.clone()) {
                    continue;
                }

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

    pub fn native_archives(&self, libraries_dir: &Path) -> Vec<NativeArchive> {
        let mut archives = Vec::new();
        let mut seen_paths = HashSet::new();

        for lib in self.active_libraries() {
            let Some(downloads) = &lib.downloads else {
                continue;
            };

            let exclude = match &lib.extract {
                Some(ExtractRules {
                    exclude: Some(list),
                }) => list.clone(),
                _ => Vec::new(),
            };

            if let (Some(natives), Some(classifiers)) = (&lib.natives, &downloads.classifiers)
                && let Some(classifier_key) = natives.get("linux")
                && let Some(native_artifact) = classifiers.get(classifier_key)
                && let Some(rel_path) = &native_artifact.path
            {
                let archive_path = libraries_dir.join(rel_path);
                if seen_paths.insert(archive_path.clone()) {
                    archives.push(NativeArchive {
                        name: format!("{}:{classifier_key}", lib.name),
                        archive_path,
                        exclude: exclude.clone(),
                    });
                }
            }

            if lib.is_modern_linux_native()
                && let Some(artifact) = &downloads.artifact
                && let Some(rel_path) = lib.artifact_relative_path(artifact)
            {
                let archive_path = libraries_dir.join(rel_path);
                if seen_paths.insert(archive_path.clone()) {
                    archives.push(NativeArchive {
                        name: lib.name.clone(),
                        archive_path,
                        exclude,
                    });
                }
            }
        }
        archives
    }

    /// Extract all active Linux native libraries into `natives_dir`.
    pub fn extract_natives(&self, libraries_dir: &Path, natives_dir: &Path) -> Result<()> {
        let archives = self.native_archives(libraries_dir);
        crate::native::extract_all_natives(&archives, natives_dir)
    }

    /// Build the ordered list of classpath jar paths (all active library artifacts + client jar).
    pub fn classpath_entries(
        &self,
        libraries_dir: &Path,
        instance_id: &str,
        versions_dir: &Path,
    ) -> Vec<PathBuf> {
        let mut entries = Vec::new();
        let mut seen_paths = HashSet::new();

        for lib in self.active_libraries() {
            let Some(downloads) = &lib.downloads else {
                continue;
            };

            if let Some(artifact) = &downloads.artifact {
                if let Some(rel_path) = lib.artifact_relative_path(artifact) {
                    let full_path = libraries_dir.join(rel_path);
                    if seen_paths.insert(full_path.clone()) {
                        entries.push(full_path);
                    }
                } else {
                    warn!(
                        "Skipping library '{}' in classpath with invalid Maven coordinate",
                        lib.name
                    );
                }
            }
        }

        entries.push(
            versions_dir
                .join(instance_id)
                .join(format!("{instance_id}.jar")),
        );
        entries
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
        assert_eq!(
            metadata_legacy.resolve_jvm_arguments(),
            vec![
                "-Djava.library.path=${natives_directory}",
                "-cp",
                "${classpath}"
            ]
        );
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

    #[test]
    fn test_client_download_task() {
        let metadata = VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            arguments: None,
            minecraft_arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 10,
                total_size: 20,
                url: "https://example.com/asset".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "abcdef0123456789".to_string(),
                    size: 54321,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: Vec::new(),
        };

        let versions_dir = Path::new("/mock/versions");
        let task = metadata.client_download_task("1.21.1", versions_dir);

        assert_eq!(task.name, "1.21.1.jar");
        assert_eq!(task.url, "https://example.com/client.jar");
        assert_eq!(task.sha1, "abcdef0123456789");
        assert_eq!(task.size, 54321);
        assert_eq!(
            task.destination,
            versions_dir.join("1.21.1").join("1.21.1.jar")
        );
    }

    #[test]
    fn test_library_download_tasks() {
        let libraries_dir = Path::new("/mock/libraries");

        // 1. Standard library with explicit path
        let lib_standard = Library {
            name: "com.mojang:authlib:6.0.55".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("com/mojang/authlib/6.0.55/authlib-6.0.55.jar".to_string()),
                    sha1: "sha_authlib".to_string(),
                    size: 1000,
                    url: "https://example.com/authlib.jar".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // 2. Library with artifact path=None (should fallback to maven_artifact_path)
        let lib_maven_fallback = Library {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: None,
                    sha1: "sha_lwjgl".to_string(),
                    size: 2000,
                    url: "https://example.com/lwjgl.jar".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // 3. Library with native classifier for linux
        let mut natives_map = HashMap::new();
        natives_map.insert("linux".to_string(), "natives-linux".to_string());
        let mut classifiers_map = HashMap::new();
        classifiers_map.insert(
            "natives-linux".to_string(),
            LibraryArtifact {
                path: Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar".to_string()),
                sha1: "sha_lwjgl_natives".to_string(),
                size: 3000,
                url: "https://example.com/lwjgl-natives.jar".to_string(),
            },
        );
        let lib_native = Library {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: Some(classifiers_map),
            }),
            natives: Some(natives_map),
            rules: None,
            extract: None,
        };

        // 4. Duplicate library sharing same SHA1 as lib_standard (should be deduplicated)
        let lib_duplicate = Library {
            name: "com.mojang:authlib-duplicate:6.0.55".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("com/mojang/authlib/6.0.55/authlib-6.0.55.jar".to_string()),
                    sha1: "sha_authlib".to_string(),
                    size: 1000,
                    url: "https://example.com/authlib-dup.jar".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // 5. Library disallowed on Linux (should be excluded)
        let lib_windows_only = Library {
            name: "windows:only:1.0".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("windows/only/1.0/only-1.0.jar".to_string()),
                    sha1: "sha_win".to_string(),
                    size: 500,
                    url: "https://example.com/win.jar".to_string(),
                }),
                classifiers: None,
            }),
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

        let metadata = VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            arguments: None,
            minecraft_arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 10,
                total_size: 20,
                url: "https://example.com/asset".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "client_sha1".to_string(),
                    size: 100,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![
                lib_standard,
                lib_maven_fallback,
                lib_native,
                lib_duplicate,
                lib_windows_only,
            ],
        };

        let tasks = metadata.library_download_tasks(libraries_dir);

        // Expected 3 tasks: standard, maven fallback, and linux native (duplicate & windows-only excluded)
        assert_eq!(tasks.len(), 3);

        let standard_task = tasks
            .iter()
            .find(|t| t.sha1 == "sha_authlib")
            .expect("standard task should exist");
        assert_eq!(
            standard_task.destination,
            libraries_dir.join("com/mojang/authlib/6.0.55/authlib-6.0.55.jar")
        );
        assert_eq!(standard_task.size, 1000);

        let fallback_task = tasks
            .iter()
            .find(|t| t.sha1 == "sha_lwjgl")
            .expect("fallback task should exist");
        assert_eq!(
            fallback_task.destination,
            libraries_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar")
        );
        assert_eq!(fallback_task.size, 2000);

        let native_task = tasks
            .iter()
            .find(|t| t.sha1 == "sha_lwjgl_natives")
            .expect("native task should exist");
        assert_eq!(
            native_task.destination,
            libraries_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar")
        );
        assert_eq!(native_task.name, "org.lwjgl:lwjgl:3.3.3:natives-linux");
        assert_eq!(native_task.size, 3000);
    }

    #[test]
    fn test_artifact_relative_path_and_is_modern_linux_native() {
        let lib_explicit = Library {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        let artifact_explicit = LibraryArtifact {
            path: Some("explicit/path/to/lib.jar".to_string()),
            sha1: "abc".to_string(),
            size: 10,
            url: "https://example.com".to_string(),
        };
        assert_eq!(
            lib_explicit.artifact_relative_path(&artifact_explicit),
            Some("explicit/path/to/lib.jar".to_string())
        );

        let artifact_fallback = LibraryArtifact {
            path: None,
            sha1: "abc".to_string(),
            size: 10,
            url: "https://example.com".to_string(),
        };
        assert_eq!(
            lib_explicit.artifact_relative_path(&artifact_fallback),
            Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar".to_string())
        );

        let lib_invalid = Library {
            name: "invalid-coordinate".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert_eq!(lib_invalid.artifact_relative_path(&artifact_fallback), None);

        let modern_native = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert!(modern_native.is_modern_linux_native());

        let modern_windows = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-windows".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert!(!modern_windows.is_modern_linux_native());

        let standard_lib = Library {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
        };
        assert!(!standard_lib.is_modern_linux_native());
    }

    #[test]
    fn test_native_archives() {
        let libraries_dir = Path::new("/mock/libraries");

        // 1. Legacy native library with classifiers
        let mut natives_map = HashMap::new();
        natives_map.insert("linux".to_string(), "natives-linux".to_string());
        let mut classifiers_map = HashMap::new();
        classifiers_map.insert(
            "natives-linux".to_string(),
            LibraryArtifact {
                path: Some(
                    "org/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4-natives-linux.jar"
                        .to_string(),
                ),
                sha1: "sha_legacy".to_string(),
                size: 100,
                url: "https://example.com".to_string(),
            },
        );
        let lib_legacy = Library {
            name: "org.lwjgl:lwjgl-platform:2.9.4".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: Some(classifiers_map),
            }),
            natives: Some(natives_map),
            rules: None,
            extract: Some(ExtractRules {
                exclude: Some(vec!["META-INF/".to_string()]),
            }),
        };

        // 2. Modern native library (1.19+)
        let lib_modern = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar".to_string()),
                    sha1: "sha_modern".to_string(),
                    size: 200,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // 3. Modern native with fallback path
        let lib_modern_fallback = Library {
            name: "org.lwjgl:lwjgl-jemalloc:3.3.3:natives-linux".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: None,
                    sha1: "sha_jemalloc".to_string(),
                    size: 300,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // 4. Windows-only library
        let lib_windows = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-windows".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar".to_string()),
                    sha1: "sha_win".to_string(),
                    size: 200,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
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

        // 5. Duplicate of lib_modern (should be deduplicated)
        let lib_modern_dup = lib_modern.clone();

        let metadata = VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            arguments: None,
            minecraft_arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 10,
                total_size: 20,
                url: "https://example.com".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "sha_client".to_string(),
                    size: 1000,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![
                lib_legacy,
                lib_modern,
                lib_modern_fallback,
                lib_windows,
                lib_modern_dup,
            ],
        };

        let archives = metadata.native_archives(libraries_dir);
        assert_eq!(archives.len(), 3);

        assert_eq!(
            archives[0].name,
            "org.lwjgl:lwjgl-platform:2.9.4:natives-linux"
        );
        assert_eq!(
            archives[0].archive_path,
            libraries_dir
                .join("org/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4-natives-linux.jar")
        );
        assert_eq!(archives[0].exclude, vec!["META-INF/"]);

        assert_eq!(archives[1].name, "org.lwjgl:lwjgl:3.3.3:natives-linux");
        assert_eq!(
            archives[1].archive_path,
            libraries_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar")
        );
        assert!(archives[1].exclude.is_empty());

        assert_eq!(
            archives[2].name,
            "org.lwjgl:lwjgl-jemalloc:3.3.3:natives-linux"
        );
        assert_eq!(
            archives[2].archive_path,
            libraries_dir
                .join("org/lwjgl/lwjgl-jemalloc/3.3.3/lwjgl-jemalloc-3.3.3-natives-linux.jar")
        );
        assert!(archives[2].exclude.is_empty());
    }

    #[test]
    fn test_classpath_entries() {
        let libraries_dir = Path::new("/mock/libraries");
        let versions_dir = Path::new("/mock/versions");

        let lib1 = Library {
            name: "com.mojang:authlib:6.0.55".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("com/mojang/authlib/6.0.55/authlib-6.0.55.jar".to_string()),
                    sha1: "sha1".to_string(),
                    size: 100,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        let lib2 = Library {
            name: "org.lwjgl:lwjgl:3.3.3".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: None,
                    sha1: "sha2".to_string(),
                    size: 200,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        // Disallowed on Linux
        let lib_windows = Library {
            name: "win:only:1.0".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("win/only/1.0/only.jar".to_string()),
                    sha1: "sha3".to_string(),
                    size: 300,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
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

        // Duplicate of lib1 (same path, should be deduplicated)
        let lib_dup = Library {
            name: "com.mojang:authlib-alias:6.0.55".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some("com/mojang/authlib/6.0.55/authlib-6.0.55.jar".to_string()),
                    sha1: "sha1".to_string(),
                    size: 100,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        let metadata = VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            arguments: None,
            minecraft_arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 10,
                total_size: 20,
                url: "https://example.com".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "sha_client".to_string(),
                    size: 1000,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![lib1, lib2, lib_windows, lib_dup],
        };

        let cp = metadata.classpath_entries(libraries_dir, "1.21.1", versions_dir);
        assert_eq!(
            cp,
            vec![
                libraries_dir.join("com/mojang/authlib/6.0.55/authlib-6.0.55.jar"),
                libraries_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar"),
                versions_dir.join("1.21.1").join("1.21.1.jar"),
            ]
        );
    }

    #[test]
    fn test_extract_natives() {
        use std::{
            fs::File,
            io::Write,
            sync::atomic::{AtomicUsize, Ordering},
        };
        use zip::{ZipWriter, write::SimpleFileOptions};

        struct TempDirGuard {
            path: PathBuf,
        }

        impl TempDirGuard {
            fn new(name: &str) -> Self {
                static COUNTER: AtomicUsize = AtomicUsize::new(0);
                let id = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!(
                    "acl_test_version_extract_{}_{}_{}",
                    name,
                    std::process::id(),
                    id
                ));
                let _ = std::fs::remove_dir_all(&path);
                std::fs::create_dir_all(&path).expect("failed to create temp test dir");
                Self { path }
            }

            fn path(&self) -> &Path {
                &self.path
            }
        }

        impl Drop for TempDirGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }

        let temp = TempDirGuard::new("extract_natives");
        let libraries_dir = temp.path().join("libraries");
        let natives_dir = temp.path().join("natives");
        let rel_jar_path = "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar";
        let jar_path = libraries_dir.join(rel_jar_path);

        if let Some(parent) = jar_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create library parent dir");
        }
        std::fs::create_dir_all(&natives_dir).expect("failed to create natives dir");

        // Create a mock native jar
        let file = File::create(&jar_path).expect("failed to create jar file");
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("liblwjgl.so", options)
            .expect("failed to add file to zip");
        zip.write_all(b"ELF_NATIVE_CONTENT")
            .expect("failed to write zip content");
        zip.finish().expect("failed to finish zip");

        let lib_native = Library {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            downloads: Some(LibraryDownloads {
                artifact: Some(LibraryArtifact {
                    path: Some(rel_jar_path.to_string()),
                    sha1: "sha_native".to_string(),
                    size: 100,
                    url: "https://example.com".to_string(),
                }),
                classifiers: None,
            }),
            natives: None,
            rules: None,
            extract: None,
        };

        let metadata = VersionMetadata {
            id: "1.21.1".to_string(),
            version_type: VersionType::Release,
            main_class: "net.minecraft.client.main.Main".to_string(),
            arguments: None,
            minecraft_arguments: None,
            assets: None,
            asset_index: AssetIndexInfo {
                id: "17".to_string(),
                sha1: "abc".to_string(),
                size: 10,
                total_size: 20,
                url: "https://example.com".to_string(),
            },
            downloads: VersionDownloads {
                client: DownloadFile {
                    sha1: "sha_client".to_string(),
                    size: 1000,
                    url: "https://example.com/client.jar".to_string(),
                },
                server: None,
            },
            java_version: None,
            libraries: vec![lib_native],
        };

        metadata
            .extract_natives(&libraries_dir, &natives_dir)
            .expect("extract_natives should succeed");

        assert_eq!(
            std::fs::read(natives_dir.join("liblwjgl.so"))
                .expect("extracted liblwjgl.so should exist"),
            b"ELF_NATIVE_CONTENT"
        );
    }
}
