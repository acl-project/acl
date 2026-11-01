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
    (
        "https://maven.quiltmc.org/repository/release",
        "https://bmclapi2.bangbang93.com/maven",
    ),
    (
        "https://meta.quiltmc.org",
        "https://bmclapi2.bangbang93.com/quilt-meta",
    ),
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
}
