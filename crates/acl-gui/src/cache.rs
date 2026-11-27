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
            debug!("Failed to download {}: download size exceeds limit", url);
            return None;
        }

        buf.extend_from_slice(&chunk);
    }

    if buf.is_empty() {
        return None;
    }

    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relm4::tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn test_runtime() -> relm4::tokio::runtime::Runtime {
        relm4::tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build test tokio runtime")
    }

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("failed to build test HTTP client")
    }

    #[test]
    fn test_download_bytes_success_and_error_cases() {
        let rt = test_runtime();
        let client = test_client();
        let max_bytes = 64usize;

        rt.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock server");
            let addr = listener.local_addr().expect("get local addr");

            let server = relm4::tokio::spawn(async move {
                for _ in 0..6 {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        break;
                    };
                    let mut buf = [0u8; 1024];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /ok ") {
                        let body = b"hello-bytes";
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                        let _ = stream.write_all(body).await;
                    } else if req.starts_with("GET /404 ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /empty ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .await;
                    } else if req.starts_with("GET /too-large-header ") {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            max_bytes + 1
                        );
                        let _ = stream.write_all(header.as_bytes()).await;
                    } else if req.starts_with("GET /too-large-stream ") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                            .await;
                        let payload = vec![b'a'; max_bytes + 16];
                        let _ = stream.write_all(&payload).await;
                    } else if req.starts_with("GET /slow ") {
                        relm4::tokio::time::sleep(Duration::from_millis(300)).await;
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                            .await;
                    }
                    let _ = stream.flush().await;
                }
            });

            let timeout = Duration::from_secs(5);
            assert_eq!(
                download_bytes(&client, &format!("http://{addr}/ok"), max_bytes, timeout).await,
                Some(b"hello-bytes".to_vec())
            );
            assert_eq!(
                download_bytes(&client, &format!("http://{addr}/404"), max_bytes, timeout).await,
                None
            );
            assert_eq!(
                download_bytes(&client, &format!("http://{addr}/empty"), max_bytes, timeout).await,
                None
            );
            assert_eq!(
                download_bytes(
                    &client,
                    &format!("http://{addr}/too-large-header"),
                    max_bytes,
                    timeout
                )
                .await,
                None
            );
            assert_eq!(
                download_bytes(
                    &client,
                    &format!("http://{addr}/too-large-stream"),
                    max_bytes,
                    timeout
                )
                .await,
                None
            );
            assert_eq!(
                download_bytes(
                    &client,
                    &format!("http://{addr}/slow"),
                    max_bytes,
                    Duration::from_millis(50)
                )
                .await,
                None
            );

            let _ = server.await;
        });
    }
}
