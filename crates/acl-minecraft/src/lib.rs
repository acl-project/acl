pub mod asset;
pub mod cache;
pub mod download;
pub mod error;
pub mod manifest;
pub mod rule;
pub mod version;

pub use asset::{AssetIndex, AssetObject, RESOURCES_BASE_URL};
pub use cache::{Cached, DEFAULT_MANIFEST_TTL, cache_key_for_url};
pub use error::{MinecraftError, Result};
pub use manifest::{
    LatestVersions, VERSION_MANIFEST_URL, VersionEntry, VersionManifest, VersionType,
};
pub use rule::{OsRule, Rule, RuleAction};
pub use version::{
    ArgumentEntry, Arguments, AssetIndexInfo, DownloadFile, ExtractRules, JavaVersionInfo, Library,
    LibraryArtifact, LibraryDownloads, StringOrVec, VersionDownloads, VersionMetadata,
};
