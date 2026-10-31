use tokio::{
    sync::{Semaphore, mpsc::UnboundedSender},
    task::JoinSet,
};
use tracing::{debug, error, warn};

use crate::{MinecraftError, Result, cache::verify_sha1};
use std::{path::PathBuf, sync::Arc, time::Duration};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTask {
    /// Label name used for logging.
    pub name: String,
    pub url: String,
    /// The fully assembled complete local save path.
    pub destination: PathBuf,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub completed_files: usize,
    pub total_files: usize,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

pub async fn download_and_verify_file(client: &reqwest::Client, task: &DownloadTask) -> Result<()> {
    if task.destination.exists() {
        match tokio::fs::read(&task.destination).await {
            Ok(bytes) => {
                if verify_sha1(&bytes, &task.sha1, &task.name).is_ok() {
                    debug!(
                        "Verified existing file for '{}' at {}",
                        task.name,
                        task.destination.display()
                    );
                    return Ok(());
                }
                warn!(
                    "Existing file at {} failed SHA-1 verification, re-downloading",
                    task.destination.display()
                );
            }

            Err(err) => {
                warn!(
                    "Failed to read existing file at {}: {err}, re-downloading",
                    task.destination.display()
                );
            }
        }
    }

    let mut last_error = None;
    let mut downloaded_bytes = None;

    const MAX_RETRIES: usize = 3;

    // Retry multiple times to prevent download failures caused by network fluctuations.
    for attempt in 1..=MAX_RETRIES {
        let result = async {
            let response = client.get(&task.url).send().await?.error_for_status()?;
            let bytes = response.bytes().await?;

            verify_sha1(&bytes, &task.sha1, &task.name)?;
            Ok(bytes)
        }
        .await;

        match result {
            Ok(bytes) => {
                downloaded_bytes = Some(bytes);
                break;
            }
            Err(err) => {
                warn!(
                    "Download attempt {attempt}/{MAX_RETRIES} failed for '{}': {err}",
                    task.name
                );
                last_error = Some(err);
                if attempt < MAX_RETRIES {
                    // Backoff delay: wait 300 ms on the first attempt, 600 ms on the second.
                    tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
                }
            }
        }
    }

    let Some(bytes) = downloaded_bytes else {
        error!("Failed to download {}", task.url);
        let err = last_error.unwrap_or_else(|| MinecraftError::Io {
            path: task.destination.clone(),
            source: std::io::Error::other("Max retries exceeded without error"),
        });
        return Err(err);
    };

    if let Some(parent) = task.destination.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|source| MinecraftError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }

    // Write directly to disk.
    tokio::fs::write(&task.destination, bytes)
        .await
        .map_err(|source| MinecraftError::Io {
            path: task.destination.clone(),
            source,
        })?;

    Ok(())
}

