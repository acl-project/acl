use tokio::{
    sync::{Semaphore, mpsc::UnboundedSender},
    task::JoinSet,
};
use tracing::{debug, warn};

use crate::{MinecraftError, Result, cache::verify_sha1};
use std::{path::PathBuf, sync::Arc};
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

    let response = client.get(&task.url).send().await?.error_for_status()?;
    let bytes = response.bytes().await?;

    verify_sha1(&bytes, &task.sha1, &task.name)?;

    acl_common::fs::write_atomic(&task.destination, &bytes).map_err(|source| {
        MinecraftError::Io {
            path: task.destination.clone(),
            source,
        }
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
