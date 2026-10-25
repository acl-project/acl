use std::{collections::HashMap, fs, os::unix::fs::symlink, path::Path};

use crate::{AssetIndexInfo, MinecraftError, Result};

use serde::{Deserialize, Serialize};
use tracing::warn;

pub const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

fn link_asset_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.symlink_metadata().is_ok() {
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    let abs_src = src.canonicalize()?;

    // Prefer hard links; if crossing mount points (EXDEV), automatically fall back to symbolic links.
    fs::hard_link(&abs_src, dst).or_else(|_| symlink(abs_src, dst))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    /// "<hash[..2]>/<hash>"
    pub fn relative_path(&self) -> Option<String> {
        let prefix = self.hash.get(..2)?;

        if !self.hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }

        Some(format!("{prefix}/{}", self.hash))
    }

    pub fn download_url(&self) -> Option<String> {
        Some(format!("{RESOURCES_BASE_URL}/{}", self.relative_path()?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
    #[serde(default)]
    pub map_to_resources: bool,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
}

impl AssetIndex {
    pub async fn fetch_and_verify(
        client: &reqwest::Client,
        info: &AssetIndexInfo,
        assets_dir: &Path,
    ) -> Result<Self> {
        let path = assets_dir.join("indexes").join(format!("{}.json", info.id));
        crate::cache::fetch_verified_json(client, &info.url, &info.sha1, &info.id, &path).await
    }

    /// Reconstruct virtual or resource-mapped assets for legacy Minecraft versions using links.
    pub fn reconstruct_legacy_assets(
        &self,
        index_id: &str,
        assets_dir: &Path,
        game_dir: &Path,
    ) -> Result<()> {
        let target_root = if self.map_to_resources {
            game_dir.join("resources")
        } else if self.is_virtual {
            assets_dir.join("virtual").join(index_id)
        } else {
            return Ok(());
        };

        for (logical_path, obj) in &self.objects {
            let rel_dest = Path::new(logical_path);
            // Prevent path traversal.
            if rel_dest
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                warn!("Skipping unsafe legacy asset path: {logical_path}");
                continue;
            }
            let Some(rel_obj) = obj.relative_path() else {
                warn!(
                    "Skipping asset '{logical_path}' with invalid hash '{}'",
                    obj.hash
                );
                continue;
            };

            let src = assets_dir.join("objects").join(rel_obj);
            let dst = target_root.join(rel_dest);

            link_asset_file(&src, &dst)
                .map_err(|source| MinecraftError::Io { path: dst, source })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_object_relative_path_and_download_url_valid() {
        let object = AssetObject {
            hash: "bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598".to_string(),
            size: 3665,
        };

        assert_eq!(
            object.relative_path().as_deref(),
            Some("bd/bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598")
        );
        assert_eq!(
            object.download_url().as_deref(),
            Some(
                "https://resources.download.minecraft.net/bd/bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598"
            )
        );
    }

    #[test]
    fn test_asset_object_relative_path_invalid() {
        let empty = AssetObject {
            hash: "".to_string(),
            size: 0,
        };
        assert_eq!(empty.relative_path(), None);
        assert_eq!(empty.download_url(), None);

        let single_char = AssetObject {
            hash: "a".to_string(),
            size: 0,
        };
        assert_eq!(single_char.relative_path(), None);
        assert_eq!(single_char.download_url(), None);

        let non_hex = AssetObject {
            hash: "zz00000000000000000000000000000000000000".to_string(),
            size: 0,
        };
        assert_eq!(non_hex.relative_path(), None);
        assert_eq!(non_hex.download_url(), None);
    }

    #[test]
    fn test_asset_index_deserialization() {
        let json_data = r#"{
            "objects": {
                "icons/icon_16x16.png": {
                    "hash": "bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598",
                    "size": 3665
                },
                "minecraft/sounds/ambient/cave/cave1.ogg": {
                    "hash": "88523c0612c62c96c4664da6929a8a3a79d0bcf3",
                    "size": 24976
                }
            },
            "map_to_resources": true,
            "virtual": true
        }"#;

        let index: AssetIndex =
            serde_json::from_str(json_data).expect("failed to deserialize asset index");
        assert!(index.map_to_resources);
        assert!(index.is_virtual);
        assert_eq!(index.objects.len(), 2);

        let icon = index
            .objects
            .get("icons/icon_16x16.png")
            .expect("icon object should exist");
        assert_eq!(icon.hash, "bdf48ef6b5d0d23ebb4f8a6179d4a1bfab1a9598");
        assert_eq!(icon.size, 3665);
    }

    #[test]
    fn test_asset_index_deserialization_defaults() {
        let json_data = r#"{
            "objects": {}
        }"#;

        let index: AssetIndex =
            serde_json::from_str(json_data).expect("failed to deserialize minimal asset index");
        assert!(!index.map_to_resources);
        assert!(!index.is_virtual);
        assert!(index.objects.is_empty());
    }

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_asset_{}_{}_{}",
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

    #[tokio::test]
    async fn test_asset_index_fetch_and_verify_from_cache() {
        use sha1::{Digest, Sha1};

        let temp = TempDirGuard::new("fetch_and_verify");
        let indexes_dir = temp.path().join("indexes");
        std::fs::create_dir_all(&indexes_dir).expect("failed to create indexes dir");

        let index_path = indexes_dir.join("test_index.json");
        let json_data = r#"{"objects":{}}"#;
        std::fs::write(&index_path, json_data).expect("failed to write index json");

        let mut hasher = Sha1::new();
        hasher.update(json_data.as_bytes());
        let sha1 = hex::encode(hasher.finalize());

        let info = AssetIndexInfo {
            id: "test_index".to_string(),
            sha1,
            size: json_data.len() as u64,
            url: "http://127.0.0.1:9/unused".to_string(),
            total_size: json_data.len() as u64,
        };

        let client = reqwest::Client::new();
        let result = AssetIndex::fetch_and_verify(&client, &info, temp.path()).await;

        let index = result.expect("fetch_and_verify should read from cached file");
        assert!(index.objects.is_empty());
    }
}
