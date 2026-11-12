// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    #[default]
    Official,
    Bmclapi,
}

/// Ordered prefix replacement rules for BMCLAPI.
/// Note: Longer/more specific prefixes MUST come before shorter ones.
const BMCLAPI_RULES: &[(&str, &str)] = &[
    // Mojang Meta & Java runtime
    (
        "https://piston-meta.mojang.com",
        "https://bmclapi2.bangbang93.com",
    ),
    (
        "https://launchermeta.mojang.com",
        "https://bmclapi2.bangbang93.com",
    ),
    // Mojang Version Data / Client JAR / AssetIndex
    (
        "https://piston-data.mojang.com",
        "https://bmclapi2.bangbang93.com",
    ),
    (
        "https://launcher.mojang.com",
        "https://bmclapi2.bangbang93.com",
    ),
    // Assets & Libraries
    (
        "https://resources.download.minecraft.net",
        "https://bmclapi2.bangbang93.com/assets",
    ),
    (
        "https://libraries.minecraft.net",
        "https://bmclapi2.bangbang93.com/maven",
    ),
    // Forge & NeoForge
    (
        "https://files.minecraftforge.net/maven",
        "https://bmclapi2.bangbang93.com/maven",
    ),
    (
        "https://maven.neoforged.net/releases/net/neoforged/forge",
        "https://bmclapi2.bangbang93.com/maven/net/neoforged/forge",
    ),
    (
        "https://maven.neoforged.net/releases/net/neoforged/neoforge",
        "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge",
    ),
    // Fabric & Quilt
    (
        "https://meta.fabricmc.net",
        "https://bmclapi2.bangbang93.com/fabric-meta",
    ),
    (
        "https://maven.fabricmc.net",
        "https://bmclapi2.bangbang93.com/maven",
    ),
    // Quilt's bmclapi is not available
    /*
    (
        "https://maven.quiltmc.org/repository/release",
        "https://bmclapi2.bangbang93.com/maven",
    ),
    (
        "https://meta.quiltmc.org",
        "https://bmclapi2.bangbang93.com/quilt-meta",
    ),
    */
    // Authlib-injector
    (
        "https://authlib-injector.yushi.moe",
        "https://bmclapi2.bangbang93.com/mirrors/authlib-injector",
    ),
];

