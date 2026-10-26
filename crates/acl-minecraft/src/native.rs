use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

use tracing::{debug, warn};
use zip::ZipArchive;

use crate::{MinecraftError, Result};

pub const DEFAULT_NATIVE_EXCLUDE: &str = "META-INF/";

#[derive(Debug, Clone)]
pub struct NativeArchive {
    /// Library coordinate name for logging.
    pub name: String,
    /// Full local path to the downloaded native `.jar` file.
    pub archive_path: PathBuf,
    /// Path prefixes to exclude during extraction (e.g., `["META-INF/"]`).
    pub exclude: Vec<String>,
}

impl NativeArchive {
    pub fn is_excluded(&self, entry_name: &str) -> bool {
        if entry_name.starts_with(DEFAULT_NATIVE_EXCLUDE) {
            return true;
        }

        self.exclude
            .iter()
            .any(|prefix| entry_name.starts_with(prefix))
    }
}

/// Extract a single native `.jar` archive into `natives_dir`.
pub fn extract_native_archive(archive: &NativeArchive, natives_dir: &Path) -> Result<()> {
    debug!(
        "Extracting native library '{}' from {}",
        archive.name,
        archive.archive_path.display()
    );

    let file = File::open(&archive.archive_path).map_err(|source| MinecraftError::Io {
        path: archive.archive_path.clone(),
        source,
    })?;

    let mut zip = ZipArchive::new(file).map_err(|source| MinecraftError::Zip {
        path: archive.archive_path.clone(),
        source,
    })?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|source| MinecraftError::Zip {
            path: archive.archive_path.clone(),
            source,
        })?;

        if entry.is_dir() {
            continue;
        }

        let entry_name = entry.name().to_string();

        if archive.is_excluded(&entry_name) {
            continue;
        }

        let rel_path = Path::new(&entry_name);

        if rel_path.as_os_str().is_empty()
            || rel_path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            warn!(
                "Skipping unsafe zip entry '{}' in native archive '{}'",
                entry_name, archive.name
            );
            continue;
        }

        // Flatten directory structure
        let Some(file_name) = rel_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Only extract Linux shared objects (.so).
        if !file_name.ends_with(".so") && !file_name.contains(".so.") {
            continue;
        }

        let dest_path = natives_dir.join(file_name);

        let mut buf = Vec::with_capacity(entry.size() as usize);

        entry
            .read_to_end(&mut buf)
            .map_err(|source| MinecraftError::Io {
                path: archive.archive_path.clone(),
                source,
            })?;

        acl_common::fs::write_atomic(&dest_path, &buf).map_err(|source| MinecraftError::Io {
            path: dest_path,
            source,
        })?;
    }

    Ok(())
}

