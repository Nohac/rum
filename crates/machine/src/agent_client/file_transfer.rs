use std::path::{Path, PathBuf};

use agent::{FileChunk, WriteFileInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

use crate::error::RumError;

pub enum CopyDirection {
    Upload { local: PathBuf, guest: String },
    Download { guest: String, local: PathBuf },
}

pub fn parse_copy_args(src: &str, dst: &str) -> Result<CopyDirection, RumError> {
    let src_guest = src.starts_with(':');
    let dst_guest = dst.starts_with(':');

    match (src_guest, dst_guest) {
        (false, true) => Ok(CopyDirection::Upload {
            local: PathBuf::from(src),
            guest: dst[1..].to_string(),
        }),
        (true, false) => Ok(CopyDirection::Download {
            guest: src[1..].to_string(),
            local: PathBuf::from(dst),
        }),
        (true, true) => Err(RumError::CopyFailed {
            message: "both paths have : prefix — guest-to-guest copy is not supported".into(),
        }),
        (false, false) => Err(RumError::CopyFailed {
            message: "neither path has a : prefix — prefix the guest path with :".into(),
        }),
    }
}

pub async fn copy_to_guest(cid: u32, local: &Path, guest_path: &str) -> Result<u64, RumError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = tokio::fs::metadata(local).await.map_err(|e| RumError::CopyFailed {
        message: format!("{}: {e}", local.display()),
    })?;
    let mode = metadata.permissions().mode();
    let size = metadata.len();
    let filename = local
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let agent = super::wait_for_agent(cid).await?;
    let (tx, rx) = roam::channel::<FileChunk>();
    let local_owned = local.to_path_buf();
    let send_task = tokio::spawn(async move {
        let file = tokio::fs::File::open(&local_owned).await?;
        let mut reader = BufReader::new(file);
        const CHUNK_SIZE: usize = 10 * 1024 * 1024;
        let mut buf = vec![0u8; CHUNK_SIZE];

        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let chunk = FileChunk {
                data: buf[..n].to_vec(),
            };
            if tx.send(&chunk).await.is_err() {
                break;
            }
        }

        Ok::<(), std::io::Error>(())
    });

    let info = WriteFileInfo {
        path: guest_path.to_string(),
        filename,
        mode,
        size,
    };

    let result = agent
        .write_file(info, rx)
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("write_file RPC: {e}"),
        })?;

    send_task
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("send task: {e}"),
        })?
        .map_err(|e| RumError::CopyFailed {
            message: format!("send: {e}"),
        })?;

    Ok(result.bytes_written)
}

pub async fn copy_from_guest(cid: u32, guest_path: &str, local: &Path) -> Result<u64, RumError> {
    use std::os::unix::fs::PermissionsExt;

    let agent = super::wait_for_agent(cid).await?;
    let (tx, mut rx) = roam::channel::<FileChunk>();
    let guest_owned = guest_path.to_string();
    let read_task = tokio::spawn(async move { agent.read_file(guest_owned, tx).await });

    let guest_filename = Path::new(guest_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let final_path = if local.is_dir() {
        local.join(&guest_filename)
    } else {
        local.to_path_buf()
    };

    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| RumError::CopyFailed {
            message: format!("create dirs: {e}"),
        })?;
    }

    let file = tokio::fs::File::create(&final_path)
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("{}: {e}", final_path.display()),
        })?;
    let mut writer = BufWriter::new(file);
    let mut bytes_written = 0_u64;

    while let Ok(Some(chunk)) = rx.recv().await {
        writer.write_all(&chunk.data).await.map_err(|e| RumError::CopyFailed {
            message: format!("write: {e}"),
        })?;
        bytes_written += chunk.data.len() as u64;
    }

    writer.flush().await.map_err(|e| RumError::CopyFailed {
        message: format!("flush: {e}"),
    })?;

    let result = read_task
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("read task: {e}"),
        })?
        .map_err(|e| RumError::CopyFailed {
            message: format!("read_file RPC: {e}"),
        })?;

    tokio::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(result.mode))
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("chmod: {e}"),
        })?;

    Ok(bytes_written)
}
