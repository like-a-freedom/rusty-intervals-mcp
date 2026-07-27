//! File-download endpoints.
//!
//! Three thin wrappers around the private `download_file` helper that lives in
//! [`http_client::mod`], plus `download_activity_file_with_progress` which
//! streams the body and reports progress (or honours a cancellation signal).

use crate::http_client::ReqwestIntervalsClient;
use crate::{IntervalsError, Result, TransportError};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    pub(crate) async fn fetch_download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/file", self.base_url, activity_id);
        let resp = self
            .execute_raw(self.request(reqwest::Method::GET, &url))
            .await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }

        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        if let Some(path) = output_path {
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&path).await?;
            let mut downloaded: u64 = 0;

            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            return Err(IntervalsError::Cancelled {
                                reason: "download cancelled".to_string(),
                            });
                        }
                        continue;
                    }
                    c = stream.next() => c,
                };

                let Some(chunk) = chunk else { break };

                let bytes = chunk
                    .map_err(TransportError::from)
                    .map_err(IntervalsError::from)?;
                file.write_all(&bytes).await?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);

                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });

                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Cancelled {
                        reason: "download cancelled".to_string(),
                    });
                }
            }

            file.sync_all().await?;
            Ok(Some(path.to_string_lossy().to_string()))
        } else {
            let mut stream = resp.bytes_stream();
            let mut downloaded: u64 = 0;
            let mut all_bytes = Vec::new();

            while let Some(chunk) = stream.next().await {
                let bytes = chunk
                    .map_err(TransportError::from)
                    .map_err(IntervalsError::from)?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);
                all_bytes.extend_from_slice(&bytes);

                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });

                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Cancelled {
                        reason: "download cancelled".to_string(),
                    });
                }
            }

            Ok(Some(STANDARD.encode(&all_bytes)))
        }
    }

    pub(crate) async fn fetch_download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/fit-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    pub(crate) async fn fetch_download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/gpx-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }
}