pub async fn download_all(
    client: &reqwest::Client,
    tasks: Vec<DownloadTask>,
    max_concurrent: usize,
    progress_tx: Option<UnboundedSender<DownloadProgress>>,
) -> Result<()> {
    let total_files = tasks.len();
    let total_bytes: u64 = tasks.iter().map(|t| t.size).sum();

    if total_files == 0 {
        if let Some(tx) = &progress_tx {
            let _ = tx.send(DownloadProgress {
                completed_files: 0,
                total_files: 0,
                completed_bytes: 0,
                total_bytes: 0,
            });
        }

        return Ok(());
    }

    let concurrency = max_concurrent.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut join_set = JoinSet::new();

    for task in tasks {
        let client = client.clone();
        let semaphore = Arc::clone(&semaphore);

        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .map_err(|err| MinecraftError::Io {
                    path: task.destination.clone(),
                    source: std::io::Error::other(err),
                })?;
            download_and_verify_file(&client, &task).await?;
            Ok::<u64, MinecraftError>(task.size)
        });
    }

    let mut completed_files = 0;
    let mut completed_bytes = 0;

    while let Some(join_res) = join_set.join_next().await {
        // Check whether the thread task has panicked / been cancelled.
        let task_result = join_res.map_err(|err| MinecraftError::Io {
            path: std::path::PathBuf::from("<download_task>"),
            source: std::io::Error::other(err),
        })?;

        // Check whether an error occurred during the download or SHA-1 verification.
        let file_size = task_result?;

        completed_files += 1;
        completed_bytes += file_size;

        if let Some(tx) = &progress_tx {
            let _ = tx.send(DownloadProgress {
                completed_files,
                total_files,
                completed_bytes,
                total_bytes,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "acl_test_download_{}_{}_{}",
                name,
                std::process::id(),
                id
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn calculate_sha1(bytes: &[u8]) -> String {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("failed to create test client")
    }

    async fn spawn_mock_server(payload: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind mock server");
        let local_addr = listener.local_addr().expect("failed to get local addr");

        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let payload = payload.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(&payload).await;
                    let _ = stream.flush().await;
                });
            }
        });

        (format!("http://{local_addr}"), handle)
    }

    async fn spawn_failing_mock_server(
        failures_before_success: usize,
        payload: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind mock server");
        let local_addr = listener.local_addr().expect("failed to get local addr");
        let req_count = Arc::new(AtomicUsize::new(0));

        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let count = req_count.fetch_add(1, Ordering::SeqCst);
                let payload = payload.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;
                    if count < failures_before_success {
                        let header = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(header.as_bytes()).await;
                    } else {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            payload.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(&payload).await;
                    }
                    let _ = stream.flush().await;
                });
            }
        });

        (format!("http://{local_addr}"), handle)
    }

    #[tokio::test]
    async fn test_download_all_empty_tasks() {
        let client = reqwest::Client::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let result = download_all(&client, Vec::new(), 4, Some(tx)).await;
        assert!(result.is_ok());

        let progress = rx.recv().await.expect("should receive initial progress");
        assert_eq!(
            progress,
            DownloadProgress {
                completed_files: 0,
                total_files: 0,
                completed_bytes: 0,
                total_bytes: 0,
            }
        );
    }

    #[tokio::test]
    async fn test_download_and_verify_file_reuse_existing() {
        let temp = TempDirGuard::new("reuse_existing");
        let dest = temp.path().join("existing.txt");
        let content = b"reused local content";
        std::fs::write(&dest, content).expect("failed to write test file");

        let sha1 = calculate_sha1(content);
        let task = DownloadTask {
            name: "existing.txt".to_string(),
            url: "http://127.0.0.1:9/unused".to_string(),
            destination: dest,
            sha1,
            size: content.len() as u64,
        };

        let client = reqwest::Client::new();
        let result = download_and_verify_file(&client, &task).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_download_and_verify_file_network_success() {
        let temp = TempDirGuard::new("network_success");
        let dest = temp.path().join("sub").join("downloaded.bin");
        let payload = b"downloadable content for minecraft".to_vec();
        let sha1 = calculate_sha1(&payload);

        let (server_url, server_handle) = spawn_mock_server(payload.clone()).await;

        let task = DownloadTask {
            name: "downloaded.bin".to_string(),
            url: format!("{server_url}/downloaded.bin"),
            destination: dest.clone(),
            sha1,
            size: payload.len() as u64,
        };

        let client = test_client();
        let result = download_and_verify_file(&client, &task).await;
        assert!(result.is_ok());

        assert!(dest.exists());
        let read_bytes = std::fs::read(&dest).expect("failed to read downloaded file");
        assert_eq!(read_bytes, payload);

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_download_and_verify_file_hash_mismatch() {
        let temp = TempDirGuard::new("hash_mismatch");
        let dest = temp.path().join("corrupted.bin");
        let payload = b"actual server content".to_vec();

        let (server_url, server_handle) = spawn_mock_server(payload.clone()).await;

        let task = DownloadTask {
            name: "corrupted.bin".to_string(),
            url: format!("{server_url}/corrupted.bin"),
            destination: dest.clone(),
            sha1: "0000000000000000000000000000000000000000".to_string(),
            size: payload.len() as u64,
        };

        let client = test_client();
        let result = download_and_verify_file(&client, &task).await;
        match result {
            Err(MinecraftError::HashMismatch {
                target, expected, ..
            }) => {
                assert_eq!(target, "corrupted.bin");
                assert_eq!(expected, "0000000000000000000000000000000000000000");
            }
            other => panic!("expected HashMismatch error, got {other:?}"),
        }

        assert!(!dest.exists());
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_download_all_multiple_files_and_progress() {
        let temp = TempDirGuard::new("download_all");
        let payload = b"shared payload".to_vec();
        let sha1 = calculate_sha1(&payload);

        let (server_url, server_handle) = spawn_mock_server(payload.clone()).await;
        let client = test_client();

        let tasks = vec![
            DownloadTask {
                name: "file1.bin".to_string(),
                url: format!("{server_url}/file1.bin"),
                destination: temp.path().join("file1.bin"),
                sha1: sha1.clone(),
                size: payload.len() as u64,
            },
            DownloadTask {
                name: "file2.bin".to_string(),
                url: format!("{server_url}/file2.bin"),
                destination: temp.path().join("file2.bin"),
                sha1: sha1.clone(),
                size: payload.len() as u64,
            },
            DownloadTask {
                name: "file3.bin".to_string(),
                url: format!("{server_url}/file3.bin"),
                destination: temp.path().join("file3.bin"),
                sha1,
                size: payload.len() as u64,
            },
        ];

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let res = download_all(&client, tasks, 2, Some(tx)).await;
        assert!(res.is_ok());

        let mut final_progress = None;
        while let Ok(progress) = rx.try_recv() {
            final_progress = Some(progress);
        }

        let progress = final_progress.expect("should have received progress");
        assert_eq!(progress.completed_files, 3);
        assert_eq!(progress.total_files, 3);
        assert_eq!(progress.completed_bytes, (payload.len() * 3) as u64);
        assert_eq!(progress.total_bytes, (payload.len() * 3) as u64);

        assert!(temp.path().join("file1.bin").exists());
        assert!(temp.path().join("file2.bin").exists());
        assert!(temp.path().join("file3.bin").exists());

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_download_and_verify_file_retry_success() {
        let temp = TempDirGuard::new("retry_success");
        let dest = temp.path().join("retry.bin");
        let payload = b"retry payload content".to_vec();
        let sha1 = calculate_sha1(&payload);

        let (server_url, server_handle) = spawn_failing_mock_server(1, payload.clone()).await;
        let client = test_client();

        let task = DownloadTask {
            name: "retry.bin".to_string(),
            url: format!("{server_url}/retry.bin"),
            destination: dest.clone(),
            sha1,
            size: payload.len() as u64,
        };

        let result = download_and_verify_file(&client, &task).await;
        assert!(result.is_ok());

        assert!(dest.exists());
        let read_bytes = std::fs::read(&dest).expect("failed to read downloaded file");
        assert_eq!(read_bytes, payload);

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_download_and_verify_file_retry_exhausted() {
        let temp = TempDirGuard::new("retry_exhausted");
        let dest = temp.path().join("exhausted.bin");
        let payload = b"unreachable content".to_vec();
        let sha1 = calculate_sha1(&payload);

        // Fail 5 times (exceeding MAX_RETRIES = 3)
        let (server_url, server_handle) = spawn_failing_mock_server(5, payload).await;
        let client = test_client();

        let task = DownloadTask {
            name: "exhausted.bin".to_string(),
            url: format!("{server_url}/exhausted.bin"),
            destination: dest.clone(),
            sha1,
            size: 10,
        };

        let result = download_and_verify_file(&client, &task).await;
        assert!(result.is_err());
        assert!(!dest.exists());

        server_handle.abort();
    }
}
