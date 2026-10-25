use std::{collections::HashMap, path::Path};

use crate::{AssetIndexInfo, Result};

use serde::{Deserialize, Serialize};

pub const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    /// "<hash[..2]>/<hash>"
    pub fn relative_path(&self) -> Option<String> {
        let prefix = self.hash.get(..2)?;

        if self.hash.len() < 2 || !self.hash.chars().all(|c| c.is_ascii_hexdigit()) {
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
}