impl DownloadSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Official => "Official (Mojang)",
            Self::Bmclapi => "BMCLAPI (Mirror)",
        }
    }

    /// Rewrite an upstream URL according to the selected download source.
    pub fn rewrite_url(self, url: &str) -> String {
        match self {
            Self::Official => url.to_string(),
            Self::Bmclapi => {
                for &(from, to) in BMCLAPI_RULES {
                    if let Some(rest) = url.strip_prefix(from) {
                        return format!("{to}{rest}");
                    }
                }

                url.to_string()
            }
        }
    }

    pub const ALL: &[Self] = &[Self::Official, Self::Bmclapi];

    pub fn to_index(self) -> u32 {
        match self {
            Self::Official => 0,
            Self::Bmclapi => 1,
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Bmclapi,
            _ => Self::Official,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_source_default() {
        assert_eq!(DownloadSource::default(), DownloadSource::Official);
    }

    #[test]
    fn test_download_source_label() {
        assert_eq!(DownloadSource::Official.label(), "Official (Mojang)");
        assert_eq!(DownloadSource::Bmclapi.label(), "BMCLAPI (Mirror)");
    }

    #[test]
    fn test_rewrite_url_official() {
        let url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        assert_eq!(DownloadSource::Official.rewrite_url(url), url);
    }

    #[test]
    fn test_rewrite_url_bmclapi_rules() {
        let cases = [
            (
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
                "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
            ),
            (
                "https://launchermeta.mojang.com/mc/game/version_manifest.json",
                "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json",
            ),
            (
                "https://piston-data.mojang.com/v1/objects/client.jar",
                "https://bmclapi2.bangbang93.com/v1/objects/client.jar",
            ),
            (
                "https://launcher.mojang.com/v1/objects/client.jar",
                "https://bmclapi2.bangbang93.com/v1/objects/client.jar",
            ),
            (
                "https://resources.download.minecraft.net/bd/hash123",
                "https://bmclapi2.bangbang93.com/assets/bd/hash123",
            ),
            (
                "https://libraries.minecraft.net/com/mojang/authlib/1.0/authlib-1.0.jar",
                "https://bmclapi2.bangbang93.com/maven/com/mojang/authlib/1.0/authlib-1.0.jar",
            ),
            (
                "https://files.minecraftforge.net/maven/net/minecraftforge/forge/test.jar",
                "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/test.jar",
            ),
            (
                "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.4/forge.jar",
                "https://bmclapi2.bangbang93.com/maven/net/neoforged/forge/1.20.4/forge.jar",
            ),
            (
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.0/neoforge.jar",
                "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/20.4.0/neoforge.jar",
            ),
            (
                "https://meta.fabricmc.net/v2/versions/loader",
                "https://bmclapi2.bangbang93.com/fabric-meta/v2/versions/loader",
            ),
            (
                "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.0/loader.jar",
                "https://bmclapi2.bangbang93.com/maven/net/fabricmc/fabric-loader/0.15.0/loader.jar",
            ),
            (
                "https://authlib-injector.yushi.moe/artifact/latest.json",
                "https://bmclapi2.bangbang93.com/mirrors/authlib-injector/artifact/latest.json",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(
                DownloadSource::Bmclapi.rewrite_url(input),
                expected,
                "Failed to rewrite {input}"
            );
        }
    }

    #[test]
    fn test_rewrite_url_unmatched() {
        let external = "https://custom.repo.org/maven/test.jar";
        assert_eq!(DownloadSource::Bmclapi.rewrite_url(external), external);

        // Quilt is not currently proxied by BMCLAPI due to upstream API issues,
        // so Quilt URLs should remain untouched and connect directly to official endpoints.
        let quilt_meta = "https://meta.quiltmc.org/v3/versions/loader/1.21.1";
        assert_eq!(DownloadSource::Bmclapi.rewrite_url(quilt_meta), quilt_meta);

        let quilt_maven =
            "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-loader/test.jar";
        assert_eq!(
            DownloadSource::Bmclapi.rewrite_url(quilt_maven),
            quilt_maven
        );
    }

    #[test]
    fn test_download_source_serde() {
        let official_json = serde_json::to_string(&DownloadSource::Official).expect("serialize");
        assert_eq!(official_json, "\"official\"");
        let parsed_official: DownloadSource =
            serde_json::from_str(&official_json).expect("deserialize");
        assert_eq!(parsed_official, DownloadSource::Official);

        let bmclapi_json = serde_json::to_string(&DownloadSource::Bmclapi).expect("serialize");
        assert_eq!(bmclapi_json, "\"bmclapi\"");
        let parsed_bmclapi: DownloadSource =
            serde_json::from_str(&bmclapi_json).expect("deserialize");
        assert_eq!(parsed_bmclapi, DownloadSource::Bmclapi);
    }

    #[test]
    fn test_download_source_indices_and_all() {
        assert_eq!(
            DownloadSource::ALL,
            &[DownloadSource::Official, DownloadSource::Bmclapi]
        );

        assert_eq!(DownloadSource::Official.to_index(), 0);
        assert_eq!(DownloadSource::Bmclapi.to_index(), 1);

        assert_eq!(DownloadSource::from_index(0), DownloadSource::Official);
        assert_eq!(DownloadSource::from_index(1), DownloadSource::Bmclapi);

        for &source in DownloadSource::ALL {
            assert_eq!(DownloadSource::from_index(source.to_index()), source);
        }

        // Out-of-bounds indices should fall back to Official
        assert_eq!(DownloadSource::from_index(2), DownloadSource::Official);
        assert_eq!(DownloadSource::from_index(99), DownloadSource::Official);
        assert_eq!(
            DownloadSource::from_index(u32::MAX),
            DownloadSource::Official
        );
    }
}