pub fn extract_all_natives(archives: &[NativeArchive], natives_dir: &Path) -> Result<()> {
    for archive in archives {
        extract_native_archive(archive, natives_dir)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
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
                "acl_test_native_{}_{}_{}",
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

        zip.finish().expect("failed to finish zip");
    }

    #[test]
    fn test_native_archive_is_excluded() {
        let archive = NativeArchive {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            archive_path: PathBuf::from("/mock/lwjgl.jar"),
            exclude: vec!["custom/".to_string(), "ignored_dir/".to_string()],
        };

        // Default META-INF/ exclude
        assert!(archive.is_excluded("META-INF/MANIFEST.MF"));
        assert!(archive.is_excluded("META-INF/versions/9/module-info.class"));

        // Custom excludes
        assert!(archive.is_excluded("custom/libtest.so"));
        assert!(archive.is_excluded("ignored_dir/file.txt"));

        // Allowed files
        assert!(!archive.is_excluded("liblwjgl.so"));
        assert!(!archive.is_excluded("linux/x64/liblwjgl.so"));
        assert!(!archive.is_excluded("other/libfoo.so"));
    }

    #[test]
    fn test_extract_native_archive_filters_and_flattens() {
        let temp = TempDirGuard::new("extract_filter");
        let archive_path = temp.path().join("test_natives.jar");
        let natives_dir = temp.path().join("natives_out");
        std::fs::create_dir_all(&natives_dir).expect("failed to create natives output dir");

        create_test_zip(
            &archive_path,
            &[
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0"),
                (
                    "META-INF/dummy.so",
                    b"should be excluded by default META-INF/",
                ),
                ("exclude_me/libignored.so", b"custom excluded"),
                ("libvalid.so", b"ELF_ROOT_SO"),
                ("linux/x64/libnested.so", b"ELF_NESTED_SO"),
                ("libversioned.so.3.3", b"ELF_VERSIONED_SO"),
                ("windows.dll", b"DLL_FILE"),
                ("macos.dylib", b"DYLIB_FILE"),
                ("readme.txt", b"TEXT_FILE"),
                ("empty_dir/", b""),
            ],
        );

        let archive = NativeArchive {
            name: "org.lwjgl:lwjgl:3.3.3:natives-linux".to_string(),
            archive_path,
            exclude: vec!["exclude_me/".to_string()],
        };

        extract_native_archive(&archive, &natives_dir).expect("extraction should succeed");

        assert_eq!(
            std::fs::read(natives_dir.join("libvalid.so")).expect("libvalid.so should exist"),
            b"ELF_ROOT_SO"
        );
        assert_eq!(
            std::fs::read(natives_dir.join("libnested.so")).expect("libnested.so should exist"),
            b"ELF_NESTED_SO"
        );
        assert_eq!(
            std::fs::read(natives_dir.join("libversioned.so.3.3"))
                .expect("libversioned.so.3.3 should exist"),
            b"ELF_VERSIONED_SO"
        );

        assert!(!natives_dir.join("MANIFEST.MF").exists());
        assert!(!natives_dir.join("dummy.so").exists());
        assert!(!natives_dir.join("libignored.so").exists());
        assert!(!natives_dir.join("windows.dll").exists());
        assert!(!natives_dir.join("macos.dylib").exists());
        assert!(!natives_dir.join("readme.txt").exists());
    }

    #[test]
    fn test_extract_native_archive_unsafe_entries_skipped() {
        let temp = TempDirGuard::new("extract_unsafe");
        let archive_path = temp.path().join("unsafe.jar");
        let natives_dir = temp.path().join("natives_out");
        std::fs::create_dir_all(&natives_dir).expect("failed to create natives output dir");

        create_test_zip(
            &archive_path,
            &[("valid.so", b"VALID_SO"), ("../escaping.so", b"UNSAFE_SO")],
        );

        let archive = NativeArchive {
            name: "test:unsafe:1.0".to_string(),
            archive_path,
            exclude: Vec::new(),
        };

        extract_native_archive(&archive, &natives_dir)
            .expect("extraction should ignore unsafe entry");
        assert!(natives_dir.join("valid.so").exists());
        assert!(!temp.path().join("escaping.so").exists());
    }

    #[test]
    fn test_extract_native_archive_errors() {
        let temp = TempDirGuard::new("extract_errors");
        let non_existent = temp.path().join("non_existent.jar");
        let natives_dir = temp.path().join("natives_out");
        std::fs::create_dir_all(&natives_dir).expect("failed to create natives output dir");

        let archive = NativeArchive {
            name: "test:missing:1.0".to_string(),
            archive_path: non_existent.clone(),
            exclude: Vec::new(),
        };

        let err = extract_native_archive(&archive, &natives_dir)
            .expect_err("should fail when archive does not exist");
        assert!(matches!(err, MinecraftError::Io { path, .. } if path == non_existent));

        let invalid_zip_path = temp.path().join("corrupt.jar");
        std::fs::write(&invalid_zip_path, b"not a zip file").expect("failed to write corrupt file");
        let corrupt_archive = NativeArchive {
            name: "test:corrupt:1.0".to_string(),
            archive_path: invalid_zip_path.clone(),
            exclude: Vec::new(),
        };

        let err2 = extract_native_archive(&corrupt_archive, &natives_dir)
            .expect_err("should fail for invalid zip content");
        assert!(matches!(err2, MinecraftError::Zip { path, .. } if path == invalid_zip_path));
    }

    #[test]
    fn test_extract_all_natives() {
        let temp = TempDirGuard::new("extract_all");
        let jar1 = temp.path().join("jar1.jar");
        let jar2 = temp.path().join("jar2.jar");
        let natives_dir = temp.path().join("natives_out");
        std::fs::create_dir_all(&natives_dir).expect("failed to create natives output dir");

        create_test_zip(&jar1, &[("libone.so", b"ONE")]);
        create_test_zip(&jar2, &[("libtwo.so", b"TWO")]);

        let archives = vec![
            NativeArchive {
                name: "test:one:1.0".to_string(),
                archive_path: jar1,
                exclude: Vec::new(),
            },
            NativeArchive {
                name: "test:two:1.0".to_string(),
                archive_path: jar2,
                exclude: Vec::new(),
            },
        ];

        extract_all_natives(&archives, &natives_dir).expect("extract all should succeed");
        assert_eq!(
            std::fs::read(natives_dir.join("libone.so")).expect("libone.so should exist"),
            b"ONE"
        );
        assert_eq!(
            std::fs::read(natives_dir.join("libtwo.so")).expect("libtwo.so should exist"),
            b"TWO"
        );
    }
}
