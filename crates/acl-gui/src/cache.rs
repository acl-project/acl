// SPDX-FileCopyrightText: 2026 The Adwaita Craft Launcher Authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use tracing::debug;

pub(crate) async fn download_bytes(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Option<Vec<u8>> {
    let mut response = match client.get(url).timeout(timeout).send().await {
        Ok(res) => match res.error_for_status() {
            Ok(res) => res,
            Err(e) => {
                debug!("Failed to download {}: {}", url, e);
                return None;
            }
        },
        Err(e) => {
            debug!("Failed to download {}: {}", url, e);
            return None;
        }
    };

    if let Some(len) = response.content_length()
        && len > max_bytes as u64
    {
        debug!(
            "Failed to download {}: download bytes {} greater than {}",
            url, len, max_bytes
        );
        return None;
    }

    let mut bytes_read: usize = 0;
    let mut buf = Vec::new();

    while let Some(chunk) = response.chunk().await.ok()? {
        bytes_read = bytes_read.saturating_add(chunk.len());

        if bytes_read > max_bytes {
            debug!("Failed to download {}: download size exceeds limit", url,);
            return None;
        }

        buf.extend_from_slice(&chunk);
    }

    if buf.is_empty() {
        return None;
    }

    Some(buf)
}
