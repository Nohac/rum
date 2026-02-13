use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::io::AsyncWriteExt;

use crate::error::RumError;

/// Download a response body to a file, updating the progress bar as chunks arrive.
async fn download_to_file(
    path: &Path,
    response: reqwest::Response,
    pb: &ProgressBar,
) -> Result<(), RumError> {
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| RumError::Io {
            context: format!("creating temp file {}", path.display()),
            source: e,
        })?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| RumError::ImageDownload {
            message: "error reading response body".into(),
            source: Box::new(e),
        })?;
        file.write_all(&chunk).await.map_err(|e| RumError::Io {
            context: "writing image data".into(),
            source: e,
        })?;
        pb.inc(chunk.len() as u64);
    }

    file.flush().await.map_err(|e| RumError::Io {
        context: "flushing image file".into(),
        source: e,
    })?;

    Ok(())
}

/// Ensure the base image is available locally, downloading if needed.
/// Returns the path to the cached image file.
pub async fn ensure_base_image(base: &str, cache_dir: &Path) -> Result<PathBuf, RumError> {
    if !base.starts_with("http://") && !base.starts_with("https://") {
        let path = PathBuf::from(base);
        if !path.exists() {
            return Err(RumError::Io {
                context: format!("base image not found: {}", path.display()),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
            });
        }
        return Ok(path);
    }

    let filename = base.rsplit('/').next().unwrap_or("image.img");

    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| RumError::Io {
            context: format!("creating cache dir {}", cache_dir.display()),
            source: e,
        })?;

    let dest = cache_dir.join(filename);
    if dest.exists() {
        tracing::info!(path = %dest.display(), "using cached base image");
        return Ok(dest);
    }

    tracing::info!(url = %base, "downloading base image");

    let response = reqwest::get(base)
        .await
        .map_err(|e| RumError::ImageDownload {
            message: format!("request to {base} failed"),
            source: Box::new(e),
        })?;

    if !response.status().is_success() {
        return Err(RumError::ImageDownload {
            message: format!("HTTP {} from {base}", response.status()),
            source: format!("HTTP {}", response.status()).into(),
        });
    }

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let tmp_path = dest.with_extension("part");

    // Remove any stale .part file from a previous failed download
    let _ = tokio::fs::remove_file(&tmp_path).await;

    if let Err(e) = download_to_file(&tmp_path, response, &pb).await {
        // Clean up the .part file on failure
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e);
    }

    tokio::fs::rename(&tmp_path, &dest)
        .await
        .map_err(|e| RumError::Io {
            context: format!("renaming {} to {}", tmp_path.display(), dest.display()),
            source: e,
        })?;

    pb.finish_with_message("download complete");
    tracing::info!(path = %dest.display(), "base image cached");

    Ok(dest)
}
