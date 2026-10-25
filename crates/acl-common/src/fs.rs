use std::{io::Write, path::Path};

pub fn write_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut tmp_os_str = path.as_os_str().to_os_string();
    // Prevent interference caused by multiple processes.
    tmp_os_str.push(format!(".tmp.{}", std::process::id()));
    let tmp_path = std::path::PathBuf::from(tmp_os_str);

    let res = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
                "acl_test_common_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_write_atomic_basic() {
        let temp = TempDirGuard::new("basic");
        let file_path = temp.path().join("test.txt");

        write_atomic(&file_path, b"hello atomic").expect("failed to write atomic");

        let content = fs::read(&file_path).expect("failed to read written file");
        assert_eq!(content, b"hello atomic");

        let tmp_path = format!("{}.tmp.{}", file_path.display(), std::process::id());
        assert!(!Path::new(&tmp_path).exists());
    }

    #[test]
    fn test_write_atomic_overwrite() {
        let temp = TempDirGuard::new("overwrite");
        let file_path = temp.path().join("test.txt");

        write_atomic(&file_path, b"initial").expect("failed initial write");
        write_atomic(&file_path, b"updated").expect("failed overwrite");

        let content = fs::read_to_string(&file_path).expect("failed to read");
        assert_eq!(content, "updated");
    }

    #[test]
    fn test_write_atomic_creates_parent_dirs() {
        let temp = TempDirGuard::new("nested");
        let file_path = temp.path().join("a").join("b").join("c").join("nested.txt");

        write_atomic(&file_path, b"nested content").expect("failed nested write");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).expect("failed to read");
        assert_eq!(content, "nested content");
    }

    #[test]
    fn test_write_atomic_when_target_has_tmp_extension() {
        let temp = TempDirGuard::new("tmp_ext");
        let file_path = temp.path().join("file.tmp");

        write_atomic(&file_path, b"temp data").expect("failed write to .tmp file");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).expect("failed to read");
        assert_eq!(content, "temp data");
    }
}
