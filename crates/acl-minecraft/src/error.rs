// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MinecraftError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error at path '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Minecraft version '{version_id}' not found in manifest")]
    VersionNotFound { version_id: String },

    #[error("System clock error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    #[error("SHA-1 mismatch for '{target}': expected {expected}, actual {actual}")]
    HashMismatch {
        target: String,
        expected: String,
        actual: String,
    },

    #[error("Zip archive error at path '{path}': {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("Invalid Maven coordinate: '{coordinate}'")]
    InvalidMavenCoordinate { coordinate: String },

    #[error("Main-Class attribute not found in manifest of jar '{jar_path}'")]
    MissingMainClass { jar_path: PathBuf },

    #[error("Modern Forge install profile is missing version.json")]
    ForgeMissingVersionJson,

    #[error("Forge processor '{jar}' failed with exit code {exit_code}")]
    ProcessorFailed { jar: String, exit_code: i32 },

    #[error("Failed to construct classpath: {0}")]
    Classpath(#[from] std::env::JoinPathsError),

    #[error("Modpack index JSON not found in '{path}'")]
    ModpackIndexNotFound { path: PathBuf },

    #[error("Archive is not a recognized modpack format")]
    ModpackUnknownFormat,

    #[error("Modpack index file exceeds maximum allowed size ({max_bytes} bytes)")]
    ModpackIndexTooLarge { max_bytes: u64 },

    #[error("Unsupported modpack format version: {version}")]
    ModpackUnsupportedFormatVersion { version: u32 },

    #[error("Unsupported modpack game: '{game}'")]
    ModpackUnsupportedGame { game: String },

    #[error("Modpack is missing the required Minecraft version")]
    ModpackMissingMinecraftVersion,

    #[error("Modpack dependency '{dependency}' has an empty version")]
    ModpackEmptyDependencyVersion { dependency: String },

    #[error("Unsupported modpack dependency: '{dependency}'")]
    ModpackUnsupportedDependency { dependency: String },

    #[error("Unsafe file path in modpack: '{path}'")]
    ModpackUnsafePath { path: String },

    #[error("Modpack contains an empty file path")]
    ModpackEmptyPath,

    #[error("Modpack file '{path}' has no download URLs")]
    ModpackMissingDownloads { path: String },

    #[error("Modpack specifies multiple mod loaders")]
    ModpackMultipleModLoaders,
}

pub type Result<T> = std::result::Result<T, MinecraftError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minecraft_error_display() {
        let err = MinecraftError::VersionNotFound {
            version_id: "1.21".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Minecraft version '1.21' not found in manifest"
        );

        let io_err = MinecraftError::Io {
            path: PathBuf::from("/test/path"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file missing"),
        };
        assert_eq!(
            io_err.to_string(),
            "I/O error at path '/test/path': file missing"
        );

        let hash_err = MinecraftError::HashMismatch {
            target: "1.21.json".to_string(),
            expected: "abcdef".to_string(),
            actual: "123456".to_string(),
        };
        assert_eq!(
            hash_err.to_string(),
            "SHA-1 mismatch for '1.21.json': expected abcdef, actual 123456"
        );

        let zip_err = MinecraftError::Zip {
            path: PathBuf::from("/test/archive.jar"),
            source: zip::result::ZipError::FileNotFound,
        };
        assert_eq!(
            zip_err.to_string(),
            "Zip archive error at path '/test/archive.jar': specified file not found in archive"
        );

        let coord_err = MinecraftError::InvalidMavenCoordinate {
            coordinate: "invalid:coord".to_string(),
        };
        assert_eq!(
            coord_err.to_string(),
            "Invalid Maven coordinate: 'invalid:coord'"
        );

        let main_class_err = MinecraftError::MissingMainClass {
            jar_path: PathBuf::from("/test/proc.jar"),
        };
        assert_eq!(
            main_class_err.to_string(),
            "Main-Class attribute not found in manifest of jar '/test/proc.jar'"
        );

        let missing_vjson_err = MinecraftError::ForgeMissingVersionJson;
        assert_eq!(
            missing_vjson_err.to_string(),
            "Modern Forge install profile is missing version.json"
        );

        let proc_err = MinecraftError::ProcessorFailed {
            jar: "net.minecraftforge:installertools:1.4.1".to_string(),
            exit_code: 1,
        };
        assert_eq!(
            proc_err.to_string(),
            "Forge processor 'net.minecraftforge:installertools:1.4.1' failed with exit code 1"
        );
    }

    #[test]
    fn test_modpack_error_display() {
        let index_not_found = MinecraftError::ModpackIndexNotFound {
            path: PathBuf::from("/test/pack.mrpack"),
        };
        assert_eq!(
            index_not_found.to_string(),
            "Modpack index JSON not found in '/test/pack.mrpack'"
        );

        assert_eq!(
            MinecraftError::ModpackUnknownFormat.to_string(),
            "Archive is not a recognized modpack format"
        );

        let too_large = MinecraftError::ModpackIndexTooLarge {
            max_bytes: 16_777_216,
        };
        assert_eq!(
            too_large.to_string(),
            "Modpack index file exceeds maximum allowed size (16777216 bytes)"
        );

        let unsupported_ver = MinecraftError::ModpackUnsupportedFormatVersion { version: 2 };
        assert_eq!(
            unsupported_ver.to_string(),
            "Unsupported modpack format version: 2"
        );

        let unsupported_game = MinecraftError::ModpackUnsupportedGame {
            game: "terraria".to_string(),
        };
        assert_eq!(
            unsupported_game.to_string(),
            "Unsupported modpack game: 'terraria'"
        );

        assert_eq!(
            MinecraftError::ModpackMissingMinecraftVersion.to_string(),
            "Modpack is missing the required Minecraft version"
        );

        let empty_dep = MinecraftError::ModpackEmptyDependencyVersion {
            dependency: "fabric-loader".to_string(),
        };
        assert_eq!(
            empty_dep.to_string(),
            "Modpack dependency 'fabric-loader' has an empty version"
        );

        let unsupported_dep = MinecraftError::ModpackUnsupportedDependency {
            dependency: "liteloader".to_string(),
        };
        assert_eq!(
            unsupported_dep.to_string(),
            "Unsupported modpack dependency: 'liteloader'"
        );

        let unsafe_path = MinecraftError::ModpackUnsafePath {
            path: "../evil.jar".to_string(),
        };
        assert_eq!(
            unsafe_path.to_string(),
            "Unsafe file path in modpack: '../evil.jar'"
        );

        assert_eq!(
            MinecraftError::ModpackEmptyPath.to_string(),
            "Modpack contains an empty file path"
        );

        let missing_downloads = MinecraftError::ModpackMissingDownloads {
            path: "mods/a.jar".to_string(),
        };
        assert_eq!(
            missing_downloads.to_string(),
            "Modpack file 'mods/a.jar' has no download URLs"
        );

        assert_eq!(
            MinecraftError::ModpackMultipleModLoaders.to_string(),
            "Modpack specifies multiple mod loaders"
        );
    }
}
