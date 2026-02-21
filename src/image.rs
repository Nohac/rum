use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

/// Check whether the base image is already available locally (no download needed).
pub fn is_cached(base: &str, cache_dir: &Path) -> bool {
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Path::new(base).exists();
    }
    let filename = base.rsplit('/').next().unwrap_or("image.img");
    cache_dir.join(filename).exists()
}

/// Ensure the base image is available locally, downloading if needed.
/// Returns the path to the cached image file.
pub async fn ensure_base_image(
    base: &str,
    cache_dir: &Path,
) -> Result<PathBuf, RumError> {
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

    pb.finish_and_clear();
    tracing::info!(path = %dest.display(), "base image cached");

    Ok(dest)
}

/// List all cached images with filename, size, and modification time.
pub fn list_cached(cache_dir: &Path) -> Result<(), RumError> {
    if !cache_dir.exists() {
        println!("No cached images.");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(cache_dir)
        .map_err(|e| RumError::Io {
            context: format!("reading cache directory {}", cache_dir.display()),
            source: e,
        })?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();

    if entries.is_empty() {
        println!("No cached images.");
        return Ok(());
    }

    entries.sort_by_key(|e| e.file_name());

    let mut total_size: u64 = 0;
    for entry in &entries {
        let meta = entry.metadata().map_err(|e| RumError::Io {
            context: format!("reading metadata for {}", entry.path().display()),
            source: e,
        })?;
        let size = meta.len();
        total_size += size;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| time_from_epoch(d.as_secs()))
            .unwrap_or_else(|| "unknown".into());
        println!(
            "  {}  {}  {}",
            entry.file_name().to_string_lossy(),
            format_size(size),
            modified
        );
    }
    println!("\n{} image(s), {} total", entries.len(), format_size(total_size));

    Ok(())
}

/// Delete a specific cached image by filename.
pub fn delete_cached(cache_dir: &Path, name: &str) -> Result<(), RumError> {
    let path = cache_dir.join(name);
    if !path.exists() {
        return Err(RumError::Io {
            context: format!(
                "cached image '{}' not found in {}",
                name,
                cache_dir.display()
            ),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        });
    }
    let meta = std::fs::metadata(&path).map_err(|e| RumError::Io {
        context: format!("reading metadata for {}", path.display()),
        source: e,
    })?;
    std::fs::remove_file(&path).map_err(|e| RumError::Io {
        context: format!("deleting {}", path.display()),
        source: e,
    })?;
    println!("Deleted '{}' ({})", name, format_size(meta.len()));
    Ok(())
}

/// Delete all cached images.
pub fn clear_cache(cache_dir: &Path) -> Result<(), RumError> {
    if !cache_dir.exists() {
        println!("No cached images.");
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(cache_dir)
        .map_err(|e| RumError::Io {
            context: format!("reading cache directory {}", cache_dir.display()),
            source: e,
        })?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .collect();

    if entries.is_empty() {
        println!("No cached images.");
        return Ok(());
    }

    let total_size: u64 = entries
        .iter()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();

    for entry in &entries {
        std::fs::remove_file(entry.path()).map_err(|e| RumError::Io {
            context: format!("deleting {}", entry.path().display()),
            source: e,
        })?;
    }
    println!(
        "Deleted {} image(s) ({})",
        entries.len(),
        format_size(total_size)
    );
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn time_from_epoch(secs: u64) -> String {
    // Simple date formatting without external deps
    // Format: YYYY-MM-DD HH:MM
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;

    // Days since epoch to date (simplified Gregorian)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let year_days = if is_leap(y) { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        y += 1;
    }
    let month_days: [i64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i;
            break;
        }
        remaining_days -= md;
    }
    let d = remaining_days + 1;
    format!("{y:04}-{:02}-{d:02} {hours:02}:{minutes:02}", m + 1)
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
